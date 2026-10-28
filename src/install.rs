use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::catalog::{self, AppEntry};
use crate::github::{self, ReleaseInfo, RemoteFile};
use crate::state::{self, Installed};

const MAX_BYTES: u64 = 1024 * 1024 * 1024;

pub enum Run<T> {
    Done(T),
    Cancelled,
    Failed(String),
}

pub fn install_app(
    app: &AppEntry,
    release: &ReleaseInfo,
    token: &str,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Option<f32>, String),
) -> Run<Installed> {
    if !catalog::valid_id(&app.id) {
        return Run::Failed(format!("Identifiant invalide : {}.", app.id));
    }
    let exe_name = catalog::host_exe_name(app);
    let zip_path =
        state::cache_dir().join(format!("{}-{}.zip", app.id, file_token(&release.version)));
    let stamp = file_token(&release.version);
    let sum_path = state::cache_dir().join(format!("{}-{}.sha256", app.id, stamp));
    let tmp = state::data_dir().join("tmp").join(&app.id);

    progress(Some(0.0), "Téléchargement…".to_string());
    let actual = match download(&release.package, &zip_path, token, cancel, |fraction| {
        progress(fraction, "Téléchargement…".to_string());
    }) {
        Run::Done(hash) => hash,
        other => return map_unused(other),
    };

    if let Some(checksum) = &release.checksum {
        progress(Some(1.0), "Vérification…".to_string());
        let text = match download(checksum, &sum_path, token, cancel, |_| {}) {
            Run::Done(_) => match fs::read_to_string(&sum_path) {
                Ok(text) => text,
                Err(err) => return Run::Failed(state::io_fr("Lecture de la somme", &err)),
            },
            other => return map_unused(other),
        };
        let Some(expected) = github::parse_checksum(&text, &release.package.name) else {
            let _ = fs::remove_file(&zip_path);
            let _ = fs::remove_file(&sum_path);
            return Run::Failed("Somme SHA-256 introuvable pour ce paquet.".to_string());
        };
        if !actual.eq_ignore_ascii_case(&expected) {
            let _ = fs::remove_file(&zip_path);
            let _ = fs::remove_file(&sum_path);
            return Run::Failed(
                "Somme SHA-256 incorrecte. Le paquet n'a pas été installé.".to_string(),
            );
        }
    }

    progress(Some(1.0), "Installation…".to_string());
    if release.package.name.to_ascii_lowercase().ends_with(".zip") {
        if let Err(err) = extract_zip(&zip_path, &tmp) {
            let _ = fs::remove_dir_all(&tmp);
            return Run::Failed(err);
        }
        let relative = match locate_exe(&tmp, &exe_name) {
            Ok(path) => path,
            Err(err) => {
                let _ = fs::remove_dir_all(&tmp);
                return Run::Failed(err);
            }
        };
        let dest = state::app_dir(&app.id);
        if let Err(err) = replace_tree(&tmp, &dest) {
            let _ = fs::remove_dir_all(&tmp);
            return Run::Failed(err);
        }
        let _ = fs::remove_file(&zip_path);
        let _ = fs::remove_file(&sum_path);
        return Run::Done(Installed {
            version: release.version.clone(),
            exe: relative.to_string_lossy().replace('\\', "/"),
        });
    }

    let dest = state::app_dir(&app.id);
    if let Err(err) = fs::create_dir_all(&dest) {
        return Run::Failed(state::io_fr("Dossier de l'application", &err));
    }
    let target = dest.join(&exe_name);
    if let Err(err) = fs::copy(&zip_path, &target) {
        return Run::Failed(state::io_fr("Installation", &err));
    }
    let _ = fs::remove_file(&zip_path);
    let _ = fs::remove_file(&sum_path);
    Run::Done(Installed {
        version: release.version.clone(),
        exe: exe_name,
    })
}

