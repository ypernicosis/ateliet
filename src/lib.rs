//! Ateliet core: everything that does not depend on the window system.
//!
//! - `text`     — line index with UTF-16 positions (what Obsidian plugins expect)
//! - `metadata` — markdown → `CachedMetadata` (links, tags, headings, sections …)
//! - `render`   — markdown → HTML blocks for the reading view
//! - `vault`    — file listing / stat helpers
//! - `search`   — full text search over the vault
//! - `images`   — the image library: scan, index, vector and name search
//! - `clip`     — CLIP tokenizer, preprocessing and ONNX inference

pub mod metadata;
pub mod render;
pub mod search;
pub mod syntax;
pub mod text;
pub mod vault;
pub mod clip;
pub mod images;
