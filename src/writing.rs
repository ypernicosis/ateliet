//! Writing in chapters without learning syntax: a "/" menu (scene break,
//! letter, diary, poem, footnote, character link…) and a floating bar over
//! the selection (bold, italic, text styles, blocks, character, footnote).
//! Both act on the editor's text; they are on while a chapter is edited.

use crate::editor::{byte_to_char, char_to_byte};
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::scriptorium::manuscript::{CALLOUT_STYLES, TEXT_STYLES};
use eframe::egui::{self, vec2, RichText};
use std::cell::RefCell;

thread_local! {
    /// The characters of the book of the chapter being edited; None outside chapters.
    pub static CHAPTER: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

pub fn set_chapter(chars: Option<Vec<String>>) {
    CHAPTER.with(|c| *c.borrow_mut() = chars);
}

fn chapter() -> Option<Vec<String>> {
    CHAPTER.with(|c| c.borrow().clone())
}

#[derive(Clone, Copy)]
enum Item {
    Text(&'static str),
    Callout(&'static str, &'static str),
    Style,
    Character,
    Footnote,
}

const SLASH: [(&str, &str, &str, &str, Item); 22] = [
    ("Cena", "Quebra de cena (⁂)", icons::SPARKLE, "quebra scene", Item::Text("\n***\n\n")),
    ("Carta", "Bloco de carta em papel", icons::SEND, "carta", Item::Callout("carta", "Querido…")),
    ("Diário", "Página pautada, letra à mão", icons::NOTEBOOK_PEN, "diario", Item::Callout("diario", "Dia 1")),
    ("Tela / terminal", "Mensagem de máquina", icons::MONITOR, "tela terminal computador", Item::Callout("tela", "TERMINAL")),
    ("Mensagem recebida", "Balão de chat à esquerda", icons::MESSAGE_SQUARE_QUOTE, "mensagem chat sms", Item::Callout("mensagem", "Nome")),
    ("Mensagem enviada", "Balão de chat à direita", icons::SEND, "mensagem eu chat", Item::Callout("mensagem-eu", "Eu")),
    ("Poema", "Versos com quebras preservadas", icons::FEATHER, "poema poesia verso", Item::Callout("poema", "")),
    ("Epígrafe", "Citação com autor", icons::QUOTE, "epigrafe citacao", Item::Callout("epigrafe", "Autor")),
    ("Destaque", "Citação grande entre filetes", icons::QUOTE, "destaque pull quote", Item::Callout("destaque", "")),
    ("Notícia", "Recorte de jornal em duas colunas", icons::FILE_TEXT, "noticia jornal", Item::Callout("noticia", "Manchete")),
    ("Inscrição", "Runas, lápide, pedra entalhada", icons::GEM, "inscricao runa lapide", Item::Callout("inscricao", "")),
    ("Placa", "Cartaz em caixa-alta", icons::TRIANGLE_ALERT, "placa cartaz aviso", Item::Callout("placa", "")),
    ("Sonho", "Trecho onírico, esmaecido", icons::MOON, "sonho", Item::Callout("sonho", "")),
    ("Flashback", "Memória com filete lateral", icons::HISTORY, "flashback memoria passado", Item::Callout("flashback", "")),
    ("Duas colunas", "Texto em colunas", icons::LAYOUT_LIST, "colunas", Item::Callout("colunas", "")),
    ("Centralizado", "Texto no centro da página", icons::ALIGN_CENTER, "centro centralizado", Item::Callout("centro", "")),
    ("Estilo de texto", "Grito, sussurro, runas, glitch…", icons::WAND_SPARKLES, "estilo fonte grito sussurro", Item::Style),
    ("Personagem", "Link para a ficha", icons::USER, "personagem link", Item::Character),
    ("Nota de rodapé", "Número no texto, nota no fim", icons::HASH, "nota rodape", Item::Footnote),
    ("Quebra de página", "Começa nova página no livro", icons::MINUS, "pagina quebra", Item::Text("\n> [!pagina]\n\n")),
    ("Página ímpar", "A próxima página é ímpar", icons::MINUS, "pagina impar quebra", Item::Text("\n> [!pagina-impar]\n\n")),
    ("Trilha sonora", "Playlist deste trecho", icons::MUSIC, "musica playlist trilha", Item::Text("\n```playlist\nNome da faixa | https://open.spotify.com/track/...\n```\n")),
];

fn fold(s: &str) -> String {
    ateliet_core::scriptorium::mundo::deaccent(s)
}

/// What the "/" menu is showing.
#[derive(Clone, Default)]
struct Slash {
    /// Char index of the "/".
    start: usize,
    query: String,
    sel: usize,
    /// Choosing a character or a style after picking those items.
    sub: Option<u8>,
    footnote: Option<String>,
    /// A "/" put in only to anchor the footnote box (taken out on cancel).
    placeholder: bool,
}

fn items(q: &str) -> Vec<usize> {
    let q = fold(q).replace('-', "");
    (0..SLASH.len())
        .filter(|&i| {
            let (label, _, _, keys, _) = SLASH[i];
            q.is_empty() || fold(&format!("{label} {keys}")).split_whitespace().any(|w| w.starts_with(&q)) || fold(label).replace(' ', "").contains(&q)
        })
        .collect()
}

/// Before the text edit runs: while a menu is open, its keys are taken.
pub fn before(ui: &egui::Ui, id: egui::Id) {
    let open = ui.data(|d| d.get_temp::<Slash>(id.with("slash"))).is_some();
    if open {
        let mut got = None;
        ui.input_mut(|i| {
            for k in [egui::Key::ArrowUp, egui::Key::ArrowDown, egui::Key::Enter, egui::Key::Escape, egui::Key::Tab] {
                if i.consume_key(egui::Modifiers::NONE, k) {
                    got = Some(k);
                }
            }
        });
        if let Some(k) = got {
            ui.ctx().data_mut(|d| d.insert_temp(id.with("slash-key"), k));
        }
    }
}

fn replace(text: &mut String, a: usize, b: usize, with: &str) -> usize {
    let (ba, bb) = (char_to_byte(text, a), char_to_byte(text, b));
    text.replace_range(ba..bb, with);
    a + with.chars().count()
}

fn callout(kind: &str, title: &str, body: &str) -> String {
    let lines: Vec<String> = if body.is_empty() { vec!["> ".into()] } else { body.lines().map(|l| format!("> {l}")).collect() };
    format!("> [!{kind}]{}\n{}\n", if title.is_empty() { String::new() } else { format!(" {title}") }, lines.join("\n"))
}

fn next_footnote(text: &str) -> usize {
    let mut n = 1;
    while text.contains(&format!("[^{n}]")) {
        n += 1;
    }
    n
}

fn add_footnote(text: &mut String, at: usize, note: &str) -> usize {
    let n = next_footnote(text);
    let mark = format!("[^{n}]");
    let end = replace(text, at, at, &mark);
    let tail = format!("{}\n[^{n}]: {}\n", if text.ends_with('\n') { "" } else { "\n" }, note.replace('\n', " "));
    text.push_str(&tail);
    end
}

fn set_cursor(ctx: &egui::Context, id: egui::Id, a: usize, b: usize) {
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(a), egui::text::CCursor::new(b))));
    state.store(ctx, id);
    ctx.memory_mut(|m| m.request_focus(id));
}

