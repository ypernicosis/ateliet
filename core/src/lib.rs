//! Ateliet core: everything that does not depend on the window system.
//!
//! - `text`     — line index with UTF-16 positions (what Obsidian plugins expect)
//! - `metadata` — markdown → `CachedMetadata` (links, tags, headings, sections …)
//! - `render`   — markdown → HTML blocks for the reading view
//! - `vault`    — file listing / stat helpers
//! - `search`   — full text search over the vault
//! - `images`   — the image library: scan, index, vector and name search
//! - `clip`     — CLIP tokenizer, preprocessing and ONNX inference
//! - `clipstudio` — Clip Studio Paint `.clip` files: layers as pictures
//! - `datefmt`  — moment/Luxon date formats in English and Portuguese
//! - `dataview` — the Dataview index, query language and functions
//! - `script`   — QuickJS for `dataviewjs` blocks and Templater commands
//! - `calendar` — a month of daily notes for the calendar view
//! - `board`    — storyboards: file format, brushes, compositing, tracing

pub mod metadata;
pub mod render;
pub mod search;
pub mod syntax;
pub mod text;
pub mod vault;
#[cfg(feature = "vision")]
pub mod clip;
pub mod images;
pub mod clipstudio;
pub mod datefmt;
pub mod dataview;
pub mod script;
pub mod calendar;
pub mod scriptorium;
pub mod board;
