use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use eframe::egui::{self, Align, Color32, Frame, Layout, RichText, ScrollArea, Stroke, Vec2};

use crate::catalog::{self, AppEntry, Catalog};
use crate::github::{self, ReleaseInfo};
use crate::install::{self, Run};
use crate::state::{self, Installed, Settings, SuiteState};
use crate::worker::{self, LAUNCHER_ID, Msg};

const BG: Color32 = Color32::from_rgb(0x12, 0x12, 0x14);
const CARD: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x20);
const CARD_LINE: Color32 = Color32::from_rgb(0x32, 0x32, 0x38);
const TEXT: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf4);
const MUTED: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xaa);
const ACCENT: Color32 = Color32::from_rgb(0xe6, 0xa8, 0x17);
const DANGER: Color32 = Color32::from_rgb(0xe0, 0x6a, 0x5a);

#[derive(Clone)]
enum Query {
    Checking,
    Missing,
    Ready(ReleaseInfo),
    Failed(String),
}

struct Job {
    id: String,
    label: String,
    fraction: Option<f32>,
    cancel: Arc<AtomicBool>,
}

struct Notice {
    error: bool,
    text: String,
}

enum CardAction {
    Install,
    Open,
    Update,
    Cancel,
}

pub struct SuiteApp {
    settings: Settings,
    state: SuiteState,
    catalog: Catalog,
    source: String,
    releases: std::collections::HashMap<String, Query>,
    launcher_release: Query,
    search: String,
    job: Option<Job>,
    wave: u64,
    job_token: u64,
    checking: bool,
    notice: Option<Notice>,
    show_settings: bool,
    draft_url: String,
    draft_token: String,
    clear_token: bool,
    settings_error: Option<String>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([760.0, 520.0])
            .with_title("dubSuite")
            .with_icon(crate::icon::app_icon()),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "dubSuite",
        options,
        Box::new(|cc| Ok(Box::new(SuiteApp::new(cc)))),
    )
}

