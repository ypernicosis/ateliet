//! Pinterest in image mode: download a board (or a pin) into the shared
//! library or a vault folder, follow boards and check them every hour (as
//! Pictogrep does, and not while it is open: then it does it).

use super::*;
use ateliet_core::images::Origin;
use ateliet_core::pictogrep::{self, Home};
use ateliet_core::pinterest::{self, Dest, Downloads, Followed, Plan};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) struct Form {
    url: String,
    folder: String,
    /// "new" (a collection or folder of its own), "existing", "direct"
    mode: String,
    backfill: bool,
    follow: bool,
    error: Option<String>,
}

pub(super) struct PinState {
    pub downloads: Arc<Downloads>,
    form: Option<Form>,
    pub open: bool,
    last_sync: Option<Instant>,
    /// The download whose end was already taken in (a rescan).
    seen_end: bool,
}

impl Default for PinState {
    fn default() -> Self {
        PinState { downloads: Arc::new(Downloads::default()), form: None, open: false, last_sync: None, seen_end: true }
    }
}

impl ImagesState {
    fn data_dir(&self) -> PathBuf {
        match &self.shared {
            Some(h) => h.data(),
            None => self.root.join(".obsidian").join("ateliet"),
        }
    }

    /// Pictures already at the destination, with their digests (the duplicate check).
    fn existing(&self, dest_dir: Option<&Path>) -> (Vec<PathBuf>, HashMap<PathBuf, [u8; 32]>) {
        let ix = self.index.lock().unwrap();
        let roots = self.shared.as_ref().map(pictogrep::source_roots).unwrap_or_default();
        let mut paths = Vec::new();
        let mut digests = HashMap::new();
        for e in &self.entries {
            let p = pictogrep::clean(Path::new(&e.abs));
            let wanted = match dest_dir {
                Some(d) => p.starts_with(d),
                None => e.origin == Origin::Shared && roots.iter().any(|r| p.starts_with(r)),
            };
            if !wanted {
                continue;
            }
            if let Some(d) = ix.records.get(&e.key).and_then(|r| r.digest) {
                digests.insert(p.clone(), d);
            }
            paths.push(p);
        }
        (paths, digests)
    }

    #[allow(clippy::too_many_arguments)]
    fn plan(&self, url: &str, name: &str, collection: Option<String>, vault_dir: Option<PathBuf>, limit: usize, link_duplicates: bool, allow_empty: bool) -> Plan {
        let data = self.data_dir();
        let archive = Some(pinterest::archive_path(&data, url));
        let (dest, (existing, digests)) = match (&self.shared, vault_dir) {
            (Some(h), _) => (Dest::Shared { home: h.clone(), collection }, self.existing(None)),
            (None, dir) => {
                let dir = dir.unwrap_or_else(|| self.root.join("Pinterest"));
                let ex = self.existing(Some(&dir));
                (Dest::Vault { dir }, ex)
            }
        };
        Plan { source: url.into(), name: name.into(), dest, archive, limit, link_duplicates, allow_empty, existing, digests }
    }

    fn run(&mut self, plan: Plan, cancel: Arc<std::sync::atomic::AtomicBool>, ctx: &egui::Context) {
        let downloads = self.pin.downloads.clone();
        let ctx = ctx.clone();
        self.pin.seen_end = false;
        std::thread::spawn(move || {
            let report = |phase: &str, done: usize, total: usize| {
                downloads.progress(phase, done, total);
                ctx.request_repaint();
            };
            match pinterest::run(&plan, &cancel, &report) {
                Ok(o) => downloads.finish(Ok(o.result)),
                Err(e) => downloads.finish(Err(e)),
            }
            ctx.request_repaint();
        });
    }

    fn start(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let f = self.pin.form.as_ref().ok_or("")?;
        let url = pinterest::validate_url(&f.url)?;
        let source = url.to_string();
        let name = pinterest::source_name(&url);
        if !f.backfill && !f.follow {
            return Err("Escolha baixar a pasta, segui-la, ou os dois.".into());
        }
        if f.mode == "direct" && f.follow {
            return Err("Para seguir uma pasta, as imagens precisam de uma coleção ou pasta própria.".into());
        }
        let (collection, vault_dir) = match (&self.shared, f.mode.as_str()) {
            (Some(h), "existing") => {
                let n = pictogrep::collection_name(&f.folder)?;
                if !pictogrep::collection_names(h).contains(&n) {
                    return Err("Escolha uma coleção que já existe.".into());
                }
                (Some(n), None)
            }
            (Some(_), "direct") => (None, None),
            (Some(_), _) => (Some(pictogrep::collection_name(&if f.folder.trim().is_empty() { name.replace('.', " ") } else { f.folder.replace('.', " ") }).map_err(|_| "Esse nome de coleção não serve; escolha outro.".to_string())?), None),
            (None, m) => {
                let base = self.root.join("Pinterest");
                let dir = match m {
                    "direct" => base,
                    "existing" => self.root.join(f.folder.trim().trim_matches('/')),
                    _ => base.join(if f.folder.trim().is_empty() { name.as_str() } else { f.folder.trim() }),
                };
                (None, Some(dir))
            }
        };
        let cancel = self.pin.downloads.start(&name, false).ok_or("Já há um download em andamento.")?;
        let data = self.data_dir();
        if f.follow {
            let folder = collection.clone().or_else(|| vault_dir.as_ref().map(|d| d.strip_prefix(&self.root).unwrap_or(d).to_string_lossy().replace('\\', "/"))).unwrap_or_default();
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            let _ = pinterest::follow(&data, Followed { url: source.clone(), name: name.clone(), folder, last_sync_at: t, added_at: t });
        }
        let limit = if f.backfill { 0 } else { pinterest::FOLLOW_NEWEST };
        let plan = self.plan(&source, &name, collection, vault_dir, limit, true, false);
        self.run(plan, cancel, ctx);
        Ok(())
    }

