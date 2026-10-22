//! References beside the frame: pictures kept inside the storyboard
//! (`refs/…`), one shown at a time (← and → change it), shuffled, placed in
//! the frame as a picture layer, or traced: a trace layer at the bottom of
//! the frame (photo, grey, lines, values or non-photo blue) and a layer
//! above it to draw on.

use super::*;
use std::collections::HashMap as Map;

pub(super) const TRACE_MODES: [(&str, &str); 5] = [("photo", "Foto"), ("grey", "Tons de cinza"), ("lines", "Linhas"), ("values", "Valores"), ("blue", "Azul de lápis")];

#[derive(Default)]
pub(super) struct RefState {
    tex: Map<String, Option<(egui::TextureHandle, egui::Vec2)>>,
    /// The reference shown when the frame names none.
    pub current: usize,
    /// Q held: the drawing without the trace.
    pub peek: bool,
}

impl BoardEditor {
    pub(super) fn references(&self) -> Vec<Value> {
        self.doc.board["references"].as_array().cloned().unwrap_or_default()
    }

    /// The reference of this frame (or the one chosen in the panel).
    fn shown_ref(&self) -> Option<(usize, Value)> {
        let refs = self.references();
        if refs.is_empty() {
            return None;
        }
        let own = self.doc.frame(self.frame)["reference"].as_str().and_then(|id| refs.iter().position(|r| r["id"].as_str() == Some(id)));
        let i = own.unwrap_or(self.rf.current.min(refs.len() - 1));
        Some((i, refs[i].clone()))
    }

