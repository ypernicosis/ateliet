//! Storyboards: the `.storyboard` file (a zip of `board.json`, 512 px PNG
//! tiles, kept pictures and thumbnails), brushes, compositing and tracing —
//! the drawing engine behind the native storyboard editor, compatible with
//! the files the web editor writes.

pub mod brush;
pub mod compose;
pub mod doc;
pub mod pages;
pub mod paint;
pub mod tile;
pub mod trace;
pub mod zip;
