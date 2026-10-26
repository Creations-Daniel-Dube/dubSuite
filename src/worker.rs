use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use eframe::egui;

use crate::catalog::{AppEntry, LoadedCatalog};
use crate::github::ReleaseInfo;
use crate::install::{self, Run};
use crate::state::{self, Installed, Settings};

pub const LAUNCHER_ID: &str = "launcher";

pub enum Msg {
    Catalog {
        wave: u64,
        loaded: LoadedCatalog,
    },
    Releases {
        wave: u64,
        items: Vec<(String, Result<Option<ReleaseInfo>, String>)>,
    },
    LauncherRelease {
        wave: u64,
        result: Result<Option<ReleaseInfo>, String>,
    },
    Progress {
        token: u64,
        fraction: Option<f32>,
        label: String,
    },
    AppFinished {
        token: u64,
        id: String,
        outcome: Run<Installed>,
    },
    LauncherFinished {
        token: u64,
        outcome: Run<PathBuf>,
    },
}

pub fn spawn_refresh(ctx: egui::Context, wave: u64, settings: Settings, tx: Sender<Msg>) {
    let _ = std::thread::Builder::new()
        .name("dubsuite-refresh".into())
        .spawn(move || {
            let loaded = crate::catalog::load(&settings);
            let token = state::effective_token(&settings);
            send(
                &ctx,
                &tx,
                Msg::Catalog {
                    wave,
                    loaded: loaded.clone(),
                },
            );
            let mut items = Vec::with_capacity(loaded.catalog.apps.len());
            for app in &loaded.catalog.apps {
                items.push((
                    app.id.clone(),
                    crate::github::fetch_latest(&app.repo, &token),
                ));
            }
            let repo = loaded.catalog.launcher.repo.trim().to_string();
            send(&ctx, &tx, Msg::Releases { wave, items });
            let result = if repo.is_empty() {
                Ok(None)
            } else {
                crate::github::fetch_latest(&repo, &token)
            };
            send(&ctx, &tx, Msg::LauncherRelease { wave, result });
        });
}

pub fn spawn_install(
    ctx: egui::Context,
    tx: Sender<Msg>,
    token_num: u64,
    app: AppEntry,
    release: ReleaseInfo,
    github_token: String,
    cancel: Arc<AtomicBool>,
) {
    let _ = std::thread::Builder::new()
        .name("dubsuite-install".into())
        .spawn(move || {
            let id = app.id.clone();
            let outcome =
                install::install_app(&app, &release, &github_token, &cancel, |fraction, label| {
                    send(
                        &ctx,
                        &tx,
                        Msg::Progress {
                            token: token_num,
                            fraction,
                            label,
                        },
                    );
                });
            send(
                &ctx,
                &tx,
                Msg::AppFinished {
                    token: token_num,
                    id,
                    outcome,
                },
            );
        });
}

pub fn spawn_launcher_update(
    ctx: egui::Context,
    tx: Sender<Msg>,
    token_num: u64,
    release: ReleaseInfo,
    exe_name: String,
    github_token: String,
    cancel: Arc<AtomicBool>,
) {
    let _ = std::thread::Builder::new()
        .name("dubsuite-self-update".into())
        .spawn(move || {
            let outcome = install::stage_launcher(
                &release,
                &exe_name,
                &github_token,
                &cancel,
                |fraction, label| {
                    send(
                        &ctx,
                        &tx,
                        Msg::Progress {
                            token: token_num,
                            fraction,
                            label,
                        },
                    );
                },
            );
            send(
                &ctx,
                &tx,
                Msg::LauncherFinished {
                    token: token_num,
                    outcome,
                },
            );
        });
}

fn send(ctx: &egui::Context, tx: &Sender<Msg>, msg: Msg) {
    let _ = tx.send(msg);
    ctx.request_repaint();
}