pub fn stage_launcher(
    release: &ReleaseInfo,
    exe_name: &str,
    token: &str,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Option<f32>, String),
) -> Run<PathBuf> {
    let current = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => return Run::Failed(state::io_fr("Programme en cours", &err)),
    };
    let staged = staged_update_path(&current);
    let zip_path =
        state::cache_dir().join(format!("launcher-{}.zip", file_token(&release.version)));
    let tmp = state::data_dir().join("tmp").join("launcher");

    progress(Some(0.0), "Téléchargement du lanceur…".to_string());
    match download(&release.package, &zip_path, token, cancel, |fraction| {
        progress(fraction, "Téléchargement du lanceur…".to_string());
    }) {
        Run::Done(_) => {}
        other => return map_unused(other),
    }
    progress(Some(1.0), "Préparation…".to_string());
    if let Err(err) = extract_zip(&zip_path, &tmp) {
        let _ = fs::remove_dir_all(&tmp);
        return Run::Failed(err);
    }
    let relative = match locate_exe(&tmp, exe_name) {
        Ok(path) => path,
        Err(err) => {
            let _ = fs::remove_dir_all(&tmp);
            return Run::Failed(err);
        }
    };
    let found = tmp.join(&relative);
    if let Err(err) = fs::copy(&found, &staged) {
        let _ = fs::remove_dir_all(&tmp);
        return Run::Failed(state::io_fr("Préparation de la mise à jour", &err));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&staged, fs::Permissions::from_mode(0o755));
    }
    let _ = fs::remove_dir_all(&tmp);
    let _ = fs::remove_file(&zip_path);
    Run::Done(staged)
}

pub fn apply_staged_update(target: &Path) -> Result<(), String> {
    let me =
        std::env::current_exe().map_err(|err| state::io_fr("Programme de mise à jour", &err))?;
    let parked = parked_update_path(target);
    move_running_aside(target, &parked)?;
    if let Err(err) = fs::copy(&me, target) {
        let _ = fs::rename(&parked, target);
        return Err(state::io_fr("Remplacement du lanceur", &err));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(target, fs::Permissions::from_mode(0o755));
    }
    std::process::Command::new(target)
        .spawn()
        .map_err(|err| state::io_fr("Relance du lanceur", &err))?;
    let _ = fs::remove_file(&parked);
    Ok(())
}

fn parked_update_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("dubSuite.exe");
    target.with_file_name(format!("{name}.old"))
}

/// Windows refuse d'écraser un programme encore ouvert (erreur 32),
/// mais accepte de le renommer. L'ancien fichier est mis de côté.
fn move_running_aside(target: &Path, parked: &Path) -> Result<(), String> {
    if !target.exists() {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if parked.exists() {
            match fs::remove_file(parked) {
                Ok(()) => {}
                Err(err) if file_is_locked(&err) && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(300));
                    continue;
                }
                Err(err) => return Err(state::io_fr("Remplacement du lanceur", &err)),
            }
        }
        match fs::rename(target, parked) {
            Ok(()) => return Ok(()),
            Err(err) if file_is_locked(&err) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(300));
            }
            Err(err) => return Err(state::io_fr("Remplacement du lanceur", &err)),
        }
    }
}

fn file_is_locked(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::PermissionDenied || matches!(err.raw_os_error(), Some(32 | 33))
}

pub fn staged_update_path(current_exe: &Path) -> PathBuf {
    let name = current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("dubSuite.exe");
    current_exe.with_file_name(format!("{name}.new"))
}

