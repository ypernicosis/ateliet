//! The character sheet: portrait, gallery, a memorable line and attributes
//! on the left; name, role, colour and the sections and lists of people as
//! cards on the right. Saved back to the same Markdown note.

use super::library::Cx;
use super::picker::{Picker, Purpose};
use super::Action;
use crate::host::{self, RenderHost};
use crate::icons;
use crate::kit;
use crate::markdown::{self, Renderer};
use crate::theme::Palette;
use ateliet_core::scriptorium::sheet::{attr_label, initials, rel_name, section_icon, Sheet};
use eframe::egui::{self, Color32, RichText, Sense};
use serde_json::{json, Value as Json};
use std::time::{Duration, Instant, SystemTime};

pub struct SheetView {
    pub path: String,
    pub sheet: Sheet,
    mtime: Option<SystemTime>,
    dirty: bool,
    last_edit: Option<Instant>,
    editing: Option<usize>,
    name: String,
    rename_attr: Option<(String, String)>,
    new_attr: Option<(String, String)>,
    new_section: Option<(String, bool)>,
}

fn icon_for(name: &str) -> &'static str {
    match name {
        "scan-face" => icons::SCAN_FACE,
        "heart" => icons::HEART,
        "route" => icons::ROUTE,
        "sticky-note" => icons::STICKY_NOTE,
        "scroll" => icons::SCROLL,
        "sparkles" => icons::SPARKLES,
        "users" => icons::USERS,
        "lock" => icons::LOCK,
        "message-square-quote" => icons::MESSAGE_SQUARE_QUOTE,
        "target" => icons::TARGET,
        _ => icons::ALIGN_LEFT,
    }
}

fn list_icon(key: &str) -> &'static str {
    match key {
        "aliados" => icons::SHIELD,
        "inimigos" => icons::SWORDS,
        "familia" | "família" => icons::HOUSE,
        "amores" => icons::HEART,
        "rivais" => icons::FLAME,
        "mentores" => icons::GRADUATION_CAP,
        _ => icons::USERS,
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

pub fn parse_color(s: &str) -> Option<Color32> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// A round avatar: the portrait, else initials on the character's colour.
fn avatar(ui: &mut egui::Ui, cx: &mut Cx, rect: egui::Rect, name: &str, portrait: &str, color: Option<Color32>, source: &str) {
    let p = cx.palette;
    let tex = if portrait.is_empty() { None } else { host::image(ui.ctx(), cx.vault, cx.caches, portrait, source) };
    match tex {
        Some((id, sz)) => {
            let k = (sz.x / sz.y).max(1e-3);
            let uv = if k > 1.0 { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0 / k, 1.0)) } else { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0, k)) };
            ui.painter().add(egui::epaint::RectShape::filled(rect, rect.width() / 2.0, Color32::WHITE).with_texture(id, uv));
        }
        None => {
            let bg = color.unwrap_or(p.bg_active);
            ui.painter().circle_filled(rect.center(), rect.width() / 2.0, bg);
            let fg = if color.is_some() { Color32::WHITE } else { p.muted };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initials(name), kit::medium(rect.width() * 0.38), fg);
        }
    }
}

fn card<R>(ui: &mut egui::Ui, p: Palette, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new().fill(p.bg).stroke(egui::Stroke::new(1.0_f32, p.border)).corner_radius(12.0).inner_margin(egui::Margin { left: 16, right: 12, top: 12, bottom: 14 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    })
    .inner
}

fn card_head(ui: &mut egui::Ui, p: Palette, glyph: &str, title: &str) {
    ui.horizontal(|ui| {
        ui.label(kit::icon(glyph, 13.0).color(p.faint));
        ui.label(RichText::new(title).font(kit::medium(13.0)).color(p.text));
    });
    ui.add_space(6.0);
}

impl SheetView {
    pub fn open(path: &str, root: &std::path::Path) -> SheetView {
        let abs = root.join(path);
        let text = std::fs::read_to_string(&abs).unwrap_or_default();
        let name = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md").to_string();
        SheetView { path: path.into(), sheet: Sheet::parse(&text), mtime: std::fs::metadata(&abs).and_then(|m| m.modified()).ok(), dirty: false, last_edit: None, editing: None, name, rename_attr: None, new_attr: None, new_section: None }
    }

