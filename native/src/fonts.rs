//! Fonts: Inter for the interface and the notes (bundled, so the app looks
//! the same everywhere), Lucide for icons, the system's monospace for code.
//! The system sans and egui's built-in fonts stay as fallbacks for scripts
//! Inter does not cover (and for emoji).
//!
//! Families: `Proportional` (Inter Regular), `"medium"`, `"bold"` (Inter
//! SemiBold), `"italic"`, `"icons"` (Lucide; see `icons.rs`).

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
const INTER_ITALIC: &[u8] = include_bytes!("../assets/fonts/Inter-Italic.ttf");
const LUCIDE: &[u8] = include_bytes!("../assets/fonts/Lucide.ttf");

const SYSTEM_SANS: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];
const MONO: &[&str] = &[
    "/System/Library/Fonts/SFNSMono.ttf",
    "/System/Library/Fonts/Menlo.ttc",
    "C:\\Windows\\Fonts\\consola.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
];

fn first(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let builtin: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let builtin_mono: Vec<String> = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let mut put = |name: &str, data: FontData| {
        fonts.font_data.insert(name.to_string(), Arc::new(data));
    };
    put("inter", FontData::from_static(INTER_REGULAR));
    put("inter-medium", FontData::from_static(INTER_MEDIUM));
    put("inter-semibold", FontData::from_static(INTER_SEMIBOLD));
    put("inter-italic", FontData::from_static(INTER_ITALIC));
    put("lucide", FontData::from_static(LUCIDE));
    let sys = first(SYSTEM_SANS).map(|d| put("sys-sans", FontData::from_owned(d))).is_some();
    let mono = first(MONO).map(|d| put("sys-mono", FontData::from_owned(d))).is_some();

    // each family: its Inter, then the icons (so an icon can sit in a label),
    // then the system sans and egui's fonts for everything else
    let chain = |main: &str| -> Vec<String> {
        let mut v = vec![main.to_string(), "lucide".to_string()];
        if sys {
            v.push("sys-sans".into());
        }
        v.extend(builtin.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, chain("inter"));
    fonts.families.insert(FontFamily::Name("medium".into()), chain("inter-medium"));
    fonts.families.insert(FontFamily::Name("bold".into()), chain("inter-semibold"));
    fonts.families.insert(FontFamily::Name("italic".into()), chain("inter-italic"));
    fonts.families.insert(FontFamily::Name("icons".into()), vec!["lucide".into(), "inter".into()]);
    let mut m: Vec<String> = if mono { vec!["sys-mono".into()] } else { Vec::new() };
    m.extend(builtin_mono);
    m.push("inter".into());
    fonts.families.insert(FontFamily::Monospace, m);
    ctx.set_fonts(fonts);
}