impl SuiteApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(theme());
        install_font(&cc.egui_ctx);
        let settings = state::load_settings();
        let mut suite_state = state::load_state();
        if prune_missing(&mut suite_state) {
            let _ = state::save_state(&suite_state);
        }
        let loaded = catalog::load_local();
        let mut releases = std::collections::HashMap::new();
        for app in &loaded.catalog.apps {
            releases.insert(app.id.clone(), Query::Checking);
        }
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            settings: settings.clone(),
            state: suite_state,
            catalog: loaded.catalog,
            source: loaded.source,
            releases,
            launcher_release: Query::Checking,
            search: String::new(),
            job: None,
            wave: 1,
            job_token: 0,
            checking: true,
            notice: None,
            show_settings: false,
            draft_url: settings.catalog_url.clone(),
            draft_token: String::new(),
            clear_token: false,
            settings_error: None,
            tx: tx.clone(),
            rx,
        };
        if let Some(text) = state::take_update_error() {
            app.notice = Some(Notice { error: true, text });
        }
        worker::spawn_refresh(cc.egui_ctx.clone(), app.wave, settings, tx);
        app
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.on_msg(msg);
        }
    }

    fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Catalog { wave, loaded } => {
                if wave != self.wave {
                    return;
                }
                self.catalog = loaded.catalog;
                self.source = loaded.source;
                self.releases
                    .retain(|id, _| self.catalog.apps.iter().any(|app| &app.id == id));
                for app in &self.catalog.apps {
                    self.releases
                        .entry(app.id.clone())
                        .or_insert(Query::Checking);
                }
            }
            Msg::Releases { wave, items } => {
                if wave != self.wave {
                    return;
                }
                for (id, result) in items {
                    self.releases.insert(id, query_from(result));
                }
            }
            Msg::LauncherRelease { wave, result } => {
                if wave != self.wave {
                    return;
                }
                self.launcher_release = query_from(result);
                self.checking = false;
            }
            Msg::Progress {
                token,
                fraction,
                label,
            } => {
                if token == self.job_token {
                    if let Some(job) = &mut self.job {
                        job.fraction = fraction;
                        job.label = label;
                    }
                }
            }
            Msg::AppFinished { token, id, outcome } => {
                if token != self.job_token {
                    return;
                }
                self.job = None;
                match outcome {
                    Run::Done(installed) => {
                        let name = self
                            .catalog
                            .apps
                            .iter()
                            .find(|app| app.id == id)
                            .map(|app| app.name.clone())
                            .unwrap_or_else(|| id.clone());
                        let version = installed.version.clone();
                        self.state.apps.insert(id, installed);
                        let _ = state::save_state(&self.state);
                        self.notice = Some(Notice {
                            error: false,
                            text: format!("{name} {version} est installé."),
                        });
                    }
                    Run::Cancelled => {}
                    Run::Failed(text) => self.notice = Some(Notice { error: true, text }),
                }
            }
            Msg::LauncherFinished { token, outcome } => {
                if token != self.job_token {
                    return;
                }
                self.job = None;
                match outcome {
                    Run::Done(staged) => {
                        if let Err(text) = swap_in(&staged) {
                            self.notice = Some(Notice { error: true, text });
                        }
                    }
                    Run::Cancelled => {}
                    Run::Failed(text) => self.notice = Some(Notice { error: true, text }),
                }
            }
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.wave = self.wave.wrapping_add(1);
        self.checking = true;
        self.launcher_release = Query::Checking;
        for app in &self.catalog.apps {
            self.releases.insert(app.id.clone(), Query::Checking);
        }
        worker::spawn_refresh(
            ctx.clone(),
            self.wave,
            self.settings.clone(),
            self.tx.clone(),
        );
    }

    fn begin_job(&mut self, id: &str) -> Arc<AtomicBool> {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.job_token = self.job_token.wrapping_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        self.job = Some(Job {
            id: id.to_string(),
            label: "Préparation…".to_string(),
            fraction: None,
            cancel: cancel.clone(),
        });
        cancel
    }

    fn perform(&mut self, ctx: &egui::Context, app: &AppEntry, action: CardAction) {
        match action {
            CardAction::Cancel => {
                if let Some(job) = &self.job {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            }
            CardAction::Open => self.open_app(app),
            CardAction::Install | CardAction::Update => self.start_install(ctx, app),
        }
    }

    fn open_app(&mut self, app: &AppEntry) {
        let Some(installed) = self.state.apps.get(&app.id).cloned() else {
            return;
        };
        if install::installed_exe(&app.id, &installed.exe).is_none() {
            self.state.apps.remove(&app.id);
            let _ = state::save_state(&self.state);
            self.notice = Some(Notice {
                error: true,
                text: format!("{} est introuvable. Installez-le de nouveau.", app.name),
            });
            return;
        }
        if let Err(text) = install::launch(&app.id, &installed.exe) {
            self.notice = Some(Notice { error: true, text });
        }
    }

    fn start_install(&mut self, ctx: &egui::Context, app: &AppEntry) {
        let Some(Query::Ready(release)) = self.releases.get(&app.id).cloned() else {
            return;
        };
        let cancel = self.begin_job(&app.id);
        worker::spawn_install(
            ctx.clone(),
            self.tx.clone(),
            self.job_token,
            app.clone(),
            release,
            state::effective_token(&self.settings),
            cancel,
        );
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("dubSuite").size(22.0).color(ACCENT).strong());
            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).color(MUTED));
            let width = ui.available_width();
            ui.allocate_ui_with_layout(egui::vec2(width, 24.0), Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Rechercher une application")
                        .desired_width(260.0),
                );
            });
        });
        ui.add_space(10.0);
    }

    fn banner(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if let Some(notice) = &self.notice {
            let color = if notice.error { DANGER } else { MUTED };
            ui.label(RichText::new(&notice.text).color(color));
            ui.add_space(8.0);
        }
        let ready = match &self.launcher_release {
            Query::Ready(info) if github::is_newer(&info.version, env!("CARGO_PKG_VERSION")) => {
                Some(info.clone())
            }
            _ => None,
        };
        let Some(info) = ready else {
            return;
        };
        let version = info.version.clone();
        let running = self.job.as_ref().is_some_and(|job| job.id == LAUNCHER_ID);
        let label = self
            .job
            .as_ref()
            .filter(|job| job.id == LAUNCHER_ID)
            .map(|job| job.label.clone());
        let fraction = self
            .job
            .as_ref()
            .filter(|job| job.id == LAUNCHER_ID)
            .and_then(|job| job.fraction);
        let mut update = false;
        let mut cancel = false;
        Frame::NONE
            .fill(Color32::from_rgb(0x2a, 0x22, 0x10))
            .stroke(Stroke::new(1.0, ACCENT))
            .corner_radius(8.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!(
                        "Une mise à jour du lanceur est disponible : {version}."
                    )));
                    if running {
                        if let Some(label) = &label {
                            show_progress(ui, fraction, label);
                        }
                        if ui.button("Annuler").clicked() {
                            cancel = true;
                        }
                    } else if ui
                        .add_enabled(self.job.is_none(), egui::Button::new("Mettre à jour"))
                        .clicked()
                    {
                        update = true;
                    }
                });
            });
        ui.add_space(10.0);
        if cancel {
            if let Some(job) = &self.job {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
        if update {
            let exe = catalog::host_launcher_exe(&self.catalog.launcher);
            let cancel = self.begin_job(LAUNCHER_ID);
            worker::spawn_launcher_update(
                ctx.clone(),
                self.tx.clone(),
                self.job_token,
                info,
                exe,
                state::effective_token(&self.settings),
                cancel,
            );
        }
    }

    fn cards(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let apps: Vec<AppEntry> = self
            .catalog
            .apps
            .iter()
            .filter(|app| matches_query(app, &self.search))
            .cloned()
            .collect();
        if apps.is_empty() {
            ui.label(RichText::new("Aucune application ne correspond.").color(MUTED));
            return;
        }
        let gap = 12.0;
        let cols = ((ui.available_width() + gap) / (300.0 + gap))
            .floor()
            .max(1.0) as usize;
        let mut pending = Vec::new();
        egui::Grid::new("apps")
            .num_columns(cols)
            .spacing([gap, gap])
            .show(ui, |ui| {
                for (index, app) in apps.iter().enumerate() {
                    if let Some(action) = self.card(ui, app) {
                        pending.push((app.clone(), action));
                    }
                    if (index + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });
        for (app, action) in pending {
            self.perform(ctx, &app, action);
        }
    }

    fn card(&mut self, ui: &mut egui::Ui, app: &AppEntry) -> Option<CardAction> {
        let query = self
            .releases
            .get(&app.id)
            .cloned()
            .unwrap_or(Query::Checking);
        let installed = self.state.apps.get(&app.id).cloned();
        let own_job = self
            .job
            .as_ref()
            .filter(|job| job.id == app.id)
            .map(|job| (job.label.clone(), job.fraction));
        let busy = self.job.is_some();
        let accent = parse_accent(&app.accent).unwrap_or(ACCENT);
        let mark = catalog::mark_of(app);
        let mut action = None;

        Frame::NONE
            .fill(CARD)
            .stroke(Stroke::new(1.0, CARD_LINE))
            .corner_radius(8.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(288.0);
                    ui.set_min_height(146.0);
                    ui.horizontal(|ui| {
                        mark_tile(ui, accent, &mark);
                        ui.label(RichText::new(&app.name).size(16.0).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(badge(&query)).color(ACCENT).strong());
                        });
                    });
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(RichText::new(&app.summary).color(MUTED).size(13.0)).wrap());
                    ui.add_space(6.0);
                    let (line, color) = status_line(&query);
                    ui.add(egui::Label::new(RichText::new(line).color(color).size(12.0)).wrap());
                    if let Some(installed) = &installed {
                        ui.label(RichText::new(format!("Installée : {}", installed.version)).color(MUTED).size(12.0));
                        if let Query::Ready(info) = &query {
                            if github::is_newer(&installed.version, &info.version) {
                                ui.label(RichText::new("La version installée est plus récente.").color(MUTED).size(12.0));
                            }
                        }
                    }
                    ui.add_space(8.0);
                    action = card_buttons(ui, &query, installed.as_ref(), own_job.as_ref(), busy);
                });
            });
        action
    }

    fn footer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let count = self.catalog.apps.len();
            let word = if count > 1 {
                "applications"
            } else {
                "application"
            };
            let status = if self.checking {
                format!("Vérification des versions… · {count} {word}")
            } else {
                format!("{} · {count} {word}", self.source)
            };
            ui.label(RichText::new(status).color(MUTED).size(12.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Catalogue…").clicked() {
                    self.draft_url = self.settings.catalog_url.clone();
                    self.draft_token.clear();
                    self.clear_token = false;
                    self.settings_error = None;
                    self.show_settings = true;
                }
                if ui
                    .add_enabled(self.job.is_none(), egui::Button::new("Actualiser"))
                    .clicked()
                {
                    self.refresh(ctx);
                }
            });
        });
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new("Catalogue")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label("Adresse du catalogue en ligne");
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft_url)
                        .hint_text("https://raw.githubusercontent.com/…/catalog.json")
                        .desired_width(420.0),
                );
                ui.add_space(8.0);
                ui.label("Jeton GitHub pour les dépôts privés");
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft_token)
                        .password(true)
                        .hint_text("laisser vide pour conserver le jeton")
                        .desired_width(420.0),
                );
                ui.checkbox(&mut self.clear_token, "Effacer le jeton enregistré");
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Le jeton est enregistré dans settings.json, sur cet ordinateur.",
                    )
                    .color(MUTED)
                    .size(12.0),
                );
                if let Some(err) = &self.settings_error {
                    ui.label(RichText::new(err).color(DANGER));
                }
                ui.add_space(8.0);
                if ui.button("Enregistrer").clicked() {
                    self.save_settings(ctx);
                }
            });
        self.show_settings = open;
    }

    fn save_settings(&mut self, ctx: &egui::Context) {
        let url = self.draft_url.trim().to_string();
        if let Err(err) = state::check_catalog_url(&url) {
            self.settings_error = Some(err);
            return;
        }
        self.settings.catalog_url = url;
        if self.clear_token {
            self.settings.github_token.clear();
        }
        if !self.draft_token.trim().is_empty() {
            self.settings.github_token = self.draft_token.trim().to_string();
        }
        if let Err(err) = state::save_settings(&self.settings) {
            self.settings_error = Some(err);
            return;
        }
        self.draft_token.clear();
        self.clear_token = false;
        self.settings_error = None;
        self.notice = Some(Notice {
            error: false,
            text: "Catalogue enregistré.".to_string(),
        });
        self.refresh(ctx);
    }
}