    pub fn changed(&mut self) {
        self.dirty = true;
        self.last_edit = Some(Instant::now());
    }

    /// Write the note (debounced by the caller); returns the path written.
    pub fn save(&mut self, root: &std::path::Path) -> Option<String> {
        if !self.dirty {
            return None;
        }
        let abs = root.join(&self.path);
        if std::fs::write(&abs, self.sheet.to_text()).is_ok() {
            self.dirty = false;
            self.mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok();
            return Some(self.path.clone());
        }
        None
    }

    pub fn title(&self) -> String {
        self.path.rsplit('/').next().unwrap_or(&self.path).trim_end_matches(".md").to_string()
    }

    /// A picture or person chosen in the picker.
    pub fn picked(&mut self, purpose: &Purpose, value: &str) {
        match purpose {
            Purpose::Portrait(_) => {
                self.sheet.fm.insert("retrato".into(), json!(format!("[[{value}]]")));
            }
            Purpose::Gallery(_) => {
                let mut g = self.sheet.list("galeria");
                g.push(json!(format!("[[{value}]]")));
                self.sheet.fm.insert("galeria".into(), Json::Array(g));
            }
            Purpose::Person(_, list) => {
                let mut l = self.sheet.list(list);
                l.push(json!(value));
                self.sheet.fm.insert(list.clone(), Json::Array(l));
            }
            _ => return,
        }
        self.changed();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<Action> {
        let p = cx.palette;
        let mut acts = Vec::new();
        // the file changed on disk and there is nothing unsaved here: reload
        let abs = cx.vault.abs(&self.path);
        let now = std::fs::metadata(&abs).and_then(|m| m.modified()).ok();
        if now != self.mtime && !self.dirty {
            if let Ok(t) = std::fs::read_to_string(&abs) {
                self.sheet = Sheet::parse(&t);
            }
            self.mtime = now;
        }
        if self.dirty && self.last_edit.is_some_and(|t| t.elapsed() > Duration::from_millis(800)) {
            if let Some(p) = self.save(&cx.vault.root) {
                acts.push(Action::Saved(p));
            }
        }
        if self.dirty {
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        let accent = parse_color(&self.sheet.str("cor"));
        egui::ScrollArea::vertical().id_salt(("sheet", &self.path)).auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_width();
            let w = avail.min(1080.0);
            let pad = if avail < 600.0 { 18.0 } else { 28.0 };
            let side = ((avail - w) / 2.0).max(0.0) + pad;
            let total = (w - 2.0 * pad).max(280.0);
            ui.add_space(28.0);
            if total < 760.0 {
                // narrow: one column, the sheet's header first
                ui.horizontal_top(|ui| {
                    ui.add_space(side);
                    ui.allocate_ui_with_layout(egui::vec2(total, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(total);
                        self.right(ui, cx, &mut acts, accent, true);
                        ui.add_space(14.0);
                        ui.allocate_ui_with_layout(egui::vec2(total.min(420.0), 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(total.min(420.0));
                            self.left(ui, cx, &mut acts, accent, true);
                        });
                    });
                });
            } else {
                ui.horizontal_top(|ui| {
                    ui.add_space(side);
                    let left_w = 270.0;
                    ui.allocate_ui_with_layout(egui::vec2(left_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(left_w);
                        self.left(ui, cx, &mut acts, accent, false);
                    });
                    ui.add_space(24.0);
                    ui.allocate_ui_with_layout(egui::vec2(total - left_w - 24.0, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(total - left_w - 24.0);
                        self.right(ui, cx, &mut acts, accent, false);
                    });
                });
            }
            ui.add_space(60.0);
        });
        let _ = p;
        acts
    }

    fn left(&mut self, ui: &mut egui::Ui, cx: &mut Cx, acts: &mut Vec<Action>, accent: Option<Color32>, compact: bool) {
        if !compact {
            self.portrait(ui, cx, acts, accent);
        }
        self.gallery_and_rest(ui, cx, acts);
    }

