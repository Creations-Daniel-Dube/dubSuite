use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::state::{self, Settings};

const EMBEDDED: &str = include_str!("../catalog.json");

pub const GENERAL_CATEGORY_ID: &str = "general";
pub const GENERAL_CATEGORY_TITLE: &str = "Applications générales";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub catalog_url: String,
    #[serde(default)]
    pub launcher: LauncherMeta,
    #[serde(default)]
    pub categories: Vec<Category>,
    pub apps: Vec<AppEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub title: String,
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
    #[serde(default)]
    pub category: String,
}

#[derive(Debug, Clone)]
pub struct AppGroup<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub apps: Vec<&'a AppEntry>,
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

/// Regroupe les applications dans l'ordre des catégories.
/// Une catégorie sans application est omise.
/// Sans catégorie, ou avec un identifiant inconnu, l'application rejoint
/// « Applications générales » si cette catégorie existe, sinon une section de repli.
pub fn group_apps<'a>(categories: &'a [Category], apps: &'a [AppEntry]) -> Vec<AppGroup<'a>> {
    let mut buckets: Vec<Vec<&'a AppEntry>> = vec![Vec::new(); categories.len()];
    let mut fallback = Vec::new();
    let general = categories
        .iter()
        .position(|category| category.id == GENERAL_CATEGORY_ID);

    for app in apps {
        let id = app.category.trim();
        if let Some(index) = categories.iter().position(|category| category.id == id) {
            buckets[index].push(app);
        } else if let Some(index) = general {
            buckets[index].push(app);
        } else {
            fallback.push(app);
        }
    }

    let mut groups = Vec::new();
    for (category, apps) in categories.iter().zip(buckets) {
        if apps.is_empty() {
            continue;
        }
        groups.push(AppGroup {
            id: &category.id,
            title: &category.title,
            apps,
        });
    }
    if !fallback.is_empty() {
        groups.push(AppGroup {
            id: GENERAL_CATEGORY_ID,
            title: GENERAL_CATEGORY_TITLE,
            apps: fallback,
        });
    }
    groups
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
    let mut seen_categories = std::collections::BTreeSet::new();
    for category in &catalog.categories {
        if !valid_id(&category.id) {
            return Err(format!(
                "Identifiant de catégorie invalide : {}",
                category.id
            ));
        }
        if !seen_categories.insert(category.id.clone()) {
            return Err(format!(
                "Identifiant de catégorie en double : {}",
                category.id
            ));
        }
        if category.title.trim().is_empty() {
            return Err(format!("La catégorie {} n'a pas de titre.", category.id));
        }
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
        assert!(catalog.apps.iter().all(|app| app.category == "cuda"));
        let groups = group_apps(&catalog.categories, &catalog.apps);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, "cuda");
        assert_eq!(groups[0].title, "Carte vidéo Nvidia (CUDA) requise");
        let grouped: Vec<_> = groups[0].apps.iter().map(|app| app.id.as_str()).collect();
        assert_eq!(grouped, ["dubdsr", "dubfocus"]);
    }

    #[test]
    fn missing_or_unknown_category_joins_general() {
        let text = r#"{
            "categories": [
                { "id": "cuda", "title": "CUDA" },
                { "id": "general", "title": "Applications générales" }
            ],
            "apps": [
                {
                    "id": "a",
                    "name": "A",
                    "repo": "DarthDub66/dubFocus",
                    "exe": "a.exe",
                    "category": "cuda"
                },
                {
                    "id": "b",
                    "name": "B",
                    "repo": "DarthDub66/dubFocus",
                    "exe": "b.exe"
                },
                {
                    "id": "c",
                    "name": "C",
                    "repo": "DarthDub66/dubFocus",
                    "exe": "c.exe",
                    "category": "inconnu"
                }
            ]
        }"#;
        let catalog = parse_catalog(text).unwrap();
        let groups = group_apps(&catalog.categories, &catalog.apps);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].id, "cuda");
        assert_eq!(groups[0].apps[0].id, "a");
        assert_eq!(groups[1].id, "general");
        assert_eq!(groups[1].title, "Applications générales");
        let ids: Vec<_> = groups[1].apps.iter().map(|app| app.id.as_str()).collect();
        assert_eq!(ids, ["b", "c"]);
    }

    #[test]
    fn unknown_category_uses_a_fallback_section() {
        let text = r#"{
            "categories": [{ "id": "cuda", "title": "CUDA" }],
            "apps": [{
                "id": "a",
                "name": "A",
                "repo": "DarthDub66/dubFocus",
                "exe": "a.exe",
                "category": "autre"
            }]
        }"#;
        let catalog = parse_catalog(text).unwrap();
        let groups = group_apps(&catalog.categories, &catalog.apps);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, GENERAL_CATEGORY_ID);
        assert_eq!(groups[0].title, GENERAL_CATEGORY_TITLE);
        assert_eq!(groups[0].apps[0].id, "a");
    }

    #[test]
    fn rejects_a_duplicate_category() {
        let text = r#"{
            "categories": [
                { "id": "cuda", "title": "CUDA" },
                { "id": "cuda", "title": "Encore CUDA" }
            ],
            "apps": []
        }"#;
        assert!(parse_catalog(text).is_err());
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
