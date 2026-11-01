//! Ateliet: the same vault, notes and built-in tools as Obsidian, drawn with
//! egui, with a separate native video companion on macOS (no GTK).

mod align;
mod app;
mod find;
mod board_ui;
mod commands;
mod config;
mod daily;
mod dv;
mod editor;
mod fonts;
mod form;
mod host;
mod icons;
mod kit;
mod images_ui;
mod journal_mode;
mod journal_ui;
mod live;
mod markdown;
mod media_ui;
mod music;
mod panels;
mod quadro_ui;
mod reader_mode;
mod reader_ui;
mod sc;
mod settings_pages;
mod templater;
mod theme;
mod typo;
mod update;
mod vault;
#[cfg(feature = "clip")]
mod vision;
mod writing;
mod work_video;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    // `ateliet <vault>` opens that vault; otherwise the last one
    let arg = std::env::args().nth(1).map(PathBuf::from).filter(|p| p.is_dir()).map(|p| std::fs::canonicalize(&p).unwrap_or(p));
    let vault = arg.or_else(config::last_vault);
    let mut viewport = eframe::egui::ViewportBuilder::default().with_title("Ateliet").with_inner_size([1280.0, 820.0]).with_min_inner_size([640.0, 400.0]).with_app_id("ateliet");
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../icons/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native("Ateliet", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, vault)))))
}