pub fn cleanup_staged_updater() {
    let Ok(me) = std::env::current_exe() else {
        return;
    };
    let Some(name) = me.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    if name.ends_with(".new") || name.ends_with(".old") {
        return;
    }
    let staged = me.with_file_name(format!("{name}.new"));
    let parked = me.with_file_name(format!("{name}.old"));
    std::thread::spawn(move || {
        for path in [staged, parked] {
            for _ in 0..40 {
                if !path.exists() || fs::remove_file(&path).is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    });
}

pub fn uninstall_app(id: &str) -> Result<(), String> {
    purge_install(&state::data_dir(), id)
}

fn purge_install(root: &Path, id: &str) -> Result<(), String> {
    if !catalog::valid_id(id) {
        return Err(format!("Identifiant invalide : {id}."));
    }
    let dest = root.join("apps").join(id);
    remove_install_dir(&dest)?;
    let backup = dest.with_extension("old");
    if backup.exists() {
        let _ = remove_install_dir(&backup);
    }
    let tmp = root.join("tmp").join(id);
    if tmp.exists() {
        let _ = remove_install_dir(&tmp);
    }
    clear_cached_packages(root, id);
    Ok(())
}

fn remove_install_dir(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|err| state::io_fr("Désinstallation", &err))?;
    } else if path.exists() {
        fs::remove_file(path).map_err(|err| state::io_fr("Désinstallation", &err))?;
    }
    Ok(())
}

fn clear_cached_packages(root: &Path, id: &str) {
    let cache = root.join("cache");
    let Ok(entries) = fs::read_dir(&cache) else {
        return;
    };
    let prefix = format!("{id}-");
    let longer_ids = installed_ids(root)
        .into_iter()
        .filter(|other| other != id && other.len() > id.len())
        .collect::<Vec<_>>();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().into_string().ok() else {
            continue;
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        if !(name.ends_with(".zip") || name.ends_with(".sha256")) {
            continue;
        }
        let kept_for_other = longer_ids
            .iter()
            .any(|other| name.starts_with(&format!("{other}-")));
        if kept_for_other {
            continue;
        }
        let _ = fs::remove_file(entry.path());
    }
}

fn installed_ids(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root.join("apps")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            catalog::valid_id(&name).then_some(name)
        })
        .collect()
}

pub fn launch(id: &str, relative: &str) -> Result<(), String> {
    let path = installed_exe(id, relative)
        .ok_or_else(|| "Le programme installé est introuvable.".to_string())?;
    let mut command = std::process::Command::new(&path);
    if let Some(dir) = path.parent() {
        command.current_dir(dir);
    }
    let mut child = command
        .spawn()
        .map_err(|err| state::io_fr("Lancement", &err))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn installed_exe(id: &str, relative: &str) -> Option<PathBuf> {
    if !catalog::valid_id(id) {
        return None;
    }
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    let full = state::app_dir(id).join(relative_path);
    full.is_file().then_some(full)
}

pub fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        fs::remove_dir_all(dest).map_err(|err| state::io_fr("Dossier temporaire", &err))?;
    }
    fs::create_dir_all(dest).map_err(|err| state::io_fr("Dossier temporaire", &err))?;
    let file = fs::File::open(zip_path).map_err(|err| state::io_fr("Ouverture du paquet", &err))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| format!("Archive illisible : {err}"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("Archive illisible : {err}"))?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let out = dest.join(&name);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|err| state::io_fr("Dossier du paquet", &err))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|err| state::io_fr("Dossier du paquet", &err))?;
        }
        let mut output =
            fs::File::create(&out).map_err(|err| state::io_fr("Fichier du paquet", &err))?;
        std::io::copy(&mut entry, &mut output)
            .map_err(|err| state::io_fr("Fichier du paquet", &err))?;
    }
    Ok(())
}

pub fn locate_exe(root: &Path, exe_name: &str) -> Result<PathBuf, String> {
    if !catalog::valid_file_name(exe_name) {
        return Err(format!("Nom de programme invalide : {exe_name}."));
    }
    if let Some(relative) = find_named(root, root, exe_name, 0) {
        return Ok(relative);
    }
    let exes = collect_exes(root, root, 0);
    if exes.len() == 1 {
        return Ok(exes.into_iter().next().expect("un seul programme"));
    }
    if exes.is_empty() {
        return Err(format!("« {exe_name} » est introuvable dans le paquet."));
    }
    let names = exes
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!("Plusieurs programmes dans le paquet : {names}."))
}

