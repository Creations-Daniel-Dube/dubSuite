use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::state::{self, Settings};

const EMBEDDED: &str = include_str!("../catalog.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub catalog_url: String,
    #[serde(default)]
    pub launcher: LauncherMeta,
    pub apps: Vec<AppEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherMeta {
    #[serde(default)]
    pub repo: String,
    #[serde(default = "default_launcher_exe")]
    pub exe: String,
}

impl Default for LauncherMeta {
    fn default() -> Self {
        Self {
            repo: String::new(),
            exe: default_launcher_exe(),
        }
    }
}

fn default_launcher_exe() -> String {
    "dubSuite.exe".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub summary: String,
    pub repo: String,
    pub exe: String,
    #[serde(default)]
    pub exe_linux: String,
    #[serde(default)]
    pub accent: String,
}

#[derive(Debug, Clone)]
pub struct LoadedCatalog {
    pub catalog: Catalog,
    pub source: String,
}

pub fn load_local() -> LoadedCatalog {
    let mut candidates = vec![PathBuf::from("catalog.json")];
    if let Some(dir) = exe_dir() {
        candidates.push(dir.join("catalog.json"));
        if exe_is_in_target(&dir) {
            candidates.push(dir.join("..").join("..").join("catalog.json"));
        }
    }
    let labels = [
        "fichier du répertoire courant",
        "fichier à côté du programme",
        "fichier du projet",
    ];
    for (path, label) in candidates.iter().zip(labels) {
        if !path.is_file() {
            continue;
        }
        return match fs::read_to_string(path) {
            Ok(text) => match parse_catalog(&text) {
                Ok(catalog) => LoadedCatalog {
                    catalog,
                    source: label.to_string(),
                },
                Err(err) => embedded_with_source(err),
            },
            Err(err) => embedded_with_source(state::io_fr(label, &err)),
        };
    }
    embedded_with_source("inclus dans le programme")
}

/// Le catalogue en ligne, quand une adresse est connue, remplace le fichier local.
/// Sans adresse, le fichier du projet ou celui inclus dans le programme est utilisé.
pub fn load(settings: &Settings) -> LoadedCatalog {
    let local = load_local();
    let url = catalog_url(settings, &local.catalog);
    if url.is_empty() {
        return local;
    }
    match fetch_remote(&url) {
        Ok(text) => match parse_catalog(&text) {
            Ok(catalog) => {
                let cache = state::catalog_cache_path();
                if let Some(parent) = cache.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(cache, &text);
                LoadedCatalog {
                    catalog,
                    source: "catalogue en ligne".to_string(),
                }
            }
            Err(err) => fallback_after_remote_failure(&local, &err),
        },
        Err(err) => fallback_after_remote_failure(&local, &err),
    }
}

pub fn catalog_url(settings: &Settings, local: &Catalog) -> String {
    if let Ok(url) = std::env::var("DUBSUITE_CATALOG_URL") {
        let url = url.trim().to_string();
        if !url.is_empty() {
            return url;
        }
    }
    if !settings.catalog_url.trim().is_empty() {
        return settings.catalog_url.trim().to_string();
    }
    local.catalog_url.trim().to_string()
}

pub fn parse_catalog(text: &str) -> Result<Catalog, String> {
    let text = text.trim_start_matches('\u{feff}');
    let catalog: Catalog =
        serde_json::from_str(text).map_err(|err| format!("Catalogue illisible : {err}"))?;
    validate(&catalog)?;
    Ok(catalog)
}

pub fn host_exe_name(app: &AppEntry) -> String {
    #[cfg(windows)]
    {
        app.exe.clone()
    }
    #[cfg(not(windows))]
    {
        if !app.exe_linux.trim().is_empty() {
            return app.exe_linux.trim().to_string();
        }
        app.exe.strip_suffix(".exe").unwrap_or(&app.exe).to_string()
    }
}

pub fn host_launcher_exe(launcher: &LauncherMeta) -> String {
    #[cfg(windows)]
    {
        launcher.exe.clone()
    }
    #[cfg(not(windows))]
    {
        launcher
            .exe
            .strip_suffix(".exe")
            .unwrap_or(&launcher.exe)
            .to_string()
    }
}

pub fn mark_of(app: &AppEntry) -> String {
    let explicit = app.mark.trim();
    if !explicit.is_empty() {
        return explicit.chars().take(3).collect();
    }
    app.name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

fn fallback_after_remote_failure(local: &LoadedCatalog, err: &str) -> LoadedCatalog {
    if let Some(text) = fs::read_to_string(state::catalog_cache_path()).ok() {
        if let Ok(catalog) = parse_catalog(&text) {
            return LoadedCatalog {
                catalog,
                source: format!("copie locale du catalogue en ligne ({err})"),
            };
        }
    }
    LoadedCatalog {
        catalog: local.catalog.clone(),
        source: format!("{} ({err})", local.source),
    }
}

fn fetch_remote(url: &str) -> Result<String, String> {
    state::check_catalog_url(url)?;
    let client = crate::github::http_client(std::time::Duration::from_secs(20))?;
    let response = client
        .get(url)
        .send()
        .map_err(|err| format!("Catalogue en ligne injoignable : {err}"))?;
    if !response.status().is_success() {
        return Err(format!("Catalogue en ligne : HTTP {}", response.status()));
    }
    response
        .text()
        .map_err(|err| format!("Catalogue en ligne illisible : {err}"))
}

fn embedded_with_source(source: impl Into<String>) -> LoadedCatalog {
    LoadedCatalog {
        catalog: parse_catalog(EMBEDDED).expect("le catalogue inclus est valide"),
        source: source.into(),
    }
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn exe_is_in_target(dir: &Path) -> bool {
    dir.components()
        .any(|component| component.as_os_str() == "target")
}

fn validate(catalog: &Catalog) -> Result<(), String> {
    if catalog.apps.len() > 100 {
        return Err("Le catalogue dépasse 100 applications.".to_string());
    }
    let mut seen = std::collections::BTreeSet::new();
    for app in &catalog.apps {
        if !valid_id(&app.id) {
            return Err(format!("Identifiant d'application invalide : {}", app.id));
        }
        if app.id == "launcher" {
            return Err("L'identifiant « launcher » est réservé.".to_string());
        }
        if !seen.insert(app.id.clone()) {
            return Err(format!("Identifiant en double : {}", app.id));
        }
        if app.name.trim().is_empty() {
            return Err(format!("L'application {} n'a pas de nom.", app.id));
        }
        if !valid_repo(&app.repo) {
            return Err(format!(
                "Dépôt GitHub invalide pour {} : {}",
                app.name, app.repo
            ));
        }
        if !valid_file_name(&app.exe) {
            return Err(format!(
                "Nom de programme invalide pour {} : {}",
                app.name, app.exe
            ));
        }
        if !app.exe_linux.is_empty() && !valid_file_name(&app.exe_linux) {
            return Err(format!("Nom Linux invalide pour {}.", app.name));
        }
    }
    if !catalog.launcher.repo.is_empty() && !valid_repo(&catalog.launcher.repo) {
        return Err(format!(
            "Dépôt du lanceur invalide : {}",
            catalog.launcher.repo
        ));
    }
    if !valid_file_name(&catalog.launcher.exe) {
        return Err("Nom du programme lanceur invalide.".to_string());
    }
    Ok(())
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(name), None) => {
            valid_github_component(owner) && valid_github_component(name)
        }
        _ => false,
    }
}

fn valid_github_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

pub fn valid_file_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_lists_the_two_apps() {
        let catalog = parse_catalog(EMBEDDED).unwrap();
        let ids: Vec<_> = catalog.apps.iter().map(|app| app.id.as_str()).collect();
        assert_eq!(ids, ["dubdsr", "dubfocus"]);
        assert!(valid_repo(&catalog.apps[0].repo));
        assert!(valid_repo(&catalog.apps[1].repo));
    }

    #[test]
    fn rejects_a_path_as_an_id() {
        let text = r#"{
            "apps": [{
                "id": "../dub",
                "name": "X",
                "repo": "DarthDub66/dubFocus",
                "exe": "dubFocus.exe"
            }]
        }"#;
        assert!(parse_catalog(text).is_err());
    }
}
