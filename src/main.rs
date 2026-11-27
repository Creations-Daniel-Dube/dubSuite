#![cfg_attr(windows, windows_subsystem = "windows")]

mod catalog;
mod github;
mod icon;
mod install;
mod state;
mod ui;
mod worker;

fn main() -> eframe::Result {
    // #region agent log
    std::panic::set_hook(Box::new(|info| {
        state::agent_debug_log(
            "H6",
            "main.rs:panic",
            "process panicked",
            serde_json::json!({ "info": info.to_string() }),
        );
    }));
    // #endregion
    if let Some(target) = update_target() {
        let code = match install::apply_staged_update(&target) {
            Ok(()) => 0,
            Err(err) => {
                let _ = state::write_update_error(&err);
                1
            }
        };
        std::process::exit(code);
    }
    install::cleanup_staged_updater();
    ui::run()
}

/// Le remplaçant est relancé avec `--apply-update <exe actuel>`.
fn update_target() -> Option<std::path::PathBuf> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--apply-update") {
        return args.next().map(std::path::PathBuf::from);
    }
    None
}