    fn portrait(&mut self, ui: &mut egui::Ui, cx: &mut Cx, acts: &mut Vec<Action>, accent: Option<Color32>) {
        let p = cx.palette;
        let pw = ui.available_width().min(270.0);
        let (rect, r) = ui.allocate_exact_size(egui::vec2(pw, pw * 1.22), Sense::click());
        let retrato = ateliet_core::scriptorium::link_text(self.sheet.fm.get("retrato").unwrap_or(&Json::Null));
        let tex = if retrato.is_empty() { None } else { host::image(ui.ctx(), cx.vault, cx.caches, &retrato, &self.path) };
        match tex {
            Some((id, sz)) => {
                let k = (rect.width() / sz.x).max(rect.height() / sz.y);
                let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(rect.width() / (sz.x * k), rect.height() / (sz.y * k)));
                ui.painter().add(egui::epaint::RectShape::filled(rect, 14.0, Color32::WHITE).with_texture(id, uv));
            }
            None => {
                let bg = accent.map(|c| c.gamma_multiply(0.85)).unwrap_or(p.bg_secondary);
                ui.painter().rect_filled(rect, 14.0, bg);
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initials(&self.title()), kit::medium(64.0), if accent.is_some() { Color32::WHITE } else { p.faint });
            }
        }
        if r.hovered() {
            let chip = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.bottom() - 26.0), egui::vec2(140.0, 30.0));
            ui.painter().rect_filled(chip, 15.0, Color32::from_black_alpha(130));
            ui.painter().text(chip.center(), egui::Align2::CENTER_CENTER, "Trocar retrato", egui::FontId::proportional(12.5), Color32::WHITE);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if r.clicked() {
            acts.push(Action::Pick(Box::new(image_picker(cx, "Retrato", Purpose::Portrait(self.path.clone())))));
        }
    }

    fn gallery_and_rest(&mut self, ui: &mut egui::Ui, cx: &mut Cx, acts: &mut Vec<Action>) {
        let p = cx.palette;
        // gallery
        ui.add_space(10.0);
        let gallery = self.sheet.list("galeria");
        let mut remove: Option<usize> = None;
        let mut make_portrait: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            for (i, g) in gallery.iter().enumerate() {
                let link = ateliet_core::scriptorium::link_text(g);
                let (rect, r) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), Sense::click());
                match host::image(ui.ctx(), cx.vault, cx.caches, &link, &self.path) {
                    Some((id, sz)) => {
                        let k = (sz.x / sz.y).max(1e-3);
                        let uv = if k > 1.0 { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0 / k, 1.0)) } else { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0, k)) };
                        ui.painter().add(egui::epaint::RectShape::filled(rect, 8.0, Color32::WHITE).with_texture(id, uv));
                    }
                    None => {
                        ui.painter().rect_filled(rect, 8.0, p.bg_active);
                    }
                }
                let r = r.on_hover_text(&link);
                if r.clicked() {
                    acts.push(Action::Open(link.clone(), true));
                }
                r.context_menu(|ui| {
                    if kit::menu_item(ui, icons::CONTACT, "Usar como retrato", p) {
                        make_portrait = Some(link.clone());
                        ui.close_menu();
                    }
                    if kit::menu_item(ui, icons::X, "Remover da galeria", p) {
                        remove = Some(i);
                        ui.close_menu();
                    }
                });
            }
            let (rect, r) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), Sense::click());
            ui.painter().rect_stroke(rect, 8.0, egui::Stroke::new(1.0_f32, if r.hovered() { p.border_strong } else { p.border }), egui::StrokeKind::Inside);
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icons::PLUS, kit::icon_font(15.0), p.faint);
            if r.on_hover_text("Adicionar imagem à galeria").clicked() {
                acts.push(Action::Pick(Box::new(image_picker(cx, "Imagem para a galeria", Purpose::Gallery(self.path.clone())))));
            }
        });
        if let Some(i) = remove {
            let mut g = gallery.clone();
            g.remove(i);
            self.sheet.fm.insert("galeria".into(), Json::Array(g));
            self.changed();
        }
        if let Some(l) = make_portrait {
            self.sheet.fm.insert("retrato".into(), json!(format!("[[{l}]]")));
            self.changed();
        }
        ui.add_space(14.0);
        // the memorable line
        card(ui, p, |ui| {
            card_head(ui, p, icons::QUOTE, "Frase marcante");
            let mut frase = self.sheet.str("frase");
            let fid = egui::Id::new(("sheet-frase", &self.path));
            if ui.add(egui::TextEdit::multiline(&mut frase).id(fid).frame(false).desired_rows(2).desired_width(f32::INFINITY).hint_text(RichText::new("Uma fala que é a cara dele…").color(p.faint)).font(egui::FontId::new(14.0, egui::FontFamily::Name("italic".into())))).changed() | crate::editor::auto_dash(ui, fid, &mut frase) {
                self.sheet.fm.insert("frase".into(), json!(frase));
                self.changed();
            }
        });
        ui.add_space(12.0);
        // attributes
        card(ui, p, |ui| {
            card_head(ui, p, icons::SLIDERS_HORIZONTAL, "Atributos");
            let keys = self.sheet.attr_keys();
            let mut op: Option<(String, &str)> = None;
            for k in &keys {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let label_w = 96.0;
                    match &mut self.rename_attr {
                        Some((from, to)) if from == k => {
                            let r = ui.add(egui::TextEdit::singleline(to).desired_width(label_w).margin(egui::Margin::symmetric(4, 2)));
                            r.request_focus();
                            if r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                op = Some((k.clone(), "rename"));
                            }
                        }
                        _ => {
                            let r = ui.allocate_ui_with_layout(egui::vec2(label_w, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.set_width(label_w);
                                ui.add(egui::Label::new(RichText::new(attr_label(k)).size(12.5).color(p.faint)).sense(Sense::click()).truncate())
                            });
                            if r.inner.on_hover_text("Clique para renomear").clicked() {
                                self.rename_attr = Some((k.clone(), attr_label(k)));
                            }
                        }
                    }
                    let was_list = self.sheet.fm.get(k).is_some_and(|v| v.is_array());
                    let mut v = self.sheet.str(k);
                    let r = ui.add(egui::TextEdit::singleline(&mut v).frame(false).desired_width((ui.available_width() - 34.0).max(40.0)).hint_text(RichText::new("—").color(p.faint)));
                    if r.changed() {
                        let val = if was_list { Json::Array(v.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).map(|x| json!(x)).collect()) } else { json!(v) };
                        self.sheet.fm.insert(k.clone(), val);
                        self.changed();
                    }
                    let mr = kit::icon_button_sized(ui, icons::ELLIPSIS_VERTICAL, "", false, p, 20.0, 12.0);
                    let popup = ui.make_persistent_id(("attr-menu", k));
                    if mr.clicked() {
                        ui.memory_mut(|m| m.toggle_popup(popup));
                    }
                    egui::popup_below_widget(ui, popup, &mr, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                        ui.set_min_width(150.0);
                        if kit::menu_item(ui, icons::PENCIL, "Renomear", p) {
                            self.rename_attr = Some((k.clone(), attr_label(k)));
                        }
                        if kit::menu_item(ui, icons::ARROW_UP, "Mover para cima", p) {
                            op = Some((k.clone(), "up"));
                        }
                        if kit::menu_item(ui, icons::ARROW_DOWN, "Mover para baixo", p) {
                            op = Some((k.clone(), "down"));
                        }
                        if kit::menu_danger(ui, icons::TRASH_2, "Remover", p) {
                            op = Some((k.clone(), "remove"));
                        }
                    });
                });
            }
            if keys.is_empty() {
                ui.label(RichText::new("Nenhum atributo.").size(12.5).color(p.faint));
            }
            if let Some((k, what)) = op {
                let i = keys.iter().position(|x| *x == k).unwrap_or(0);
                match what {
                    "up" if i > 0 => {
                        let mut o = keys.clone();
                        o.swap(i, i - 1);
                        self.sheet.reorder_attrs(&o);
                    }
                    "down" if i + 1 < keys.len() => {
                        let mut o = keys.clone();
                        o.swap(i, i + 1);
                        self.sheet.reorder_attrs(&o);
                    }
                    "remove" => {
                        self.sheet.fm.shift_remove(&k);
                    }
                    "rename" => {
                        if let Some((_, to)) = self.rename_attr.take() {
                            let nk = to.trim().to_lowercase();
                            if !nk.is_empty() && nk != k && nk != attr_label(&k) && !self.sheet.fm.contains_key(&nk) && !self.sheet.reserved(&nk) {
                                self.sheet.rename_key(&k, &nk);
                            }
                        }
                    }
                    _ => {}
                }
                self.changed();
            }
            ui.add_space(6.0);
            match &mut self.new_attr {
                Some((k, v)) => {
                    let mut done = None;
                    ui.horizontal(|ui| {
                        let r = ui.add(egui::TextEdit::singleline(k).hint_text("nome").desired_width(96.0).margin(egui::Margin::symmetric(4, 2)));
                        if k.is_empty() && !r.has_focus() && v.is_empty() {
                            r.request_focus();
                        }
                        ui.add(egui::TextEdit::singleline(v).hint_text("valor").desired_width(ui.available_width() - 30.0).margin(egui::Margin::symmetric(4, 2)));
                        if kit::icon_button_sized(ui, icons::CHECK, "Adicionar", false, p, 22.0, 12.0).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            done = Some(true);
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            done = Some(false);
                        }
                    });
                    if let Some(ok) = done {
                        let (k, v) = self.new_attr.take().unwrap();
                        let k = k.trim().to_lowercase();
                        if ok && !k.is_empty() && !self.sheet.fm.contains_key(&k) && !self.sheet.reserved(&k) {
                            self.sheet.fm.insert(k, json!(v));
                            self.changed();
                        }
                    }
                }
                None => {
                    if kit::quiet(ui, Some(icons::PLUS), "Atributo", p).clicked() {
                        self.new_attr = Some((String::new(), String::new()));
                    }
                }
            }
        });
    }

    fn right(&mut self, ui: &mut egui::Ui, cx: &mut Cx, acts: &mut Vec<Action>, accent: Option<Color32>, narrow: bool) {
        let p = cx.palette;
        // header: (avatar when narrow), name, role, colour, Markdown
        ui.horizontal(|ui| {
            if narrow {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(72.0, 72.0), Sense::click());
                let retrato = ateliet_core::scriptorium::link_text(self.sheet.fm.get("retrato").unwrap_or(&Json::Null));
                let name = self.title();
                avatar(ui, cx, r, &name, &retrato, accent, &self.path.clone());
                if resp.on_hover_text("Trocar retrato").clicked() {
                    acts.push(Action::Pick(Box::new(image_picker(cx, "Retrato", Purpose::Portrait(self.path.clone())))));
                }
                ui.add_space(8.0);
            }
            ui.vertical(|ui| {
            ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.name).frame(false).font(kit::bold(30.0)).desired_width(ui.available_width() - 110.0));
            if r.lost_focus() {
                let nn = ateliet_core::scriptorium::safe_name(&self.name);
                if nn != self.title() && !nn.is_empty() {
                    acts.push(Action::Rename(self.path.clone(), nn));
                } else {
                    self.name = self.title();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if kit::icon_button(ui, icons::FILE_TEXT, "Abrir como Markdown", false, p).clicked() {
                    acts.push(Action::AsMarkdown(self.path.clone()));
                }
                let mut c = accent.unwrap_or(Color32::from_rgb(0x8a, 0x5a, 0x2b));
                if ui.color_edit_button_srgba(&mut c).on_hover_text("Cor do personagem").changed() {
                    self.sheet.fm.insert("cor".into(), json!(format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())));
                    self.changed();
                }
            });
            });
            let mut role = self.sheet.str("papel");
            if ui.add(egui::TextEdit::singleline(&mut role).frame(false).desired_width(f32::INFINITY).font(egui::FontId::proportional(15.0)).text_color(p.muted).hint_text(RichText::new("Papel na história…").color(p.faint))).changed() {
                self.sheet.fm.insert("papel".into(), json!(role));
                self.changed();
            }
            });
        });
        if let Some(c) = accent {
            let (r, _) = ui.allocate_exact_size(egui::vec2(48.0, 3.0), Sense::hover());
            ui.painter().rect_filled(r, 1.5, c);
        }
        ui.add_space(18.0);

        // sections: two columns, long ones full width
        let width = ui.available_width();
        let two = width > 560.0 && !narrow;
        let col_w = if two { (width - 14.0) / 2.0 } else { width };
        let n = self.sheet.sections.len();
        let mut i = 0;
        let mut op: Option<(usize, &str)> = None;
        while i < n {
            let wide = |t: &str| {
                let l = t.to_lowercase();
                l.contains("arco") || l.contains("hist") || l.contains("notas") || l.contains("origem") || l == "ars"
            };
            let pair = two && !wide(&self.sheet.sections[i].0) && i + 1 < n && !wide(&self.sheet.sections[i + 1].0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let take = if pair { 2 } else { 1 };
                for j in i..i + take {
                    let w = if pair { col_w } else { width };
                    ui.allocate_ui_with_layout(egui::vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(w);
                        if let Some(o) = self.section_card(ui, cx, j, acts) {
                            op = Some((j, o));
                        }
                    });
                }
            });
            ui.add_space(14.0);
            i += if pair { 2 } else { 1 };
        }
        if let Some((j, what)) = op {
            match what {
                "up" if j > 0 => self.sheet.sections.swap(j, j - 1),
                "down" if j + 1 < n => self.sheet.sections.swap(j, j + 1),
                "remove" => {
                    self.sheet.sections.remove(j);
                }
                _ => {}
            }
            self.editing = None;
            self.changed();
        }

        // lists of people
        let lists = self.sheet.lists();
        let mut list_op: Option<(String, usize, &str)> = None;
        for key in &lists {
            card(ui, p, |ui| {
                ui.horizontal(|ui| {
                    ui.label(kit::icon(list_icon(key), 13.0).color(p.faint));
                    ui.label(RichText::new(capitalize(&attr_label(key))).font(kit::medium(13.0)).color(p.text));
                });
                ui.add_space(6.0);
                let entries = self.sheet.list(key);
                for (idx, v) in entries.iter().enumerate() {
                    let name = rel_name(v);
                    let target = cx.vault.index.read().unwrap().resolve(&ateliet_core::scriptorium::link_text(v), &self.path);
                    let (portrait, color) = target.as_ref().and_then(|t| cx.vault.index.read().unwrap().page(t).map(|pg| (ateliet_core::scriptorium::link_text(pg.yaml.get("retrato").unwrap_or(&Json::Null)), pg.yaml.get("cor").and_then(|c| c.as_str()).and_then(parse_color)))).unwrap_or_default();
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::hover());
                        avatar(ui, cx, r, &name, &portrait, color, target.as_deref().unwrap_or(&self.path));
                        let nr = ui.add(egui::Label::new(RichText::new(&name).font(kit::medium(13.0)).color(if target.is_some() { p.text } else { p.muted })).sense(Sense::click()));
                        if target.is_some() && nr.on_hover_text("Abrir a ficha").clicked() {
                            acts.push(Action::Open(target.clone().unwrap(), true));
                        }
                        let mut bond = self.sheet.bond(&name);
                        let hint = if key == "inimigos" { "por quê?".to_string() } else { format!("o que é para {}?", self.title().split(' ').next().unwrap_or("")) };
                        if ui.add(egui::TextEdit::singleline(&mut bond).frame(false).desired_width(ui.available_width() - 26.0).text_color(p.muted).hint_text(RichText::new(hint).color(p.faint))).changed() {
                            self.sheet.set_bond(&name, &bond);
                            self.changed();
                        }
                        let mr = kit::icon_button_sized(ui, icons::ELLIPSIS_VERTICAL, "", false, p, 20.0, 12.0);
                        let popup = ui.make_persistent_id(("rel-menu", key, idx));
                        if mr.clicked() {
                            ui.memory_mut(|m| m.toggle_popup(popup));
                        }
                        egui::popup_below_widget(ui, popup, &mr, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                            ui.set_min_width(180.0);
                            if target.is_none() && kit::menu_item(ui, icons::FILE_PLUS, &format!("Criar ficha para {name}"), p) {
                                acts.push(Action::CreateSheetFor { sheet: self.path.clone(), list: key.clone(), idx, name: name.clone() });
                            }
                            for other in lists.iter().filter(|o| *o != key) {
                                if kit::menu_item(ui, icons::ARROW_RIGHT_LEFT, &format!("Mover para {}", attr_label(other)), p) {
                                    list_op = Some((format!("{key}\u{1}{other}"), idx, "move"));
                                }
                            }
                            if kit::menu_item(ui, icons::X, "Tirar da lista", p) {
                                list_op = Some((key.clone(), idx, "remove"));
                            }
                        });
                    });
                }
                if entries.is_empty() {
                    ui.label(RichText::new("Ninguém ainda.").size(12.5).color(p.faint));
                }
                ui.add_space(4.0);
                if kit::quiet(ui, Some(icons::PLUS), "Adicionar", p).clicked() {
                    acts.push(Action::Pick(Box::new(person_picker(cx, &self.path, key))));
                }
            });
            ui.add_space(14.0);
        }
        if let Some((key, idx, what)) = list_op {
            match what {
                "remove" => {
                    let mut l = self.sheet.list(&key);
                    if idx < l.len() {
                        l.remove(idx);
                    }
                    self.sheet.fm.insert(key, Json::Array(l));
                }
                _ => {
                    if let Some((from, to)) = key.split_once('\u{1}') {
                        let mut a = self.sheet.list(from);
                        if idx < a.len() {
                            let it = a.remove(idx);
                            let mut b = self.sheet.list(to);
                            b.push(it);
                            self.sheet.fm.insert(from.to_string(), Json::Array(a));
                            self.sheet.fm.insert(to.to_string(), Json::Array(b));
                        }
                    }
                }
            }
            self.changed();
        }

        // new section or list
        match &mut self.new_section {
            Some((title, is_list)) => {
                let mut done = None;
                card(ui, p, |ui| {
                    ui.label(RichText::new(if *is_list { "Nova lista de pessoas" } else { "Nova seção" }).font(kit::medium(13.0)).color(p.text));
                    ui.add_space(6.0);
                    let r = ui.add(egui::TextEdit::singleline(title).hint_text(if *is_list { "Ex.: família, rivais, mentores" } else { "Ex.: Poderes, Segredos, Voz, Medos" }).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 5)));
                    r.request_focus();
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if kit::primary(ui, "Adicionar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            done = Some(true);
                        }
                        if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            done = Some(false);
                        }
                    });
                });
                if let Some(ok) = done {
                    let (t, is_list) = self.new_section.take().unwrap();
                    let t = t.trim().to_string();
                    if ok && !t.is_empty() {
                        if is_list {
                            let k = t.to_lowercase();
                            if !self.sheet.fm.contains_key(&k) {
                                let mut l = self.sheet.lists();
                                l.push(k.clone());
                                self.sheet.fm.insert(k, json!([]));
                                self.sheet.set_lists(l);
                            }
                        } else {
                            self.sheet.sections.push((t, String::new()));
                            self.editing = Some(self.sheet.sections.len() - 1);
                        }
                        self.changed();
                    }
                }
            }
            None => {
                ui.horizontal(|ui| {
                    if kit::button(ui, Some(icons::PLUS), "Seção", p).clicked() {
                        self.new_section = Some((String::new(), false));
                    }
                    if kit::button(ui, Some(icons::USERS), "Lista de pessoas", p).clicked() {
                        self.new_section = Some((String::new(), true));
                    }
                });
            }
        }
    }

    fn section_card(&mut self, ui: &mut egui::Ui, cx: &mut Cx, j: usize, acts: &mut Vec<Action>) -> Option<&'static str> {
        let p = cx.palette;
        let mut op = None;
        let (title, text) = self.sheet.sections[j].clone();
        let editing = self.editing == Some(j) || text.trim().is_empty();
        card(ui, p, |ui| {
            ui.horizontal(|ui| {
                ui.label(kit::icon(icon_for(section_icon(&title)), 13.0).color(p.faint));
                ui.label(RichText::new(&title).font(kit::medium(13.0)).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let mr = kit::icon_button_sized(ui, icons::ELLIPSIS_VERTICAL, "Opções", false, p, 22.0, 12.0);
                    let popup = ui.make_persistent_id(("sec-menu", j));
                    if mr.clicked() {
                        ui.memory_mut(|m| m.toggle_popup(popup));
                    }
                    egui::popup_below_widget(ui, popup, &mr, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                        ui.set_min_width(150.0);
                        if kit::menu_item(ui, icons::ARROW_UP, "Mover para cima", p) {
                            op = Some("up");
                        }
                        if kit::menu_item(ui, icons::ARROW_DOWN, "Mover para baixo", p) {
                            op = Some("down");
                        }
                        if kit::menu_danger(ui, icons::TRASH_2, "Remover seção", p) {
                            op = Some("remove");
                        }
                    });
                    if !editing && kit::icon_button_sized(ui, icons::PENCIL, "Editar", false, p, 22.0, 12.0).clicked() {
                        self.editing = Some(j);
                    }
                });
            });
            ui.add_space(4.0);
            if editing {
                let mut t = text.clone();
                let id = egui::Id::new(("sec-edit", &self.path, j));
                let r = ui.add(egui::TextEdit::multiline(&mut t).id(id).frame(false).desired_rows(3).desired_width(f32::INFINITY).font(egui::FontId::proportional(14.0)).hint_text(RichText::new("Escreva…").color(p.faint)));
                if self.editing == Some(j) && !r.has_focus() && r.ctx.memory(|m| m.focused().is_none()) {
                    r.request_focus();
                }
                if crate::editor::auto_dash(ui, id, &mut t) || r.changed() {
                    self.sheet.sections[j].1 = t;
                    self.changed();
                }
                if (r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Escape))) && self.editing == Some(j) {
                    self.editing = None;
                }
            } else {
                let doc = markdown::parse(&text);
                let resp = ui.scope(|ui| {
                    let size = 14.0;
                    let mut h = RenderHost { vault: cx.vault, palette: p, size, source: &self.path, caches: cx.caches, settings: cx.dv };
                    let mut r = Renderer::new(&mut h);
                    r.blocks(ui, &doc.blocks);
                    std::mem::take(&mut r.actions)
                });
                let had_actions = !resp.inner.is_empty();
                for a in resp.inner {
                    if let markdown::Action::Open(t, nt) = a {
                        acts.push(Action::OpenLink(t, self.path.clone(), nt));
                    }
                }
                let r = ui.interact(resp.response.rect, egui::Id::new(("sec-click", &self.path, j)), Sense::click());
                if r.clicked() && !had_actions {
                    self.editing = Some(j);
                }
                if r.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                }
            }
        });
        op
    }
}