impl eframe::App for SuiteApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll();
        self.settings_window(&ctx);
        egui::Panel::bottom("status")
            .frame(Frame::NONE.fill(BG).inner_margin(12.0))
            .show(ui, |ui| self.footer(ui, &ctx));
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(BG).inner_margin(16.0))
            .show(ui, |ui| {
                self.header(ui);
                self.banner(ui, &ctx);
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.cards(ui, &ctx));
            });
        if self.checking || self.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

fn card_buttons(
    ui: &mut egui::Ui,
    query: &Query,
    installed: Option<&Installed>,
    job: Option<&(String, Option<f32>)>,
    busy: bool,
) -> Option<CardAction> {
    if let Some((label, fraction)) = job {
        show_progress(ui, *fraction, label);
        if ui.button("Annuler").clicked() {
            return Some(CardAction::Cancel);
        }
        return None;
    }
    match query {
        Query::Checking => {
            let _ = wide_button(ui, false, "VÉRIFICATION…");
            None
        }
        Query::Failed(_) | Query::Missing => {
            let _ = wide_button(ui, false, "INDISPONIBLE");
            None
        }
        Query::Ready(info) => match installed {
            None => wide_button(ui, !busy, "INSTALLER").then_some(CardAction::Install),
            Some(local) if github::is_newer(&info.version, &local.version) => {
                let mut action = None;
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !busy,
                            egui::Button::new(RichText::new("METTRE À JOUR").color(ACCENT)),
                        )
                        .clicked()
                    {
                        action = Some(CardAction::Update);
                    }
                    if ui.button("OUVRIR").clicked() {
                        action = Some(CardAction::Open);
                    }
                });
                action
            }
            Some(_) => wide_button(ui, true, "OUVRIR").then_some(CardAction::Open),
        },
    }
}