fn download(
    file: &RemoteFile,
    dest: &Path,
    token: &str,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Option<f32>),
) -> Run<String> {
    if file.size > MAX_BYTES {
        return Run::Failed("Le paquet dépasse 1 Gio.".to_string());
    }
    if let Some(parent) = dest.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            return Run::Failed(state::io_fr("Dossier de cache", &err));
        }
    }
    let client = match github::http_client(Duration::from_secs(15 * 60)) {
        Ok(client) => client,
        Err(err) => return Run::Failed(err),
    };
    let url = github::download_url(file, token);
    if url.is_empty() {
        return Run::Failed("Adresse de téléchargement absente.".to_string());
    }
    let mut request = client.get(url);
    if url.contains("://api.github.com/") {
        request = request.header(reqwest::header::ACCEPT, "application/octet-stream");
    }
    if let Some(header) = github::auth_header(token) {
        request = request.header(reqwest::header::AUTHORIZATION, header);
    }
    let mut response = match request.send() {
        Ok(response) => response,
        Err(err) => return Run::Failed(format!("Téléchargement impossible : {err}")),
    };
    if !response.status().is_success() {
        return Run::Failed(format!(
            "Téléchargement refusé : HTTP {}.",
            response.status()
        ));
    }
    let total = response
        .content_length()
        .filter(|size| *size > 0)
        .or(if file.size > 0 { Some(file.size) } else { None });
    let mut output = match fs::File::create(dest) {
        Ok(file) => file,
        Err(err) => return Run::Failed(state::io_fr("Écriture du paquet", &err)),
    };
    let mut hasher = Sha256::new();
    let mut got = 0u64;
    let mut last_paint = Instant::now() - Duration::from_secs(1);
    let mut buf = [0u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(output);
            let _ = fs::remove_file(dest);
            return Run::Cancelled;
        }
        let read = match response.read(&mut buf) {
            Ok(0) => break,
            Ok(count) => count,
            Err(err) => {
                drop(output);
                let _ = fs::remove_file(dest);
                return Run::Failed(format!("Téléchargement interrompu : {err}"));
            }
        };
        if got.saturating_add(read as u64) > MAX_BYTES {
            drop(output);
            let _ = fs::remove_file(dest);
            return Run::Failed("Le paquet dépasse 1 Gio.".to_string());
        }
        if let Err(err) = output.write_all(&buf[..read]) {
            drop(output);
            let _ = fs::remove_file(dest);
            return Run::Failed(state::io_fr("Écriture du paquet", &err));
        }
        hasher.update(&buf[..read]);
        got += read as u64;
        if last_paint.elapsed() >= Duration::from_millis(80) {
            last_paint = Instant::now();
            let fraction = total.map(|total| got as f32 / total as f32);
            on_progress(fraction);
        }
    }
    let fraction = total.map(|total| got as f32 / total as f32);
    on_progress(fraction);
    Run::Done(hex_encode(&hasher.finalize()))
}

fn replace_tree(new_dir: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| state::io_fr("Dossier des applications", &err))?;
    }
    let backup = dest.with_extension("old");
    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(|err| state::io_fr("Ancienne installation", &err))?;
    }
    if dest.exists() {
        fs::rename(dest, &backup).map_err(|err| state::io_fr("Remplacement", &err))?;
    }
    if let Err(err) = fs::rename(new_dir, dest) {
        if backup.exists() && !dest.exists() {
            let _ = fs::rename(&backup, dest);
        }
        return Err(state::io_fr("Installation", &err));
    }
    let _ = fs::remove_dir_all(&backup);
    Ok(())
}

fn find_named(root: &Path, dir: &Path, exe_name: &str, depth: u8) -> Option<PathBuf> {
    if depth > 5 {
        return None;
    }
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name == "__MACOSX" {
            continue;
        }
        if path.is_dir() {
            if let Some(found) = find_named(root, &path, exe_name, depth + 1) {
                return Some(found);
            }
        } else if name.eq_ignore_ascii_case(exe_name) {
            return path.strip_prefix(root).ok().map(Path::to_path_buf);
        }
    }
    None
}