    /// A followed board's newest pins.
    fn check_followed(&mut self, f: &Followed, automatic: bool, ctx: &egui::Context) -> Result<(), String> {
        let data = self.data_dir();
        let source = pinterest::validate_url(&f.url)?.to_string();
        let (collection, vault_dir) = match &self.shared {
            Some(_) => (pictogrep::collection_name(&f.folder.replace('.', " ")).ok(), None),
            None => (None, Some(self.root.join(&f.folder))),
        };
        let cancel = self.pin.downloads.start(&f.name, automatic).ok_or("Já há um download em andamento.")?;
        pinterest::mark_synced(&data, &f.url);
        let plan = self.plan(&source, &f.name, collection, vault_dir, pinterest::FOLLOW_NEWEST, true, true);
        self.run(plan, cancel, ctx);
        Ok(())
    }

    /// Once an hour: followed boards that are due, unless Pictogrep is open
    /// (it checks them itself) or its automatic checks are off.
    pub(super) fn pin_tick(&mut self, ctx: &egui::Context) {
        let job = self.pin.downloads.snapshot();
        if job.state != "running" && !self.pin.seen_end {
            self.pin.seen_end = true;
            self.rescan();
        }
        if self.root.as_os_str().is_empty() || self.pin.last_sync.is_some_and(|t| t.elapsed() < Duration::from_secs(3600)) {
            return;
        }
        if self.pin.last_sync.is_none() {
            // two minutes after the library opens
            self.pin.last_sync = Some(Instant::now() - Duration::from_secs(3600 - 120));
            return;
        }
        self.pin.last_sync = Some(Instant::now());
        if let Some(h) = &self.shared {
            if pictogrep::is_running(h) || !pinterest::auto_sync(h) {
                return;
            }
        }
        if pinterest::gallery_dl().is_none() || job.state == "running" {
            return;
        }
        if let Some(f) = pinterest::followed(&self.data_dir()).into_iter().find(pinterest::due) {
            let _ = self.check_followed(&f, true, ctx);
        }
    }