/// Pictures of the vault, newest first.
pub fn image_picker(cx: &Cx, title: &str, purpose: Purpose) -> Picker {
    let ix = cx.vault.index.read().unwrap();
    let _ = &ix;
    let mut items: Vec<(String, String, String, u64)> = cx
        .vault
        .files
        .iter()
        .filter(|f| ateliet_core::scriptorium::manuscript::is_image_link(&f.path))
        .map(|f| {
            let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
            let folder = f.path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
            (name, f.path.clone(), folder, f.mtime)
        })
        .collect();
    items.sort_by(|a, b| b.3.cmp(&a.3));
    Picker::new(title, items.into_iter().map(|(a, b, c, _)| (a, b, c)).collect(), purpose, icons::IMAGE)
}

/// Characters of the sheet's book, then a free name.
pub fn person_picker(cx: &Cx, sheet: &str, list: &str) -> Picker {
    let ix = cx.vault.index.read().unwrap();
    let author = cx.sc.author();
    let chars: Vec<String> = match ateliet_core::scriptorium::book::book_for(&ix, sheet, &author) {
        Some(b) => ateliet_core::scriptorium::book::characters(&ix, &b),
        None => ix.page_paths().filter(|p| ix.page(p).is_some_and(|pg| ateliet_core::scriptorium::book::kind(pg) == "personagem")).cloned().collect(),
    };
    let items = chars
        .into_iter()
        .filter(|c| c != sheet)
        .map(|c| {
            let name = c.rsplit('/').next().unwrap_or(&c).trim_end_matches(".md").to_string();
            let role = ix.page(&c).and_then(|pg| pg.yaml.get("papel").and_then(|v| v.as_str()).map(str::to_string)).unwrap_or_default();
            (name.clone(), format!("[[{name}]]"), role)
        })
        .collect();
    let mut p = Picker::new("Personagem, facção ou grupo", items, Purpose::Person(sheet.to_string(), list.to_string()), icons::USER_ROUND);
    p.free = true;
    p
}
