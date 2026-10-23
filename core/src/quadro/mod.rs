//! Quadro: Kanban columns, loose cards and Excalidraw drawings on one
//! infinite canvas. Three file formats open in it and are written back in
//! the same format:
//!
//! - `kanban`     — a note with `kanban-plugin: board` (obsidian-kanban):
//!                  `##` columns, `- [ ]` cards, `**Complete**`, the archive
//!                  and `%% kanban:settings %%`; what only the Quadro has
//!                  (drawings, loose cards, where the columns are) goes in a
//!                  `%% ateliet:quadro %%` comment the plugin skips.
//! - `excalidraw` — `.excalidraw.md` (obsidian-excalidraw-plugin, with the
//!                  `## Drawing` json or LZString `compressed-json`) and plain
//!                  `.excalidraw` JSON; unknown fields are kept.
//! - `element` · `rough` · `card` · `lz` · `export` — the drawing model, the
//!                  hand-drawn geometry, card text, LZString, PNG export.

pub mod card;
pub mod element;
pub mod excalidraw;
pub mod export;
pub mod kanban;
pub mod layout;
pub mod lz;
pub mod rough;

use element::Element;
use serde_json::{Map, Value};
use std::path::Path;

/// Width of a column, and the space between columns laid out in a row.
pub const LANE_W: f32 = 272.0;
pub const LANE_GAP: f32 = 24.0;
/// Width of a loose card.
pub const CARD_W: f32 = 240.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Kanban,
    ExcalidrawMd,
    Excalidraw,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    /// The block id for a column's card (`^id`, kept by the plugin), or the
    /// loose card's own id.
    pub id: String,
    pub text: String,
    /// The task mark: ' ' open, 'x' done (or a custom status).
    pub check: char,
    /// The `^id` written after the card's first line, when it has one.
    pub block: Option<String>,
    /// Loose cards: where they are and their width; colour of the edge.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub color: Option<String>,
}

impl Card {
    pub fn new(text: &str) -> Card {
        Card { id: element::new_id(), text: text.to_string(), check: ' ', block: None, x: 0.0, y: 0.0, w: CARD_W, color: None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub id: String,
    pub title: String,
    /// The plugin's card limit, written as "Title (5)"; 0 for none.
    pub max: u32,
    /// Cards here are marked done (`**Complete**` under the title), and
    /// the marker as the file had it (it is translated by the plugin).
    pub complete: Option<String>,
    pub collapsed: bool,
    pub cards: Vec<Card>,
    pub x: f32,
    pub y: f32,
    pub w: f32,
}

impl Lane {
    pub fn new(title: &str) -> Lane {
        Lane { id: element::new_id(), title: title.to_string(), max: 0, complete: None, collapsed: false, cards: Vec::new(), x: 0.0, y: 0.0, w: LANE_W }
    }
}

/// A picture an image element points to.
#[derive(Clone, Debug, PartialEq)]
pub enum FileRef {
    /// A file of the vault (`[[path]]` in `## Embedded Files`, or the path).
    Vault(String),
    /// A file object with a data URL (`files` of a plain `.excalidraw`).
    Data(Value),
    /// Anything else (equations, web links…), written back as it was.
    Raw(String),
}

impl FileRef {
    /// The vault file it names (`[[path]]`, with or without a colour map).
    pub fn vault_path(&self) -> Option<String> {
        match self {
            FileRef::Vault(p) => Some(p.clone()),
            FileRef::Raw(r) => {
                let s = r.find("[[")?;
                let e = r[s..].find("]]")?;
                Some(r[s + 2..s + e].split('|').next().unwrap_or("").to_string())
            }
            FileRef::Data(_) => None,
        }
    }
}

/// What a format keeps that the canvas does not show, to write it back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kept {
    /// Kanban: the frontmatter block as written (`---` … `---\n`).
    pub frontmatter: String,
    /// Kanban: the settings (`%% kanban:settings %%`).
    pub settings: Map<String, Value>,
    /// Kanban: how continuation lines of a card are indented.
    pub indent: String,
    /// Kanban: the archive's heading, and whether the file had one.
    pub archive_title: String,
    /// Excalidraw: the scene object without `elements` and `files`.
    pub scene: Map<String, Value>,
    /// Excalidraw .md: everything before `# Excalidraw Data`.
    pub head: String,
    /// Excalidraw .md: the text elements section sits inside the comment.
    pub commented: bool,
    pub compressed: bool,
    /// Excalidraw .md: the plugin's `^_dummy!_` line.
    pub dummy: bool,
    /// Excalidraw .md: `## Element Links` (id, link), in order.
    pub links: Vec<(String, String)>,
    /// Excalidraw .md: the order of `## Text Elements`.
    pub text_order: Vec<String>,
    /// Excalidraw: what follows the drawing block (or the JSON).
    pub tail: String,
}

#[derive(Clone, Debug)]
pub struct Board {
    pub format: Format,
    pub lanes: Vec<Lane>,
    pub archive: Vec<Card>,
    /// Cards outside the columns.
    pub cards: Vec<Card>,
    pub elements: Vec<Element>,
    /// Pictures of image elements, by file id, in order.
    pub files: Vec<(String, FileRef)>,
    pub kept: Kept,
}

impl Board {
    pub fn empty(format: Format) -> Board {
        Board { format, lanes: Vec::new(), archive: Vec::new(), cards: Vec::new(), elements: Vec::new(), files: Vec::new(), kept: Kept { indent: "\t".into(), ..Default::default() } }
    }

