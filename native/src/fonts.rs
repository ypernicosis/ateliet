//! Fonts: the system's sans-serif in regular, bold and italic, so headings
//! and emphasis look right; egui's built-in fonts stay as fallbacks (they
//! also carry the emoji).

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

const REGULAR: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];
const BOLD: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/Library/Fonts/Arial Bold.ttf",
    "C:\\Windows\\Fonts\\segoeuib.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
];
const ITALIC: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Italic.ttf",
    "C:\\Windows\\Fonts\\segoeuii.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Italic.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Oblique.ttf",
];
const MONO: &[&str] = &[
    "/System/Library/Fonts/Menlo.ttc",
    "/System/Library/Fonts/SFNSMono.ttf",
    "C:\\Windows\\Fonts\\consola.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
];

fn first(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, data: Option<Vec<u8>>| -> bool {
        match data {
            Some(d) => {
                fonts.font_data.insert(name.to_string(), Arc::new(FontData::from_owned(d)));
                true
            }
            None => false,
        }
    };
    let has_regular = add("sys-regular", first(REGULAR));
    let has_bold = add("sys-bold", first(BOLD));
    let has_italic = add("sys-italic", first(ITALIC));
    let has_mono = add("sys-mono", first(MONO));
    let fallbacks: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if has_regular {
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "sys-regular".into());
    }
    if has_mono {
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "sys-mono".into());
    }
    let family = |main: Option<&str>| -> Vec<String> {
        let mut v: Vec<String> = main.map(|m| vec![m.to_string()]).unwrap_or_default();
        if has_regular {
            v.push("sys-regular".into());
        }
        v.extend(fallbacks.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Name("bold".into()), family(has_bold.then_some("sys-bold")));
    fonts.families.insert(FontFamily::Name("italic".into()), family(has_italic.then_some("sys-italic")));
    ctx.set_fonts(fonts);
}