    fn ref_texture(&mut self, ctx: &egui::Context, r: &Value) -> Option<(egui::TextureId, egui::Vec2)> {
        let id = r["id"].as_str().unwrap_or("").to_string();
        if !self.rf.tex.contains_key(&id) {
            let img = self.doc.reference_bytes(r["path"].as_str().unwrap_or("")).and_then(|b| image::load_from_memory(&b).ok()).map(|i| {
                let i = if i.width() > 1200 || i.height() > 1200 { i.thumbnail(1200, 1200) } else { i };
                let rgba = i.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let t = ctx.load_texture(format!("ref:{id}"), egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()), egui::TextureOptions::LINEAR);
                (t, egui::vec2(size[0] as f32, size[1] as f32))
            });
            self.rf.tex.insert(id.clone(), img);
        }
        self.rf.tex.get(&id).and_then(|x| x.as_ref()).map(|(t, s)| (t.id(), *s))
    }

    /// Pictures added to the storyboard (kept inside it).
    fn add_reference_bytes(&mut self, name: &str, ext: &str, bytes: Vec<u8>, from: &str) {
        let before = self.doc.board.clone();
        let path = format!("refs/{}.{ext}", doc::uid());
        self.doc.refs.insert(path.clone(), bytes);
        let r = json!({ "id": doc::uid(), "name": name, "path": path, "from": from });
        if !self.doc.board["references"].is_array() {
            self.doc.board["references"] = json!([]);
        }
        self.doc.board["references"].as_array_mut().unwrap().push(r);
        self.rf.current = self.references().len() - 1;
        let after = self.doc.board.clone();
        self.undo.push(Edit::Structure { before, after, tiles: Vec::new() });
        self.redo.clear();
        self.changed();
    }

    fn add_files(&mut self) {
        let Some(files) = rfd::FileDialog::new().add_filter("Imagens", &["png", "jpg", "jpeg", "webp", "gif"]).pick_files() else { return };
        for f in files {
            if let Ok(b) = std::fs::read(&f) {
                let name = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let ext = f.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_else(|| "png".into());
                self.add_reference_bytes(&name, &ext, b, "arquivo");
            }
        }
    }

    fn add_clipboard(&mut self) {
        let img = arboard::Clipboard::new().and_then(|mut c| c.get_image());
        match img {
            Ok(i) => {
                let png = ateliet_core::board::tile::encode_png(&i.bytes, i.width as u32, i.height as u32);
                self.add_reference_bytes("Colada", "png", png, "área de transferência");
            }
            Err(_) => self.error = Some("Não há imagem na área de transferência.".into()),
        }
    }

    fn remove_reference(&mut self, i: usize) {
        let before = self.doc.board.clone();
        if let Some(a) = self.doc.board["references"].as_array_mut() {
            if i < a.len() {
                a.remove(i);
            }
        }
        let after = self.doc.board.clone();
        self.undo.push(Edit::Structure { before, after, tiles: Vec::new() });
        self.redo.clear();
        self.changed();
    }

    fn shuffle_references(&mut self) {
        let before = self.doc.board.clone();
        if let Some(a) = self.doc.board["references"].as_array_mut() {
            let mut s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7) | 1;
            for i in (1..a.len()).rev() {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                a.swap(i, (s % (i as u64 + 1)) as usize);
            }
        }
        self.rf.current = 0;
        let after = self.doc.board.clone();
        self.undo.push(Edit::Structure { before, after, tiles: Vec::new() });
        self.redo.clear();
        self.changed();
    }

    /// Fit (or cover) a picture of `w`×`h` in the frame.
    fn fit(&self, w: f32, h: f32, cover: bool) -> Value {
        let (bw, bh) = (self.doc.width() as f32, self.doc.height() as f32);
        let k = if cover { (bw / w).max(bh / h) } else { (bw / w).min(bh / h) };
        json!({ "x": bw / 2.0, "y": bh / 2.0, "sx": k, "sy": k, "rotation": 0 })
    }

    /// The reference as a layer of this frame: a picture, or a trace at the bottom.
    fn place(&mut self, r: &Value, trace: bool) {
        let Some(size) = self.doc.reference_bytes(r["path"].as_str().unwrap_or("")).and_then(|b| image::load_from_memory(&b).ok()).map(|i| (i.width() as f32, i.height() as f32)) else {
            self.error = Some("Não consegui ler a imagem.".into());
            return;
        };
        let f = self.frame;
        let src = r["id"].as_str().unwrap_or("").to_string();
        let name = r["name"].as_str().unwrap_or("Imagem").to_string();
        let t = self.fit(size.0, size.1, trace);
        self.structure(|d| {
            if trace {
                let mut l = doc::new_layer(&format!("Decalque · {name}"), "trace");
                l["src"] = json!(src);
                l["transform"] = t;
                l["opacity"] = json!(0.5);
                l["locked"] = json!(true);
                l["trace"] = json!({ "mode": "photo", "above": false, "export": false, "threshold": 0.5, "levels": 3 });
                let id = l["id"].as_str().unwrap_or("").to_string();
                d.board["layers"][&id] = l;
                if let Some(a) = d.board["frames"][f]["layers"].as_array_mut() {
                    a.insert(0, json!(id));
                }
                // a fresh layer above it to draw on
                let draw = d.add_layer(f, "Desenho");
                d.board["frames"][f]["active"] = json!(draw);
            } else {
                let id = d.add_layer(f, &name);
                d.board["layers"][&id]["kind"] = json!("image");
                d.board["layers"][&id]["src"] = json!(src);
                d.board["layers"][&id]["transform"] = t;
            }
            d.mark_frame(f);
        });
        self.pics.clear();
    }

    /// The trace layers of this frame shown or hidden (T), or their mode changed (Shift+T).
    pub(super) fn trace_keys(&mut self, ui: &egui::Ui) {
        let (t, shift_t, q_down) = ui.input(|i| (i.key_pressed(egui::Key::T) && !i.modifiers.shift && !i.modifiers.command, i.key_pressed(egui::Key::T) && i.modifiers.shift, i.key_down(egui::Key::Q)));
        let traces: Vec<String> = self.doc.frame_layers(self.frame).into_iter().filter(|id| self.doc.layer(id)["kind"] == "trace").collect();
        if traces.is_empty() {
            return;
        }
        if t {
            for id in &traces {
                let v = self.doc.layer(id)["visible"].as_bool().unwrap_or(true);
                self.doc.layer_mut(id)["visible"] = json!(!v);
            }
            self.doc.mark_frame(self.frame);
            self.invalidate();
            self.dirty = true;
        }
        if shift_t {
            for id in &traces {
                let cur = self.doc.layer(id)["trace"]["mode"].as_str().unwrap_or("photo").to_string();
                let i = TRACE_MODES.iter().position(|m| m.0 == cur).unwrap_or(0);
                self.doc.layer_mut(id)["trace"]["mode"] = json!(TRACE_MODES[(i + 1) % TRACE_MODES.len()].0);
            }
            self.pics.clear();
            self.doc.mark_frame(self.frame);
            self.invalidate();
            self.dirty = true;
            let m = self.doc.layer(&traces[0])["trace"]["mode"].as_str().unwrap_or("photo").to_string();
            self.error = Some(format!("Decalque: {}", TRACE_MODES.iter().find(|x| x.0 == m).map(|x| x.1).unwrap_or("")));
        }
        // hold Q: the drawing without the trace
        if q_down != self.rf.peek {
            self.rf.peek = q_down;
            for id in &traces {
                self.doc.layer_mut(id)["visible"] = json!(!q_down);
            }
            self.doc.mark_frame(self.frame);
            self.invalidate();
        }
    }

    /// A trace layer's settings (in the layers panel).
    pub(super) fn trace_props(&mut self, ui: &mut egui::Ui, id: &str, p: crate::theme::Palette) -> bool {
        let mut changed = false;
        let t = self.doc.layer(id)["trace"].clone();
        let mode = t["mode"].as_str().unwrap_or("photo").to_string();
        egui::ComboBox::from_id_salt(("trace-mode", id)).width(ui.available_width()).selected_text(TRACE_MODES.iter().find(|m| m.0 == mode).map(|m| m.1).unwrap_or("Foto")).show_ui(ui, |ui| {
            for (k, l) in TRACE_MODES {
                if ui.selectable_label(mode == k, l).clicked() {
                    self.doc.layer_mut(id)["trace"]["mode"] = json!(k);
                    changed = true;
                }
            }
        });
        if mode == "lines" {
            let mut v = t["threshold"].as_f64().unwrap_or(0.5) as f32;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Detalhe").size(12.0).color(p.muted));
                if ui.add(egui::Slider::new(&mut v, 0.05..=0.95).show_value(false)).drag_stopped() {
                    self.doc.layer_mut(id)["trace"]["threshold"] = json!(v);
                    changed = true;
                }
            });
        }
        if mode == "values" {
            let mut n = t["levels"].as_u64().unwrap_or(3) as u32;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Tons").size(12.0).color(p.muted));
                if ui.add(egui::Slider::new(&mut n, 2..=6)).changed() {
                    self.doc.layer_mut(id)["trace"]["levels"] = json!(n);
                    changed = true;
                }
            });
        }
        for (k, label) in [("above", "Por cima (mesa de luz)"), ("export", "Nas exportações")] {
            let mut on = t[k].as_bool().unwrap_or(false);
            ui.horizontal(|ui| {
                if kit::toggle(ui, &mut on, p).changed() {
                    self.doc.layer_mut(id)["trace"][k] = json!(on);
                    changed = true;
                }
                ui.label(RichText::new(label).size(12.0).color(p.muted));
            });
        }
        ui.label(RichText::new("T mostra/esconde · segurar Q mostra sem o decalque · Shift+T troca o modo").size(11.0).color(p.faint));
        if changed {
            self.pics.clear();
        }
        changed
    }

    /// The references panel.
    pub(super) fn refs_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        let refs = self.references();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Referências").font(kit::medium(13.0)).color(p.text));
            if !refs.is_empty() {
                ui.label(RichText::new(refs.len().to_string()).size(12.0).color(p.faint));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let add = kit::icon_button_sized(ui, icons::PLUS, "Adicionar referências", false, p, 24.0, 14.0);
                let id = ui.make_persistent_id("ref-add");
                if add.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(id));
                }
                egui::popup_below_widget(ui, id, &add, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(200.0);
                    if kit::menu_item(ui, icons::FOLDER_CLOSED, "Arquivos do computador…", p) {
                        self.add_files();
                    }
                    if kit::menu_item(ui, icons::COPY, "Da área de transferência", p) {
                        self.add_clipboard();
                    }
                });
                if refs.len() > 1 && kit::icon_button_sized(ui, icons::SHUFFLE, "Embaralhar", false, p, 24.0, 13.0).clicked() {
                    self.shuffle_references();
                }
            });
        });
        let Some((i, r)) = self.shown_ref() else {
            ui.label(RichText::new("Fotos, quadros e estudos ao lado do desenho. Alt+← e Alt+→ trocam a referência.").size(11.5).color(p.faint));
            ui.add_space(8.0);
            kit::hairline(ui, p);
            ui.add_space(8.0);
            return;
        };
        let w = ui.available_width();
        if let Some((tex, size)) = self.ref_texture(ui.ctx(), &r) {
            let h = (w * size.y / size.x).min(260.0);
            let iw = h * size.x / size.y;
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
            let ir = egui::Rect::from_center_size(rect.center(), egui::vec2(iw, h));
            ui.painter().rect_filled(rect, 6.0, p.bg_hover);
            ui.painter().image(tex, ir, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            resp.on_hover_text(r["name"].as_str().unwrap_or(""));
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if kit::icon_button_sized(ui, icons::CHEVRON_LEFT, "Anterior (Alt+←)", false, p, 24.0, 13.0).clicked() {
                self.step_ref(-1);
            }
            ui.label(RichText::new(format!("{} / {}", i + 1, refs.len())).size(11.5).color(p.faint));
            if kit::icon_button_sized(ui, icons::CHEVRON_RIGHT, "Seguinte (Alt+→)", false, p, 24.0, 13.0).clicked() {
                self.step_ref(1);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button_sized(ui, icons::TRASH_2, "Tirar das referências", false, p, 24.0, 13.0).clicked() {
                    self.remove_reference(i);
                }
                if kit::icon_button_sized(ui, icons::IMAGE, "Colocar no quadro (camada de imagem)", false, p, 24.0, 13.0).clicked() {
                    self.place(&r, false);
                }
                if kit::icon_button_sized(ui, icons::SCAN_SEARCH, "Decalcar: a referência no fundo, semitransparente, e uma camada para desenhar por cima", false, p, 24.0, 13.0).clicked() {
                    self.place(&r, true);
                }
            });
        });
        ui.add_space(8.0);
        kit::hairline(ui, p);
        ui.add_space(8.0);
    }

    pub(super) fn step_ref(&mut self, d: i32) {
        let n = self.references().len();
        if n == 0 {
            return;
        }
        let i = self.shown_ref().map(|x| x.0).unwrap_or(0) as i32;
        let j = (i + d).rem_euclid(n as i32) as usize;
        self.rf.current = j;
        // the frame follows the picture chosen for it
        if self.doc.frame(self.frame)["reference"].is_string() {
            let id = self.references()[j]["id"].clone();
            self.doc.board["frames"][self.frame]["reference"] = id;
            self.dirty = true;
        }
    }
}