    /// Read a file's text; the format comes from its name and frontmatter.
    pub fn parse(name: &str, text: &str) -> Result<Board, String> {
        match detect(name, text) {
            Some(Format::Kanban) => kanban::parse(text),
            Some(Format::ExcalidrawMd) => excalidraw::parse_md(text),
            Some(Format::Excalidraw) => excalidraw::parse_json(text),
            None => Err("não é um quadro".into()),
        }
    }

    pub fn write(&mut self) -> String {
        for e in self.elements.iter_mut() {
            e.stamp();
        }
        match self.format {
            Format::Kanban => kanban::write(self),
            Format::ExcalidrawMd => excalidraw::write_md(self),
            Format::Excalidraw => excalidraw::write_json(self),
        }
    }

    pub fn file(&self, id: &str) -> Option<&FileRef> {
        self.files.iter().find(|(k, _)| k == id).map(|(_, f)| f)
    }

    pub fn element(&self, id: &str) -> Option<&Element> {
        self.elements.iter().find(|e| e.id() == id)
    }

    pub fn element_mut(&mut self, id: &str) -> Option<&mut Element> {
        self.elements.iter_mut().find(|e| e.id() == id)
    }

    /// A card anywhere (in a column or loose), by id.
    pub fn card(&self, id: &str) -> Option<&Card> {
        self.cards.iter().chain(self.lanes.iter().flat_map(|l| l.cards.iter())).find(|c| c.id == id)
    }

    pub fn card_mut(&mut self, id: &str) -> Option<&mut Card> {
        if let Some(c) = self.cards.iter_mut().find(|c| c.id == id) {
            return Some(c);
        }
        self.lanes.iter_mut().flat_map(|l| l.cards.iter_mut()).find(|c| c.id == id)
    }

    /// Ids that arrows are bound to (start or end).
    pub fn bound_ids(&self) -> std::collections::HashSet<String> {
        self.elements.iter().filter(|e| !e.deleted()).flat_map(|e| [e.binding(false), e.binding(true)]).flatten().collect()
    }

    /// The columns laid out in a row, as a board without positions shows them.
    pub fn default_layout(&mut self) {
        for (i, l) in self.lanes.iter_mut().enumerate() {
            l.x = i as f32 * (LANE_W + LANE_GAP);
            l.y = 0.0;
            l.w = LANE_W;
        }
    }

    pub fn is_default_layout(&self) -> bool {
        self.lanes.iter().enumerate().all(|(i, l)| l.x == i as f32 * (LANE_W + LANE_GAP) && l.y == 0.0 && l.w == LANE_W)
    }

    /// The order of the columns in the file follows where they are: left to
    /// right (then top to bottom), so moving one on the canvas reorders it.
    pub fn sort_lanes(&mut self) {
        self.lanes.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal).then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal)));
    }
}

/// Which kind of quadro a file is, if any.
pub fn detect(name: &str, text: &str) -> Option<Format> {
    let lower = name.to_lowercase();
    if lower.ends_with(".excalidraw") || lower.ends_with(".excalidraw.json") {
        return Some(Format::Excalidraw);
    }
    if !lower.ends_with(".md") {
        return None;
    }
    let fm = frontmatter(text).unwrap_or("");
    let has = |key: &str| fm.lines().any(|l| l.trim_start().strip_prefix(key).is_some_and(|r| r.trim_start().starts_with(':')));
    if has("kanban-plugin") {
        return Some(Format::Kanban);
    }
    if lower.ends_with(".excalidraw.md") || has("excalidraw-plugin") {
        return Some(Format::ExcalidrawMd);
    }
    None
}

/// The YAML between the `---` fences at the top.
pub fn frontmatter(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---").map(|i| i + 1).or_else(|| rest.starts_with("---").then_some(0))?;
    Some(&rest[..end])
}

/// Is this file a quadro? Reads only the start of a `.md`.
pub fn is_quadro_file(abs: &Path) -> bool {
    let name = abs.to_string_lossy();
    let lower = name.to_lowercase();
    if lower.ends_with(".excalidraw") {
        return true;
    }
    if !lower.ends_with(".md") {
        return false;
    }
    if lower.ends_with(".excalidraw.md") {
        return true;
    }
    use std::io::Read;
    let mut head = vec![0u8; 4096];
    let n = std::fs::File::open(abs).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let text = String::from_utf8_lossy(&head[..n]);
    detect(&name, &text).is_some()
}

/// Write atomically (a temporary file beside it, then a rename).
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("ateliet-tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// A new quadro note: Kanban columns the plugin reads, nothing drawn yet.
pub fn new_board_text(columns: &[&str]) -> String {
    let mut b = Board::empty(Format::Kanban);
    b.kept.frontmatter = "---\n\nkanban-plugin: board\n\n---\n".into();
    b.kept.settings.insert("kanban-plugin".into(), Value::String("board".into()));
    for c in columns {
        b.lanes.push(Lane::new(c));
    }
    b.default_layout();
    b.write()
}
