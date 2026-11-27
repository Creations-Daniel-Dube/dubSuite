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
    let path = data_dir().join("settings.json");
    let exists = path.is_file();
    let settings: Settings = read_json(&path).unwrap_or_default();
    // #region agent log
    agent_debug_log(
        "H1",
        "state.rs:load_settings",
        "loaded settings",
        serde_json::json!({
            "settingsPath": path.display().to_string(),
            "settingsFileExists": exists,
            "catalogUrlLen": settings.catalog_url.trim().len(),
            "tokenLen": settings.github_token.trim().len(),
        }),
    );
    // #endregion
    settings
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
    let from_env = std::env::var("DUBSUITE_GITHUB_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let token = from_env
        .clone()
        .unwrap_or_else(|| settings.github_token.trim().to_string());
    // #region agent log
    agent_debug_log(
        "H1",
        "state.rs:effective_token",
        "resolved github token source",
        serde_json::json!({
            "fromEnv": from_env.is_some(),
            "tokenLen": token.len(),
            "settingsTokenLen": settings.github_token.trim().len(),
        }),
    );
    // #endregion
    token
}

// #region agent log
pub(crate) fn agent_debug_log(
    hypothesis_id: &str,
    location: &str,
    message: &str,
    data: serde_json::Value,
) {
    use std::io::Write;
    #[derive(Serialize)]
    struct Entry<'a> {
        sessionId: &'static str,
        hypothesisId: &'a str,
        location: &'a str,
        message: &'a str,
        data: serde_json::Value,
        timestamp: u128,
        runId: &'static str,
    }
    let Ok(line) = serde_json::to_string(&Entry {
        sessionId: "da2fc9",
        hypothesisId: hypothesis_id,
        location,
        message,
        data,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        runId: "pre-fix",
    }) else {
        return;
    };
    let log_path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/.cursor/debug-da2fc9.log"
    ));
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = writeln!(file, "{line}");
    }
    let fallback = data_dir().join("debug-da2fc9.log");
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&fallback)
    {
        let _ = writeln!(file, "{line}");
    }
}
// #endregion

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
