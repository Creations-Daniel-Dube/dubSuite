use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct GhAsset {
    pub name: String,
    #[serde(default)]
    pub browser_download_url: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Clone)]
pub struct RemoteFile {
    pub name: String,
    pub browser_url: String,
    pub api_url: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub version: String,
    pub published_label: String,
    pub package: RemoteFile,
    pub checksum: Option<RemoteFile>,
}

pub fn http_client(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(15))
        .user_agent(format!("dubSuite/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| format!("Client HTTP indisponible : {err}"))
}

pub fn fetch_latest(repo: &str, token: &str) -> Result<Option<ReleaseInfo>, String> {
    let client = http_client(Duration::from_secs(30))?;
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let mut request = client.get(&url).headers(api_headers(token));
    if let Some(value) = auth_header(token) {
        request = request.header(AUTHORIZATION, value);
    }
    let response = request
        .send()
        .map_err(|err| format!("GitHub injoignable : {err}"))?;
    let status = response.status();
    if status.as_u16() == 404 {
        return Ok(None);
    }
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(http_error(status.as_u16(), &body));
    }
    let release: GhRelease = response
        .json()
        .map_err(|err| format!("Réponse GitHub illisible : {err}"))?;
    match select_release(&release, &platform_tags()) {
        Ok(info) => Ok(Some(info)),
        Err(err) => Err(err),
    }
}

pub fn platform_tags() -> Vec<String> {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    vec![format!("{os}-{arch}"), os.to_string()]
}

pub fn version_from_tag(tag: &str) -> Result<String, String> {
    let trimmed = tag.trim();
    let stripped = trimmed.trim_start_matches(['v', 'V']);
    if let Ok(version) = semver::Version::parse(stripped) {
        return Ok(version.to_string());
    }
    if let Some(version) = embedded_semver(trimmed) {
        return Ok(version);
    }
    Err(format!(
        "Le tag « {tag} » n'est pas une version, par exemple v1.2.50."
    ))
}

fn embedded_semver(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    for start in 0..chars.len() {
        if !chars[start].is_ascii_digit() {
            continue;
        }
        let rest: String = chars[start..].iter().collect();
        let end = rest
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+'))
            .map(|(index, _)| index)
            .unwrap_or(rest.len());
        let mut candidate = rest[..end].trim_end_matches(['.', '-', '+']).to_string();
        while !candidate.is_empty() {
            if let Ok(version) = semver::Version::parse(&candidate) {
                return Some(version.to_string());
            }
            match candidate.rfind('.') {
                Some(dot) => candidate.truncate(dot),
                None => break,
            }
        }
    }
    None
}

pub fn is_newer(remote: &str, local: &str) -> bool {
    match (
        semver::Version::parse(remote),
        semver::Version::parse(local),
    ) {
        (Ok(remote), Ok(local)) => remote > local,
        _ => false,
    }
}

pub fn french_date(iso: &str) -> String {
    let Some(date) = iso.get(0..10) else {
        return "—".to_string();
    };
    let Some(year) = date.get(0..4) else {
        return date.to_string();
    };
    let Ok(month) = date.get(5..7).unwrap_or("").parse::<usize>() else {
        return date.to_string();
    };
    let Ok(day) = date.get(8..10).unwrap_or("").parse::<u32>() else {
        return date.to_string();
    };
    const MONTHS: [&str; 12] = [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
        "déc.",
    ];
    match MONTHS.get(month.wrapping_sub(1)) {
        Some(label) => format!("{day} {label} {year}"),
        None => date.to_string(),
    }
}

fn select_release(release: &GhRelease, tags: &[String]) -> Result<ReleaseInfo, String> {
    let version = version_from_tag(&release.tag_name)?;
    let package = select_package(&release.assets, tags)?;
    let checksum = checksum_asset(&release.assets, &package.name).cloned();
    Ok(ReleaseInfo {
        version,
        published_label: release
            .published_at
            .as_deref()
            .map(french_date)
            .unwrap_or_else(|| "—".to_string()),
        package: remote_file(package),
        checksum: checksum.as_ref().map(remote_file),
    })
}

fn select_package<'a>(assets: &'a [GhAsset], tags: &[String]) -> Result<&'a GhAsset, String> {
    let zips: Vec<&GhAsset> = assets
        .iter()
        .filter(|asset| asset.name.to_ascii_lowercase().ends_with(".zip"))
        .collect();
    if !zips.is_empty() {
        return select_named(&zips, tags);
    }
    let programs: Vec<&GhAsset> = assets
        .iter()
        .filter(|asset| is_program(&asset.name))
        .collect();
    if programs.is_empty() {
        return Err("Aucun fichier .zip ni programme dans cette version.".to_string());
    }
    select_named(&programs, tags)
}

fn is_program(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".sha256") || name == "checksums.txt" || name == "sha256sums" {
        return false;
    }
    if cfg!(windows) {
        name.ends_with(".exe")
    } else {
        !name.contains('.')
    }
}

fn select_named<'a>(files: &[&'a GhAsset], tags: &[String]) -> Result<&'a GhAsset, String> {
    let mut best: Vec<&GhAsset> = Vec::new();
    let mut best_score = 0usize;
    for asset in files {
        let name = asset.name.to_ascii_lowercase();
        let score = tags
            .iter()
            .enumerate()
            .filter_map(|(index, tag)| {
                name.contains(&tag.to_ascii_lowercase())
                    .then_some(tags.len() - index)
            })
            .max()
            .unwrap_or(0);
        if score > best_score {
            best_score = score;
            best.clear();
            best.push(*asset);
        } else if score == best_score && score > 0 {
            best.push(*asset);
        }
    }
    if best_score > 0 {
        if best.len() == 1 {
            return Ok(best[0]);
        }
        return Err(format!(
            "Plusieurs paquets correspondent : {}.",
            joined_names(&best)
        ));
    }
    if files.len() == 1 {
        return Ok(files[0]);
    }
    let wanted = tags
        .first()
        .map(String::as_str)
        .unwrap_or("cette plateforme");
    Err(format!(
        "Aucun paquet pour {wanted} parmi : {}.",
        joined_names(files)
    ))
}