fn wide_button(ui: &mut egui::Ui, enabled: bool, text: impl Into<egui::WidgetText>) -> bool {
    let width = ui.available_width();
    ui.add_enabled(
        enabled,
        egui::Button::new(text).min_size(Vec2::new(width, 28.0)),
    )
    .clicked()
}

fn show_progress(ui: &mut egui::Ui, fraction: Option<f32>, label: &str) {
    ui.label(RichText::new(label).color(MUTED).size(12.0));
    match fraction {
        Some(value) => {
            ui.add(egui::ProgressBar::new(value.clamp(0.0, 1.0)).show_percentage());
        }
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("en cours");
            });
        }
    }
}

fn badge(query: &Query) -> String {
    match query {
        Query::Ready(info) => format!("v{}", info.version),
        _ => "—".to_string(),
    }
}

fn status_line(query: &Query) -> (String, Color32) {
    match query {
        Query::Ready(info) => (
            format!("Dernière mise à jour : {}", info.published_label),
            MUTED,
        ),
        Query::Failed(err) => (err.clone(), DANGER),
        Query::Missing => ("Aucune version publiée".to_string(), MUTED),
        Query::Checking => ("Vérification…".to_string(), MUTED),
    }
}

fn query_from(result: Result<Option<ReleaseInfo>, String>) -> Query {
    match result {
        Ok(Some(info)) => Query::Ready(info),
        Ok(None) => Query::Missing,
        Err(err) => Query::Failed(err),
    }
}

