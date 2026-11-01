//! The pictures in the side panel, driven through the whole window.

use eframe::egui;
use eframe::App as _;

const NOTE: &str = "# Nota\n\nPrimeiro parágrafo.\n\nSegundo parágrafo.\n\nTerceiro parágrafo.\n";

struct W {
    app: crate::app::App,
    ctx: egui::Context,
    root: std::path::PathBuf,
    t: f64,
}

impl W {
    fn new(name: &str) -> W {
        let ctx = egui::Context::default();
        let mut app = crate::app::App::new(&eframe::CreationContext::_new_kittest(ctx.clone()), None);
        let root = std::env::temp_dir().join(format!("ateliet-side-pictures-{}-{name}", std::process::id()));
        std::fs::create_dir_all(root.join("img")).unwrap();
        std::fs::write(root.join("Nota.md"), NOTE).unwrap();
        for (i, c) in [[200, 30, 30], [30, 200, 30], [30, 30, 200], [200, 200, 30]].iter().enumerate() {
            image::RgbaImage::from_pixel(64, 48, image::Rgba([c[0], c[1], c[2], 255])).save(root.join(format!("img/{i}.png"))).unwrap();
        }
        crate::images_ui::ImagesState::save_settings(&root, serde_json::json!({"shared":"", "show_vault":true}));
        app.vault = Some(crate::vault::Vault::open(&root, ctx.clone()));
        app.sc = Some(crate::sc::Sc::load(&root.join(".obsidian")));
        app.open("Nota.md", true);
        app.tabs[0].mode = crate::app::Mode::Edit;
        app.right_open = true;
        app.right_tab = crate::app::RightTab::Pictures;
        let mut w = W { app, ctx, root, t: 1.0 };
        for _ in 0..60 {
            w.frame(vec![]);
            if w.app.images.entries.len() == 4 && w.app.images.scanning.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        for _ in 0..4 {
            w.frame(vec![]);
        }
        w
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        self.t += 0.05;
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));
        let _ = self.ctx.run(egui::RawInput { screen_rect: Some(bounds), time: Some(self.t), events, ..Default::default() }, |ctx| self.app.update(ctx, &mut eframe::Frame::_new_kittest()));
    }

    fn press(&mut self, pos: egui::Pos2, down: bool) {
        self.frame(vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: down, modifiers: Default::default() }]);
    }

    fn click(&mut self, pos: egui::Pos2) {
        self.frame(vec![egui::Event::PointerMoved(pos)]);
        self.press(pos, true);
        self.press(pos, false);
        self.frame(vec![]);
    }

    /// The first square of the grid: the panel's right part, below the search.
    fn first_square(&mut self) -> egui::Pos2 {
        for y in (60..260).step_by(8) {
            let q = egui::pos2(1280.0 - 250.0, y as f32);
            self.click(q);
            if self.app.images.side_view.is_some() {
                self.app.images.side_view = None;
                self.frame(vec![]);
                return q;
            }
        }
        panic!("no picture square found in the side panel");
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_click_opens_the_picture_in_the_panel_and_leaves_the_note_alone() {
    let mut w = W::new("click");
    let q = w.first_square();
    w.click(q);
    let key = w.app.images.side_view.clone().expect("the picture opens in the panel");
    assert_eq!(w.app.tabs[0].text, NOTE, "a click must not touch the note");
    // → over the panel goes to the next one
    w.frame(vec![egui::Event::Key { key: egui::Key::ArrowRight, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]);
    w.frame(vec![]);
    assert_ne!(w.app.images.side_view.as_ref(), Some(&key));
    w.frame(vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]);
    w.frame(vec![]);
    assert!(w.app.images.side_view.is_none());
}

#[test]
fn a_picture_dragged_onto_the_note_lands_where_it_is_dropped() {
    let mut w = W::new("drag");
    let q = w.first_square();
    let key = w.app.images.entries[w.app.images.shown[0]].key.clone();
    let name = key.rsplit('/').next().unwrap().to_string();
    // the block of the second paragraph, as drawn
    let (top, bottom, start, _) = *w.app.tabs[0].live.spots.iter().find(|s| s.2 == 4).expect("the second paragraph drawn");
    assert_eq!(start, 4);
    let target = egui::pos2(500.0, top + (bottom - top) * 0.25);
    w.frame(vec![egui::Event::PointerMoved(q)]);
    w.press(q, true);
    let mut p = q;
    for _ in 0..12 {
        p += (target - q) / 12.0;
        w.frame(vec![egui::Event::PointerMoved(p)]);
    }
    w.press(target, false);
    w.frame(vec![]);
    let text = &w.app.tabs[0].text;
    let embed = format!("![[{name}]]\n");
    assert!(text.contains(&embed), "the picture goes into the note: {text}");
    let at = text.find(&embed).unwrap();
    assert!(text[..at].contains("Primeiro") && !text[..at].contains("Segundo"), "before the second paragraph: {text}");
}

#[test]
fn the_panel_shuffles_like_image_mode() {
    let mut w = W::new("shuffle");
    w.app.images.set_order(crate::images_ui::Order::Newest);
    w.frame(vec![]);
    // the shuffle button: right of the order button, at the top right of the panel
    let mut shuffled = false;
    'scan: for y in (40..100).step_by(6) {
        for x in (1220..1278).step_by(4) {
            w.click(egui::pos2(x as f32, y as f32));
            w.app.right_tab = crate::app::RightTab::Pictures;
            if matches!(w.app.images.order, crate::images_ui::Order::Shuffled(_)) {
                shuffled = true;
                break 'scan;
            }
        }
    }
    assert!(shuffled, "a shuffle button in the panel");
}
