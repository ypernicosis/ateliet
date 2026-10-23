//! Fonts: Inter for the interface and the notes (bundled, so the app looks
//! the same everywhere), Lucide for icons, the system's monospace for code.
//! The system sans and egui's built-in fonts stay as fallbacks for scripts
//! Inter does not cover (and for emoji).
//!
//! Families: `Proportional` (Inter Regular), `"medium"`, `"bold"` (Inter
//! SemiBold), `"italic"`, `"icons"` (Lucide; see `icons.rs`); the serifs
//! `"cormorant"`, `"cormorant-italic"`, `"cormorant-bold"` (map labels) and
//! `"garamond"`, `"garamond-italic"` (typeset pages).

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
const INTER_ITALIC: &[u8] = include_bytes!("../assets/fonts/Inter-Italic.ttf");
const LUCIDE: &[u8] = include_bytes!("../assets/fonts/Lucide.ttf");
pub const CORMORANT: &[u8] = include_bytes!("../assets/fonts/CormorantGaramond-Medium.ttf");
pub const CORMORANT_ITALIC: &[u8] = include_bytes!("../assets/fonts/CormorantGaramond-MediumItalic.ttf");
pub const CORMORANT_BOLD: &[u8] = include_bytes!("../assets/fonts/CormorantGaramond-Bold.ttf");
pub const GARAMOND: &[u8] = include_bytes!("../assets/fonts/EBGaramond-Medium.ttf");
pub const GARAMOND_ITALIC: &[u8] = include_bytes!("../assets/fonts/EBGaramond-MediumItalic.ttf");
pub const GARAMOND_BOLD: &[u8] = include_bytes!("../assets/fonts/EBGaramond-Bold.ttf");
pub const GARAMOND_BOLD_ITALIC: &[u8] = include_bytes!("../assets/fonts/EBGaramond-BoldItalic.ttf");
/// The faces of the ready-made book styles (Diagramação), only for typesetting.
const BOOK_SERIFS: [&[u8]; 14] = [
    include_bytes!("../assets/fonts/Lora-Regular.ttf"),
    include_bytes!("../assets/fonts/Lora-Italic.ttf"),
    include_bytes!("../assets/fonts/Lora-Bold.ttf"),
    include_bytes!("../assets/fonts/CrimsonText-Regular.ttf"),
    include_bytes!("../assets/fonts/CrimsonText-Italic.ttf"),
    include_bytes!("../assets/fonts/CrimsonText-Bold.ttf"),
    include_bytes!("../assets/fonts/Cinzel-Regular.ttf"),
    include_bytes!("../assets/fonts/Cinzel-Bold.ttf"),
    include_bytes!("../assets/fonts/CinzelDecorative-Regular.ttf"),
    include_bytes!("../assets/fonts/PlayfairDisplay-Regular.ttf"),
    include_bytes!("../assets/fonts/PlayfairDisplay-Italic.ttf"),
    include_bytes!("../assets/fonts/LibreBaskerville-Regular.ttf"),
    include_bytes!("../assets/fonts/LibreBaskerville-Italic.ttf"),
    include_bytes!("../assets/fonts/LibreBaskerville-Bold.ttf"),
];

/// The fonts for typesetting books: the bundled serifs and the system's
/// (read once, in the background).
pub fn book_fonts() -> std::sync::Arc<ateliet_core::typeset::fonts::FontDb> {
    static DB: std::sync::OnceLock<std::sync::Arc<ateliet_core::typeset::fonts::FontDb>> = std::sync::OnceLock::new();
    DB.get_or_init(|| {
        let mut db = ateliet_core::typeset::fonts::FontDb::new(&[&[GARAMOND, GARAMOND_ITALIC, GARAMOND_BOLD, GARAMOND_BOLD_ITALIC, CORMORANT, CORMORANT_ITALIC, CORMORANT_BOLD, INTER_REGULAR, INTER_SEMIBOLD][..], &BOOK_SERIFS[..]].concat());
        db.scan_system();
        std::sync::Arc::new(db)
    })
    .clone()
}

/// Start reading the system's fonts without waiting.
pub fn warm_book_fonts() {
    std::thread::spawn(|| {
        let _ = book_fonts();
    });
}

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
    ctx.set_fonts(definitions());
}

/// The app's font families (see the module's notes).
pub fn definitions() -> FontDefinitions {
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
    put("cormorant", FontData::from_static(CORMORANT));
    put("cormorant-italic", FontData::from_static(CORMORANT_ITALIC));
    put("cormorant-bold", FontData::from_static(CORMORANT_BOLD));
    put("garamond", FontData::from_static(GARAMOND));
    put("garamond-italic", FontData::from_static(GARAMOND_ITALIC));
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
    for f in ["cormorant", "cormorant-italic", "cormorant-bold", "garamond", "garamond-italic"] {
        fonts.families.insert(FontFamily::Name(f.into()), chain(f));
    }
    fonts.families.insert(FontFamily::Name("icons".into()), vec!["lucide".into(), "inter".into()]);
    let mut m: Vec<String> = if mono { vec!["sys-mono".into()] } else { Vec::new() };
    m.extend(builtin_mono);
    m.push("inter".into());
    fonts.families.insert(FontFamily::Monospace, m);
    fonts
}