fn matches_query(app: &AppEntry, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    app.name.to_lowercase().contains(&query)
        || app.summary.to_lowercase().contains(&query)
        || app.id.to_lowercase().contains(&query)
}

fn parse_accent(hex: &str) -> Option<Color32> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color32::from_rgb(red, green, blue))
}

fn mark_tile(ui: &mut egui::Ui, accent: Color32, mark: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 6.0, accent);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        mark,
        egui::FontId::proportional(13.0),
        Color32::from_rgb(0x14, 0x10, 0x08),
    );
}

fn prune_missing(state: &mut SuiteState) -> bool {
    let before = state.apps.len();
    state
        .apps
        .retain(|id, installed| install::installed_exe(id, &installed.exe).is_some());
    state.apps.len() != before
}

fn swap_in(staged: &std::path::Path) -> Result<(), String> {
    let current =
        std::env::current_exe().map_err(|err| state::io_fr("Programme en cours", &err))?;
    std::process::Command::new(staged)
        .arg("--apply-update")
        .arg(&current)
        .spawn()
        .map_err(|err| state::io_fr("Relance du lanceur", &err))?;
    std::process::exit(0);
}

fn theme() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = BG;
    visuals.panel_fill = BG;
    visuals.extreme_bg_color = Color32::from_rgb(0x0e, 0x0e, 0x10);
    visuals.faint_bg_color = Color32::from_rgb(0x2a, 0x2a, 0x30);
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.noninteractive.bg_fill = CARD;
    visuals.widgets.noninteractive.fg_stroke.color = TEXT;
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(0x2c, 0x2c, 0x32);
    visuals.widgets.inactive.fg_stroke.color = TEXT;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x3a, 0x3a, 0x42);
    visuals.widgets.hovered.fg_stroke.color = TEXT;
    visuals.widgets.active.bg_fill = Color32::from_rgb(0x48, 0x48, 0x52);
    visuals.widgets.active.fg_stroke.color = TEXT;
    visuals.widgets.open.bg_fill = Color32::from_rgb(0x2c, 0x2c, 0x32);
    visuals.widgets.open.fg_stroke.color = TEXT;
    visuals.selection.bg_fill = Color32::from_rgb(0x3d, 0x32, 0x14);
    visuals.selection.stroke.color = ACCENT;
    visuals.hyperlink_color = ACCENT;
    visuals
}

fn install_font(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ];
    let Some(bytes) = candidates.iter().find_map(|path| std::fs::read(path).ok()) else {
        return;
    };
    ctx.add_font(egui::FontInsert::new(
        "suite",
        egui::FontData::from_owned(bytes),
        vec![egui::InsertFontFamily {
            family: egui::FontFamily::Proportional,
            priority: egui::FontPriority::Highest,
        }],
    ));
}