#[cfg(test)]
pub fn select_zip<'a>(assets: &'a [GhAsset], tags: &[String]) -> Result<&'a GhAsset, String> {
    let zips: Vec<&GhAsset> = assets
        .iter()
        .filter(|asset| asset.name.to_ascii_lowercase().ends_with(".zip"))
        .collect();
    if zips.is_empty() {
        return Err("Aucun fichier .zip dans cette version.".to_string());
    }
    select_named(&zips, tags)
}

pub fn parse_checksum(text: &str, asset_name: &str) -> Option<String> {
    let trimmed = text.trim();
    if is_hex64(trimmed) {
        return Some(trimmed.to_ascii_lowercase());
    }
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(first) = parts.next() else { continue };
        let Some(second) = parts.next() else { continue };
        if !is_hex64(first) {
            continue;
        }
        let name = second.trim_start_matches('*');
        let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        if name.eq_ignore_ascii_case(asset_name) {
            return Some(first.to_ascii_lowercase());
        }
    }
    None
}

pub fn download_url<'a>(file: &'a RemoteFile, token: &str) -> &'a str {
    if token.is_empty() || file.api_url.is_empty() {
        &file.browser_url
    } else {
        &file.api_url
    }
}

pub fn api_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(concat!("dubSuite/", env!("CARGO_PKG_VERSION"))),
    );
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static("2022-11-28"),
    );
    if token.is_empty() {
        headers.remove(AUTHORIZATION);
    }
    headers
}

pub fn auth_header(token: &str) -> Option<HeaderValue> {
    if token.is_empty() {
        return None;
    }
    HeaderValue::from_str(&format!("Bearer {token}")).ok()
}

fn checksum_asset<'a>(assets: &'a [GhAsset], zip_name: &str) -> Option<&'a GhAsset> {
    let sha_name = format!("{zip_name}.sha256");
    assets.iter().find(|asset| {
        let name = asset.name.to_ascii_lowercase();
        name == "checksums.txt" || name == "sha256sums" || name == sha_name.to_ascii_lowercase()
    })
}

fn remote_file(asset: &GhAsset) -> RemoteFile {
    RemoteFile {
        name: asset.name.clone(),
        browser_url: asset.browser_download_url.clone(),
        api_url: asset.url.clone(),
        size: asset.size,
    }
}

fn joined_names(assets: &[&GhAsset]) -> String {
    assets
        .iter()
        .map(|asset| asset.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

fn http_error(status: u16, body: &str) -> String {
    if status == 401 || status == 403 {
        return format!(
            "Accès refusé par GitHub ({status}). Pour un dépôt privé, ajoutez un jeton dans Catalogue…"
        );
    }
    let snippet: String = body
        .chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .take(160)
        .collect();
    if snippet.is_empty() {
        format!("GitHub a répondu {status}.")
    } else {
        format!("GitHub a répondu {status}. {snippet}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> GhAsset {
        GhAsset {
            name: name.to_string(),
            browser_download_url: format!("https://example.test/{name}"),
            url: String::new(),
            size: 10,
        }
    }

    fn tags() -> Vec<String> {
        vec!["windows-x64".to_string(), "windows".to_string()]
    }

    #[test]
    fn prefers_the_platform_zip() {
        let assets = vec![
            asset("dubdsr-1.2.50-linux-x64.zip"),
            asset("dubdsr-1.2.50-windows-x64.zip"),
            asset("checksums.txt"),
        ];
        let chosen = select_zip(&assets, &tags()).unwrap();
        assert_eq!(chosen.name, "dubdsr-1.2.50-windows-x64.zip");
    }

    #[test]
    fn a_single_exe_is_accepted_when_there_is_no_zip() {
        let assets = vec![asset("dubdsr1.2.1.exe")];
        let chosen = select_package(&assets, &tags()).unwrap();
        assert_eq!(chosen.name, "dubdsr1.2.1.exe");
    }

    #[test]
    fn a_single_zip_is_accepted() {
        let assets = vec![asset("dubdsr1.2.50.zip")];
        let chosen = select_zip(&assets, &tags()).unwrap();
        assert_eq!(chosen.name, "dubdsr1.2.50.zip");
    }

    #[test]
    fn version_tag_and_order() {
        assert_eq!(version_from_tag("v1.2.50").unwrap(), "1.2.50");
        assert_eq!(version_from_tag("dubDSRv1.2.1").unwrap(), "1.2.1");
        assert!(is_newer("1.2.51", "1.2.50"));
        assert!(!is_newer("1.2.50", "1.2.50"));
        assert!(!is_newer("0.9.0", "0.10.0"));
    }

    #[test]
    fn french_date_reads_the_release_timestamp() {
        assert_eq!(french_date("2026-10-26T15:04:05Z"), "26 oct. 2026");
    }

    #[test]
    fn checksum_line_matches_the_zip_name() {
        let text = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  dubdsr-1.2.50-windows-x64.zip\n";
        assert!(parse_checksum(text, "dubdsr-1.2.50-windows-x64.zip").is_some());
        assert!(parse_checksum(text, "autre.zip").is_none());
    }
}