    fn home_label(home: &Option<Home>) -> &'static str {
        if home.is_some() {
            "coleção"
        } else {
            "pasta"
        }
    }

    /// The Pinterest window.
    pub(super) fn pin_ui(&mut self, ctx: &egui::Context, p: crate::theme::Palette) {
        if !self.pin.open {
            return;
        }
        if self.pin.form.is_none() {
            self.pin.form = Some(Form { url: String::new(), folder: String::new(), mode: "new".into(), backfill: true, follow: false, error: None });
        }
        let job = self.pin.downloads.snapshot();
        let mut close = false;
        let mut go = false;
        let mut stop = false;
        let mut check: Option<Followed> = None;
        let mut forget: Option<String> = None;
        let data = self.data_dir();
        let followed = pinterest::followed(&data);
        let unit = Self::home_label(&self.shared);
        let collections = self.shared.as_ref().map(pictogrep::collection_names).unwrap_or_default();
        let has_tool = pinterest::gallery_dl().is_some();
        kit::scrim(ctx, p);
        egui::Window::new("pinterest").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, -20.0]).show(ctx, |ui| {
            ui.set_width(500.0);
            let f = self.pin.form.as_mut().unwrap();
            ui.label(RichText::new("Trazer do Pinterest").font(kit::medium(17.0)).color(p.text));
            ui.label(RichText::new(match &self.shared { Some(_) => "As imagens vão para a biblioteca compartilhada (a mesma do Pictogrep).", None => "Sem biblioteca compartilhada, as imagens vão para a pasta Pinterest do cofre." }).size(12.0).color(p.muted));
            ui.add_space(8.0);
            ui.add(egui::TextEdit::singleline(&mut f.url).hint_text("https://pinterest.com/usuario/pasta/").desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6)));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (k, l) in [("new", format!("Nova {unit}")), ("existing", format!("{} existente", if unit == "coleção" { "Coleção" } else { "Pasta" })), ("direct", "Direto".to_string())] {
                    if kit::chip(ui, None, &l, f.mode == k, p).clicked() {
                        f.mode = k.into();
                    }
                }
            });
            match f.mode.as_str() {
                "new" => {
                    ui.add(egui::TextEdit::singleline(&mut f.folder).hint_text("Nome (vazio: o nome da pasta do Pinterest)").desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 5)));
                }
                "existing" if !collections.is_empty() => {
                    egui::ComboBox::from_id_salt("pin-coll").width(240.0).selected_text(if f.folder.is_empty() { "Escolha a coleção".to_string() } else { f.folder.clone() }).show_ui(ui, |ui| {
                        for c in &collections {
                            if ui.selectable_label(&f.folder == c, c.as_str()).clicked() {
                                f.folder = c.clone();
                            }
                        }
                    });
                }
                "existing" => {
                    ui.add(egui::TextEdit::singleline(&mut f.folder).hint_text("Pasta do cofre").desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 5)));
                }
                _ => {}
            }
            ui.horizontal(|ui| {
                kit::toggle(ui, &mut f.backfill, p);
                ui.label(RichText::new("Baixar o que já está lá").size(12.5).color(p.muted));
                ui.add_space(10.0);
                kit::toggle(ui, &mut f.follow, p);
                ui.label(RichText::new("Seguir (os pins novos, de hora em hora)").size(12.5).color(p.muted));
            });
            if !has_tool {
                ui.label(RichText::new("Sem o gallery-dl (ou o do Pictogrep) instalado, o download usa o leitor próprio, que traz menos pins.").size(11.5).color(p.faint));
            }
            if let Some(e) = &f.error {
                ui.label(RichText::new(e).size(12.0).color(p.error));
            }
            // the running (or last) download
            if job.state != "idle" {
                ui.add_space(6.0);
                let label = match job.state.as_str() {
                    "running" => format!("{}{}: {}", job.board, if job.automatic { " (automático)" } else { "" }, if job.phase == "importing" { "trazendo para a biblioteca" } else { "baixando" }),
                    "done" => format!("{}: pronto. {}", job.board, job.result.as_ref().and_then(|r| r["imported"].as_u64()).map(|n| format!("{n} imagens novas.")).unwrap_or_default()),
                    "cancelled" => format!("{}: parado.", job.board),
                    _ => format!("{}: {}", job.board, job.error.clone().unwrap_or_default()),
                };
                ui.label(RichText::new(label).size(12.5).color(if job.state == "error" { p.error } else { p.text }));
                if job.state == "running" {
                    let frac = if job.total > 0 { job.done as f32 / job.total as f32 } else { 0.0 };
                    ui.horizontal(|ui| {
                        ui.add(egui::ProgressBar::new(frac).desired_width(380.0).text(if job.total > 0 { format!("{} de {}", job.done, job.total) } else { "…".into() }));
                        if !job.stopping && kit::button(ui, None, "Parar", p).clicked() {
                            stop = true;
                        }
                    });
                }
            }
            if !followed.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("Pastas seguidas").font(kit::medium(13.0)).color(p.text));
                egui::ScrollArea::vertical().max_height(150.0).id_salt("pin-follow").show(ui, |ui| {
                    for fl in &followed {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&fl.name).size(12.5).color(p.text)).on_hover_text(&fl.url);
                            ui.label(RichText::new(format!("→ {}", fl.folder)).size(11.5).color(p.faint));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if kit::icon_button_sized(ui, icons::X, "Deixar de seguir", false, p, 20.0, 11.0).clicked() {
                                    forget = Some(fl.url.clone());
                                }
                                if kit::icon_button_sized(ui, icons::ROTATE_CW, "Ver agora os pins novos", false, p, 20.0, 11.0).clicked() {
                                    check = Some(fl.clone());
                                }
                            });
                        });
                    }
                });
            }
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let busy = job.state == "running";
                if ui.add_enabled_ui(!busy, |ui| kit::primary(ui, "Trazer", p)).inner.clicked() {
                    go = true;
                }
                if kit::button(ui, None, "Fechar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
        if go {
            match self.start(ctx) {
                Ok(()) => {
                    if let Some(f) = self.pin.form.as_mut() {
                        f.error = None;
                        f.url.clear();
                    }
                }
                Err(e) => {
                    if let Some(f) = self.pin.form.as_mut() {
                        f.error = Some(e);
                    }
                }
            }
        }
        if stop {
            self.pin.downloads.stop();
        }
        if let Some(f) = check {
            if let Err(e) = self.check_followed(&f, false, ctx) {
                if let Some(fm) = self.pin.form.as_mut() {
                    fm.error = Some(e);
                }
            }
        }
        if let Some(u) = forget {
            let _ = pinterest::forget(&data, &u);
        }
        if close {
            self.pin.open = false;
        }
        if job.state == "running" {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
    }
}