/// After the text edit: the "/" menu and the selection bar. Returns true
/// when the text changed.
pub fn after(ui: &mut egui::Ui, id: egui::Id, text: &mut String, out: &egui::text_edit::TextEditOutput, p: Palette) -> bool {
    let Some(chars) = chapter() else {
        ui.data_mut(|d| d.remove::<Slash>(id.with("slash")));
        return false;
    };
    let focused = out.response.has_focus();
    let open: Option<Slash> = ui.data(|d| d.get_temp::<Slash>(id.with("slash")));
    let (a, b) = match out.cursor_range {
        Some(r) => (r.primary.ccursor.index.min(r.secondary.ccursor.index), r.primary.ccursor.index.max(r.secondary.ccursor.index)),
        None => match &open {
            Some(st) => (st.start, st.start),
            None => return false,
        },
    };
    let galley_pos = out.galley_pos;
    let rect_at = |ci: usize| out.galley.pos_from_cursor(&out.galley.from_ccursor(egui::text::CCursor::new(ci))).translate(galley_pos.to_vec2());
    let key = ui.data_mut(|d| {
        let k = d.get_temp::<egui::Key>(id.with("slash-key"));
        d.remove::<egui::Key>(id.with("slash-key"));
        k
    });
    let mut changed = false;

    // ── "/" menu ──
    let cursor_b = char_to_byte(text, b);
    let line_start = text[..cursor_b].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let before_cursor = &text[line_start..cursor_b];
    let slash_at = before_cursor.rfind('/').filter(|&i| {
        let q = &before_cursor[i + 1..];
        (i == 0 || before_cursor[..i].ends_with(char::is_whitespace)) && q.chars().count() <= 24 && q.chars().all(|c| c.is_alphanumeric() || c == '-')
    });
    let mut state = ui.data(|d| d.get_temp::<Slash>(id.with("slash")));
    match (focused && a == b, slash_at) {
        (true, Some(i)) => {
            let start = byte_to_char(text, line_start + i);
            let query = before_cursor[i + 1..].to_string();
            let mut st = state.take().filter(|s| s.start == start).unwrap_or_default();
            if st.query != query {
                st.sel = 0;
            }
            st.start = start;
            st.query = query;
            state = Some(st);
        }
        _ => {
            if state.as_ref().is_some_and(|s| s.sub.is_none() && s.footnote.is_none()) {
                state = None;
            }
        }
    }
    if let Some(mut st) = state {
        let list = items(&st.query);
        let mut chosen: Option<Item> = None;
        let mut close = false;
        match key {
            Some(egui::Key::ArrowDown) => st.sel += 1,
            Some(egui::Key::ArrowUp) => st.sel = st.sel.saturating_sub(1),
            Some(egui::Key::Escape) => close = true,
            Some(egui::Key::Enter) | Some(egui::Key::Tab) if st.sub.is_none() && st.footnote.is_none() => chosen = list.get(st.sel.min(list.len().saturating_sub(1))).map(|&i| SLASH[i].4),
            _ => {}
        }
        let anchor = rect_at(st.start).left_bottom() + vec2(0.0, 4.0);
        let mut pick_char: Option<String> = None;
        let mut pick_style: Option<&str> = None;
        let mut footnote_done: Option<String> = None;
        egui::Area::new(id.with("slash-area")).order(egui::Order::Foreground).fixed_pos(anchor).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(6.0).show(ui, |ui| {
                ui.set_width(300.0);
                if let Some(note) = st.footnote.as_mut() {
                    ui.label(RichText::new("Nota de rodapé").font(kit::medium(12.5)).color(p.text));
                    let r = ui.add(egui::TextEdit::multiline(note).desired_rows(2).desired_width(f32::INFINITY).hint_text(RichText::new("Texto da nota").color(p.faint)));
                    r.request_focus();
                    ui.horizontal(|ui| {
                        if kit::primary(ui, "Inserir", p).clicked() || kit::cmd_enter(ui.ctx()) {
                            footnote_done = Some(note.clone());
                        }
                        if kit::quiet(ui, None, "Cancelar", p).clicked() {
                            close = true;
                        }
                    });
                    return;
                }
                match st.sub {
                    Some(1) => {
                        ui.label(RichText::new("Personagem").font(kit::medium(12.5)).color(p.muted));
                        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                            for c in &chars {
                                let name = c.rsplit('/').next().unwrap_or(c).trim_end_matches(".md").to_string();
                                if kit::Row::new(&name).icon(icons::USER).show(ui, p).clicked() {
                                    pick_char = Some(name);
                                }
                            }
                            if chars.is_empty() {
                                ui.label(RichText::new("Este livro ainda não tem personagens.").color(p.faint));
                            }
                        });
                    }
                    Some(2) => {
                        ui.label(RichText::new("Estilo de texto").font(kit::medium(12.5)).color(p.muted));
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            for (k, v) in TEXT_STYLES {
                                if kit::Row::new(v.split(" — ").next().unwrap_or(v)).show(ui, p).clicked() {
                                    pick_style = Some(k);
                                }
                            }
                        });
                    }
                    _ => {
                        if list.is_empty() {
                            ui.label(RichText::new("Nada com esse nome").color(p.faint));
                        }
                        st.sel = st.sel.min(list.len().saturating_sub(1));
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            for (k, &i) in list.iter().enumerate() {
                                let (label, desc, glyph, _, item) = SLASH[i];
                                let on = k == st.sel;
                                let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), egui::Sense::click());
                                if on || r.hovered() {
                                    ui.painter().rect_filled(rect, 6.0, if on { p.bg_active } else { p.bg_hover });
                                }
                                if on {
                                    r.scroll_to_me(None);
                                }
                                ui.painter().text(rect.left_center() + vec2(16.0, 0.0), egui::Align2::CENTER_CENTER, glyph, kit::icon_font(15.0), p.muted);
                                ui.painter().text(rect.left_top() + vec2(36.0, 6.0), egui::Align2::LEFT_TOP, label, kit::medium(13.0), p.text);
                                ui.painter().text(rect.left_top() + vec2(36.0, 23.0), egui::Align2::LEFT_TOP, desc, egui::FontId::proportional(11.5), p.faint);
                                if r.clicked() {
                                    chosen = Some(item);
                                }
                            }
                        });
                    }
                }
            });
        });
        // what was picked
        let q_end = st.start + 1 + st.query.chars().count();
        if let Some(item) = chosen {
            match item {
                Item::Text(t) => {
                    let end = replace(text, st.start, q_end, t);
                    set_cursor(ui.ctx(), id, end, end);
                    changed = true;
                    close = true;
                }
                Item::Callout(kind, title) => {
                    let block = callout(kind, title, "");
                    let lead = if st.start > 0 && !text[..char_to_byte(text, st.start)].ends_with('\n') { "\n" } else { "" };
                    let ins = format!("{lead}{block}");
                    replace(text, st.start, q_end, &ins);
                    // the caret goes to the block's first line, after "> "
                    let first = st.start + lead.chars().count() + block.lines().next().map(|l| l.chars().count()).unwrap_or(0) + 3;
                    set_cursor(ui.ctx(), id, first, first);
                    changed = true;
                    close = true;
                }
                Item::Character => st.sub = Some(1),
                Item::Style => st.sub = Some(2),
                Item::Footnote => st.footnote = Some(String::new()),
            }
        }
        if let Some(name) = pick_char {
            let end = replace(text, st.start, q_end, &format!("[[{name}]]"));
            set_cursor(ui.ctx(), id, end, end);
            changed = true;
            close = true;
        }
        if let Some(k) = pick_style {
            let ins = format!("[texto]{{.{k}}}");
            replace(text, st.start, q_end, &ins);
            set_cursor(ui.ctx(), id, st.start + 1, st.start + 6);
            changed = true;
            close = true;
        }
        if let Some(note) = footnote_done {
            replace(text, st.start, q_end, "");
            let end = add_footnote(text, st.start, &note);
            set_cursor(ui.ctx(), id, end, end);
            changed = true;
            close = true;
        }
        if close {
            if st.placeholder && !changed && text.chars().nth(st.start) == Some('/') {
                replace(text, st.start, st.start + 1, "");
                changed = true;
            }
            ui.data_mut(|d| d.remove::<Slash>(id.with("slash")));
        } else {
            ui.data_mut(|d| d.insert_temp(id.with("slash"), st));
        }
        return changed;
    }
    ui.data_mut(|d| d.remove::<Slash>(id.with("slash")));

    // ── the bar over a selection ──
    if !focused || a == b || ui.input(|i| i.pointer.primary_down()) {
        return false;
    }
    let sel_text: String = text.chars().skip(a).take(b - a).collect();
    if sel_text.trim().is_empty() {
        return false;
    }
    let top = rect_at(a);
    let pos = top.left_top() - vec2(0.0, 40.0);
    let mut op: Option<Box<dyn FnOnce(&str) -> String>> = None;
    let mut foot = false;
    egui::Area::new(id.with("selbar")).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(4, 3)).corner_radius(9.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let wrap = |m: &'static str| -> Box<dyn FnOnce(&str) -> String> {
                    Box::new(move |t: &str| if t.len() > m.len() * 2 && t.starts_with(m) && t.ends_with(m) { t[m.len()..t.len() - m.len()].to_string() } else { format!("{m}{t}{m}") })
                };
                if kit::icon_button_sized(ui, icons::BOLD, "Negrito", false, p, 28.0, 14.0).clicked() {
                    op = Some(wrap("**"));
                }
                if kit::icon_button_sized(ui, icons::ITALIC, "Itálico", false, p, 28.0, 14.0).clicked() {
                    op = Some(wrap("*"));
                }
                ui.add_space(4.0);
                let st = kit::quiet(ui, Some(icons::WAND_SPARKLES), "Estilo", p);
                let pop = ui.make_persistent_id(id.with("selbar-style"));
                if st.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(pop));
                }
                egui::popup_below_widget(ui, pop, &st, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(230.0);
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (k, v) in TEXT_STYLES {
                            if kit::menu_item(ui, "", v.split(" — ").next().unwrap_or(v), p) {
                                op = Some(Box::new(move |t: &str| {
                                    let inner = regex::Regex::new(r"(?s)^\[(.*)\]\{[^}]*\}$").unwrap().replace(t, "$1").into_owned();
                                    format!("[{inner}]{{.{k}}}")
                                }));
                            }
                        }
                        kit::menu_sep(ui, p);
                        if kit::menu_item(ui, icons::ERASER, "Limpar estilos", p) {
                            op = Some(Box::new(|t: &str| regex::Regex::new(r"\[([^\[\]]+)\]\{[^}]+\}").unwrap().replace_all(t, "$1").into_owned()));
                        }
                    });
                });
                ui.add_space(6.0);
                let bl = kit::quiet(ui, Some(icons::SCROLL), "Bloco", p);
                let pop = ui.make_persistent_id(id.with("selbar-block"));
                if bl.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(pop));
                }
                egui::popup_below_widget(ui, pop, &bl, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(240.0);
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (k, v) in CALLOUT_STYLES.iter().filter(|x| !x.0.starts_with("pagina")) {
                            if kit::menu_item(ui, "", v, p) {
                                let k = *k;
                                op = Some(Box::new(move |t: &str| callout(k, "", t).trim_end().to_string()));
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                let pc = kit::icon_button_sized(ui, icons::USER, "Ligar a um personagem", false, p, 28.0, 14.0);
                let pop = ui.make_persistent_id(id.with("selbar-char"));
                if pc.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(pop));
                }
                egui::popup_below_widget(ui, pop, &pc, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(200.0);
                    if chars.is_empty() {
                        ui.label(RichText::new("Este livro ainda não tem personagens.").color(p.faint));
                    }
                    for c in &chars {
                        let name = c.rsplit('/').next().unwrap_or(c).trim_end_matches(".md").to_string();
                        if kit::menu_item(ui, icons::USER, &name, p) {
                            op = Some(Box::new(move |t: &str| format!("[[{name}|{t}]]")));
                        }
                    }
                });
                if kit::icon_button_sized(ui, icons::HASH, "Nota de rodapé", false, p, 28.0, 14.0).clicked() {
                    foot = true;
                }
            });
        });
    });
    if let Some(f) = op {
        let out_text = f(&sel_text);
        let end = replace(text, a, b, &out_text);
        set_cursor(ui.ctx(), id, a, end);
        return true;
    }
    if foot {
        // the note's text is asked in the "/" menu's box, anchored by a "/" at the end of the selection
        ui.data_mut(|d| d.insert_temp(id.with("slash"), Slash { start: b, footnote: Some(String::new()), placeholder: true, ..Default::default() }));
        let end = replace(text, b, b, "/");
        set_cursor(ui.ctx(), id, end, end);
        return true;
    }
    changed
}
