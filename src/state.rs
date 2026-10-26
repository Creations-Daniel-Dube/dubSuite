use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub catalog_url: String,
    #[serde(default)]
    pub github_token: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            catalog_url: String::new(),
            github_token: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    pub exe: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SuiteState {
    #[serde(default)]
    pub apps: BTreeMap<String, Installed>,
}

pub fn data_dir() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        if !local.trim().is_empty() {
            return PathBuf::from(local).join("DubSuite");
        }
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg).join("dubsuite");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("dubsuite");
        }
    }
    PathBuf::from("DubSuite")
}

pub fn app_dir(id: &str) -> PathBuf {
    data_dir().join("apps").join(id)
}

pub fn cache_dir() -> PathBuf {
    data_dir().join("cache")
}

pub fn load_settings() -> Settings {
    read_json(&data_dir().join("settings.json")).unwrap_or_default()
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    write_json(&data_dir().join("settings.json"), settings)
}

pub fn load_state() -> SuiteState {
    let path = data_dir().join("state.json");
    match read_json(&path) {
        Some(state) => state,
        None if path.exists() => {
            let _ = fs::rename(&path, path.with_extension("json.bak"));
            SuiteState::default()
        }
        None => SuiteState::default(),
    }
}

pub fn save_state(state: &SuiteState) -> Result<(), String> {
    write_json(&data_dir().join("state.json"), state)
}

pub fn catalog_cache_path() -> PathBuf {
    data_dir().join("catalog.cache.json")
}

pub fn write_update_error(message: &str) -> Result<(), String> {
    let path = data_dir().join("update-error.txt");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| io_fr("Dossier du lanceur", &err))?;
    }
    fs::write(&path, message).map_err(|err| io_fr("Écriture de l'erreur de mise à jour", &err))
}

pub fn take_update_error() -> Option<String> {
    let path = data_dir().join("update-error.txt");
    let text = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    let text = text.trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

pub fn effective_token(settings: &Settings) -> String {
    std::env::var("DUBSUITE_GITHUB_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| settings.github_token.trim().to_string())
}

pub fn check_catalog_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Ok(());
    }
    if url.starts_with("https://") && url.len() < 500 && !url.contains(char::is_whitespace) {
        return Ok(());
    }
    Err("L'adresse du catalogue doit commencer par https://".to_string())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| io_fr("Dossier du lanceur", &err))?;
    }
    let data = serde_json::to_vec_pretty(value)
        .map_err(|err| format!("Enregistrement impossible : {err}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data).map_err(|err| io_fr("Enregistrement", &err))?;
    fs::rename(&tmp, path).map_err(|err| io_fr("Enregistrement", &err))
}

pub fn io_fr(context: &str, err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied => {
            format!("{context} : accès refusé. Fermez l'application si elle est encore ouverte.")
        }
        std::io::ErrorKind::NotFound => format!("{context} : fichier introuvable."),
        _ => format!("{context} : {err}"),
    }
}