fn collect_exes(root: &Path, dir: &Path, depth: u8) -> Vec<PathBuf> {
    if depth > 5 {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name == "__MACOSX" {
            continue;
        }
        if path.is_dir() {
            found.extend(collect_exes(root, &path, depth + 1));
        } else if name.to_ascii_lowercase().ends_with(".exe") {
            if let Ok(relative) = path.strip_prefix(root) {
                found.push(relative.to_path_buf());
            }
        }
    }
    found
}

fn map_unused<T>(run: Run<String>) -> Run<T> {
    match run {
        Run::Cancelled => Run::Cancelled,
        Run::Failed(err) => Run::Failed(err),
        Run::Done(_) => Run::Failed("Téléchargement inattendu.".to_string()),
    }
}

fn file_token(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharing_violation_counts_as_a_locked_program() {
        let sharing = std::io::Error::from_raw_os_error(32);
        let locked = std::io::Error::from_raw_os_error(33);
        let denied = std::io::Error::from_raw_os_error(5);
        let missing = std::io::Error::from_raw_os_error(2);
        assert!(file_is_locked(&sharing));
        assert!(file_is_locked(&locked));
        assert!(file_is_locked(&denied));
        assert!(!file_is_locked(&missing));
    }

    #[test]
    fn extract_keeps_the_exe_and_skips_parent_paths() {
        let dir = std::env::temp_dir().join(format!("dubsuite-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("app.zip");
        {
            let file = fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("dubdsr.exe", opts).unwrap();
            zip.write_all(b"hello").unwrap();
            zip.start_file("../evil.txt", opts).unwrap();
            zip.write_all(b"no").unwrap();
            zip.finish().unwrap();
        }
        let dest = dir.join("out");
        extract_zip(&zip_path, &dest).unwrap();
        assert_eq!(fs::read(dest.join("dubdsr.exe")).unwrap(), b"hello");
        assert!(!dir.join("evil.txt").exists());
        assert!(!dest.join("evil.txt").exists());
        assert_eq!(
            locate_exe(&dest, "dubdsr.exe").unwrap(),
            PathBuf::from("dubdsr.exe")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn purge_install_removes_one_app_and_its_cache() {
        let dir = std::env::temp_dir().join(format!(
            "dubsuite-uninstall-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        let app = dir.join("apps").join("dubdsr");
        let other = dir.join("apps").join("dubfocus");
        let longer = dir.join("apps").join("dubdsr-extra");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::create_dir_all(&longer).unwrap();
        fs::write(app.join("dubdsr.exe"), b"a").unwrap();
        fs::write(other.join("dubFocus.exe"), b"b").unwrap();
        fs::create_dir_all(app.with_extension("old")).unwrap();
        fs::create_dir_all(dir.join("tmp").join("dubdsr")).unwrap();
        fs::write(dir.join("tmp").join("dubdsr").join("part"), b"t").unwrap();
        let cache = dir.join("cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("dubdsr-1.2.0.zip"), b"zip").unwrap();
        fs::write(cache.join("dubdsr-1.2.0.sha256"), b"sum").unwrap();
        fs::write(cache.join("dubfocus-1.0.0.zip"), b"keep").unwrap();
        fs::write(cache.join("dubdsr-extra-1.0.0.zip"), b"keep-too").unwrap();

        purge_install(&dir, "dubdsr").unwrap();

        assert!(!app.exists());
        assert!(!app.with_extension("old").exists());
        assert!(!dir.join("tmp").join("dubdsr").exists());
        assert!(!cache.join("dubdsr-1.2.0.zip").exists());
        assert!(!cache.join("dubdsr-1.2.0.sha256").exists());
        assert_eq!(fs::read(other.join("dubFocus.exe")).unwrap(), b"b");
        assert_eq!(fs::read(cache.join("dubfocus-1.0.0.zip")).unwrap(), b"keep");
        assert_eq!(
            fs::read(cache.join("dubdsr-extra-1.0.0.zip")).unwrap(),
            b"keep-too"
        );
        assert!(purge_install(&dir, "../dubdsr").is_err());
        assert!(other.join("dubFocus.exe").is_file());
        let _ = fs::remove_dir_all(&dir);
    }
}
