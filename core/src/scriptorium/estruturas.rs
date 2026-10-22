//! Narrative structures (beats), in the plugin's Markdown format:
//!
//! ```text
//! ## Group · 0–25%             ← phase/act (optional)
//! ### 1. Beat name · 0–5%      ← beat; number and percent range optional
//! - Key: value                 ← info shown on the beat's card
//! - Espelho: 21                ← mirrored beat (optional)
//! Free text = description.
//! ```
//!
//! `campos: [..]` in the frontmatter lists the questions asked of each
//! scene tied to a beat. Built-in structures have ids; the writer's own are
//! notes with `scriptorium: estrutura`, referred to as `nota:<path>`.

use super::book;
use super::enredo::Col;
use super::manuscript::ascii_slug;
use crate::dataview::index::Index;
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub nome: String,
    pub de: Option<f64>,
    pub ate: Option<f64>,
    pub auto: bool,
    pub desc: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Beat {
    pub id: String,
    pub num: Option<i64>,
    pub nome: String,
    pub de: Option<f64>,
    pub ate: Option<f64>,
    pub auto: bool,
    pub grupo: String,
    pub info: Vec<(String, String)>,
    pub desc: String,
    pub espelho: Option<String>,
}

impl Beat {
    pub fn label(&self) -> String {
        match self.num {
            Some(n) => format!("{n}. {}", self.nome),
            None => self.nome.clone(),
        }
    }
    pub fn info(&self, k: &str) -> Option<&str> {
        self.info.iter().find(|x| x.0 == k).map(|x| x.1.as_str())
    }
    pub fn de(&self) -> f64 {
        self.de.unwrap_or(0.0)
    }
    pub fn ate(&self) -> f64 {
        self.ate.or(self.de).unwrap_or(0.0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Structure {
    pub id: String,
    pub nome: String,
    pub desc: String,
    pub campos: Vec<String>,
    pub grupos: Vec<Group>,
    pub batidas: Vec<Beat>,
    pub capitulos: Option<i64>,
    /// The whole frontmatter (other keys are kept when writing back).
    pub fm: Json,
}

pub const BUILTIN: &[(&str, &str)] = &[
    ("tres-atos", r#"---
nome: Três atos
campos: [Objetivo da cena, Obstáculo, Virada]
---
A estrutura clássica: preparação, confrontação e resolução, com os marcos de K. M. Weiland.
## Ato I — Preparação · 0–25%
### Gancho · 0–2%
- Função: prender o leitor na primeira página
### Incidente incitante · 10–12%
- Função: o mundo comum é perturbado
### Primeiro ponto de virada · 23–25%
- Função: o protagonista entra no conflito sem volta
## Ato II — Confrontação · 25–75%
### Primeira pinça · 36–38%
- Função: a força antagônica mostra o tamanho da ameaça
### Ponto médio · 48–52%
- Função: virada de reação para ação; uma verdade aparece
### Segunda pinça · 61–63%
- Função: o antagonismo aperta de novo
### Crise / tudo está perdido · 73–75%
- Função: a maior derrota; a mentira do protagonista cai
## Ato III — Resolução · 75–100%
### Clímax · 85–95%
- Função: o confronto final
### Resolução · 96–100%
- Função: o novo equilíbrio"#),
    ("save-the-cat", r#"---
nome: Save the Cat (15 batidas)
campos: [Objetivo, Conflito, Valor em jogo]
---
As 15 batidas de Blake Snyder, adaptadas por Jessica Brody para romances.
## Ato 1 · 0–20%
### 1. Imagem de abertura · 0–1%
### 2. Tema declarado · 4–6%
### 3. Preparação · 1–10%
### 4. Catalisador · 10–11%
### 5. Debate · 11–20%
## Ato 2 · 20–80%
### 6. Entrada no Ato 2 · 20–21%
### 7. História B · 22–24%
### 8. Diversão e jogos · 20–50%
### 9. Ponto médio · 50–51%
### 10. Os vilões se aproximam · 50–75%
### 11. Tudo está perdido · 75–76%
### 12. Noite escura da alma · 76–80%
## Ato 3 · 80–100%
### 13. Entrada no Ato 3 · 80–81%
### 14. Final · 81–99%
### 15. Imagem final · 99–100%"#),
    ("jornada-heroi", r#"---
nome: Jornada do Herói (12 estágios)
campos: [O que o herói deseja, O que o herói teme, O que muda]
---
Joseph Campbell, na versão de Christopher Vogler.
## Partida · 0–25%
### 1. Mundo comum · 0–8%
### 2. Chamado à aventura · 8–11%
### 3. Recusa do chamado · 11–14%
### 4. Encontro com o mentor · 14–19%
### 5. Travessia do primeiro limiar · 20–25%
## Iniciação · 25–75%
### 6. Provas, aliados e inimigos · 25–40%
### 7. Aproximação da caverna oculta · 40–48%
### 8. Provação · 48–55%
### 9. Recompensa · 55–62%
## Retorno · 62–100%
### 10. Caminho de volta · 62–75%
### 11. Ressurreição · 75–92%
### 12. Retorno com o elixir · 92–100%"#),
    ("kishotenketsu", r#"---
nome: Kishōtenketsu
campos: [O que é apresentado, O que se desenvolve, A reviravolta, Como tudo se reconcilia]
---
Estrutura em quatro tempos (起承転結), sem exigir conflito central; a força está na reviravolta (ten). Base do método do Araki.
### Ki — introdução · 0–25%
- Função: apresentar personagens e mundo
### Shō — desenvolvimento · 25–50%
- Função: aprofundar sem mudar a direção
### Ten — reviravolta · 50–75%
- Função: um elemento inesperado muda a leitura de tudo
### Ketsu — conclusão · 75–100%
- Função: reconciliar o que veio antes com a reviravolta"#),
    ("freytag", r#"---
nome: Pirâmide de Freytag
campos: [Tensão, Virada]
---
### Exposição · 0–15%
### Ação crescente · 15–45%
### Clímax · 45–55%
### Ação decrescente · 55–85%
### Desfecho · 85–100%"#),
    ("sete-pontos", r#"---
nome: Estrutura de 7 pontos
campos: [Estado do protagonista, Virada]
---
Dan Wells: comece pelo fim (resolução) e espelhe no gancho.
### 1. Gancho · 0–3%
- Espelho: 7
### 2. Primeiro ponto de virada · 13–17%
- Espelho: 6
### 3. Primeira pinça · 28–32%
- Espelho: 5
### 4. Ponto médio · 48–52%
### 5. Segunda pinça · 63–67%
- Espelho: 3
### 6. Segundo ponto de virada · 78–82%
- Espelho: 2
### 7. Resolução · 96–100%
- Espelho: 1"#),
];

pub const TEMPLATE: &str = r####"---
scriptorium: estrutura
nome: Minha estrutura
campos: [Valor em jogo, Pergunta dramática, Virada]
---
Descreva aqui a ideia da estrutura. Cada "###" é uma batida/ato; "##" agrupa (fase, ato, setenário).
Faixas percentuais ("· 0–10%") dizem onde a batida deveria cair no livro. Sem faixa, o Scriptorium distribui por igual.
As linhas "- Chave: valor" aparecem no cartão da batida. "- Espelho: N" liga batidas espelhadas.

## Fase 1 · 0–50%
### 1. Primeira batida · 0–10%
- Função: o que ela faz na história
### 2. Segunda batida
- Função: …

## Fase 2 · 50–100%
### 3. Terceira batida
### 4. Quarta batida · 90–100%"####;

pub const ESCOPOS: [(&str, &str); 4] = [("livro", "Livro inteiro"), ("parte", "Um arco"), ("capitulos", "Capítulos"), ("capitulo", "Um capítulo")];
pub const MODOS: [(&str, &str); 2] = [("proporcional", "Pelas porcentagens"), ("sequencial", "Uma batida por capítulo")];

fn pct(s: &str) -> Option<f64> {
    s.replace(',', ".").parse().ok()
}

/// "Name · 10–25%" → ("Name", 10, 25)
fn split_range(t: &str) -> (String, Option<f64>, Option<f64>) {
    static RANGE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"\s*[·|—-]\s*([\d.,]+)\s*(?:[–\-a]\s*([\d.,]+))?\s*%\s*$").unwrap());
    match RANGE.captures(t) {
        Some(c) => {
            let de = pct(&c[1]);
            let ate = c.get(2).and_then(|m| pct(m.as_str())).or(de);
            (t[..c.get(0).unwrap().start()].trim().to_string(), de, ate)
        }
        None => (t.trim().to_string(), None, None),
    }
}

pub fn parse(md: &str, id: &str) -> Structure {
    let (fm, body) = match crate::syntax::frontmatter(md) {
        Some(f) => (crate::syntax::parse_yaml(&md[f.yaml.clone()]).unwrap_or(json!({})), &md[(f.block.end + 1).min(md.len())..]),
        None => (json!({}), md),
    };
    let campos = match &fm["campos"] {
        Json::Array(a) => a.iter().map(|x| x.as_str().map(String::from).unwrap_or_else(|| x.to_string())).collect(),
        Json::String(s) if !s.is_empty() => vec![s.clone()],
        _ => vec![],
    };
    let mut st = Structure {
        id: id.into(),
        nome: fm["nome"].as_str().unwrap_or("Estrutura").to_string(),
        campos,
        capitulos: fm["capitulos"].as_i64().or_else(|| fm["capitulos"].as_str().and_then(|s| s.parse().ok())),
        fm: fm.clone(),
        ..Default::default()
    };
    static NUM: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"^(\d+)[.)]\s*(.+)$").unwrap());
    static KV: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"^\s*[-*]\s+\**([^:*]{1,40})\**:\s*(.+)$").unwrap());
    let mut idx = 0;
    let mut in_beat = false;
    for raw in body.lines() {
        let line = raw.trim_end();
        if let Some(t) = line.strip_prefix("## ").filter(|_| !line.starts_with("###")) {
            let (nome, de, ate) = split_range(t);
            st.grupos.push(Group { nome, de, ate, auto: de.is_none(), desc: String::new() });
            in_beat = false;
            continue;
        }
        if let Some(t) = line.strip_prefix("### ") {
            let (mut nome, de, ate) = split_range(t);
            let mut num = None;
            if let Some(c) = NUM.captures(&nome.clone()) {
                num = c[1].parse().ok();
                nome = c[2].trim().to_string();
            }
            idx += 1;
            let slug: String = ascii_slug(&nome).chars().take(24).collect();
            let id = format!("b{}", num.map(|n: i64| n.to_string()).unwrap_or_else(|| if slug.is_empty() { idx.to_string() } else { slug }));
            st.batidas.push(Beat { id, num, nome, de, ate, auto: de.is_none(), grupo: st.grupos.last().map(|g| g.nome.clone()).unwrap_or_default(), ..Default::default() });
            in_beat = true;
            continue;
        }
        if in_beat {
            if let Some(c) = KV.captures(line) {
                let (k, v) = (c[1].trim().to_string(), c[2].trim().to_string());
                let b = st.batidas.last_mut().unwrap();
                if k.eq_ignore_ascii_case("espelho") {
                    let n: Option<i64> = v.split(|c: char| !c.is_ascii_digit()).next().and_then(|d| d.parse().ok());
                    b.espelho = Some(format!("b{}", n.map(|n| n.to_string()).unwrap_or_else(|| ascii_slug(&v))));
                } else {
                    b.info.push((k, v));
                }
                continue;
            }
        }
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let push = |s: &mut String| {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(t);
        };
        if in_beat {
            push(&mut st.batidas.last_mut().unwrap().desc);
        } else if let Some(g) = st.grupos.last_mut() {
            push(&mut g.desc);
        } else if !line.starts_with("# ") {
            push(&mut st.desc);
        }
    }
    // beats without a range share their group's span (or the whole book)
    let mut by_group: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, b) in st.batidas.iter().enumerate().filter(|(_, b)| b.de.is_none()) {
        match by_group.iter_mut().find(|g| g.0 == b.grupo) {
            Some(g) => g.1.push(i),
            None => by_group.push((b.grupo.clone(), vec![i])),
        }
    }
    for (g, list) in by_group {
        let gr = st.grupos.iter().find(|x| x.nome == g);
        let a = gr.and_then(|x| x.de).unwrap_or(0.0);
        let z = gr.and_then(|x| x.ate).unwrap_or(100.0);
        let n = list.len() as f64;
        for (k, i) in list.into_iter().enumerate() {
            st.batidas[i].de = Some(a + (z - a) * k as f64 / n);
            st.batidas[i].ate = Some(a + (z - a) * (k + 1) as f64 / n);
        }
    }
    for gi in 0..st.grupos.len() {
        if st.grupos[gi].de.is_none() {
            let bs: Vec<&Beat> = st.batidas.iter().filter(|b| b.grupo == st.grupos[gi].nome).collect();
            if !bs.is_empty() {
                st.grupos[gi].de = Some(bs.iter().map(|b| b.de()).fold(f64::INFINITY, f64::min));
                st.grupos[gi].ate = Some(bs.iter().map(|b| b.ate()).fold(f64::NEG_INFINITY, f64::max));
            }
        }
    }
    st
}

fn yaml_str(v: &str) -> String {
    static PLAIN: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"^[\w\sÀ-ÿ.,()'-]+$").unwrap());
    let lower = v.to_lowercase();
    if PLAIN.is_match(v) && v.trim() == v && !["true", "false", "null"].iter().any(|k| lower.starts_with(k)) && !v.starts_with(|c: char| c.is_ascii_digit()) {
        v.to_string()
    } else {
        serde_json::to_string(v).unwrap()
    }
}

fn fmt_pct(x: f64) -> String {
    let r = (x * 10.0).round() / 10.0;
    if r.fract() == 0.0 { format!("{}", r as i64) } else { format!("{r}") }
}

/// Beats in written order: the loose ones, then each group's.
pub fn ordered(st: &Structure) -> Vec<usize> {
    let loose = |b: &Beat| b.grupo.is_empty() || !st.grupos.iter().any(|g| g.nome == b.grupo);
    let mut out: Vec<usize> = (0..st.batidas.len()).filter(|&i| loose(&st.batidas[i])).collect();
    for g in &st.grupos {
        out.extend((0..st.batidas.len()).filter(|&i| st.batidas[i].grupo == g.nome));
    }
    out
}

pub fn to_md(st: &Structure) -> String {
    let range = |auto: bool, de: Option<f64>, ate: Option<f64>| match (auto, de) {
        (false, Some(d)) => format!(" · {}–{}%", fmt_pct(d), fmt_pct(ate.unwrap_or(d))),
        _ => String::new(),
    };
    let mut out = vec!["---".to_string(), "scriptorium: estrutura".into(), format!("nome: {}", yaml_str(if st.nome.is_empty() { "Estrutura" } else { &st.nome }))];
    if let Some(c) = st.capitulos {
        out.push(format!("capitulos: {c}"));
    }
    if let Some(o) = st.fm.as_object() {
        for (k, v) in o {
            if ["nome", "campos", "capitulos", "scriptorium"].contains(&k.as_str()) || v.is_null() || v.is_object() || v.is_array() {
                continue;
            }
            out.push(match v {
                Json::String(s) => format!("{k}: {}", serde_json::to_string(s).unwrap()),
                other => format!("{k}: {other}"),
            });
        }
    }
    let campos: Vec<String> = st.campos.iter().map(|c| if c.contains([',', '[', ']', ':', '#']) { serde_json::to_string(c).unwrap() } else { c.clone() }).collect();
    out.push(format!("campos: [{}]", campos.join(", ")));
    out.push("---".into());
    if !st.desc.is_empty() {
        out.push(st.desc.clone());
    }
    let mirror = |b: &Beat| -> String {
        b.espelho.as_ref().and_then(|e| st.batidas.iter().find(|x| &x.id == e)).map(|m| m.num.map(|n| n.to_string()).unwrap_or_else(|| m.nome.clone())).unwrap_or_default()
    };
    let beat = |out: &mut Vec<String>, b: &Beat| {
        out.push(format!("### {}{}{}", b.num.map(|n| format!("{n}. ")).unwrap_or_default(), if b.nome.is_empty() { "Batida" } else { &b.nome }, range(b.auto, b.de, b.ate)));
        for (k, v) in &b.info {
            if !v.trim().is_empty() {
                out.push(format!("- {k}: {}", v.replace('\n', " ")));
            }
        }
        let m = mirror(b);
        if !m.is_empty() {
            out.push(format!("- Espelho: {m}"));
        }
        if !b.desc.is_empty() {
            out.push(b.desc.clone());
        }
    };
    let loose: Vec<&Beat> = st.batidas.iter().filter(|b| b.grupo.is_empty() || !st.grupos.iter().any(|g| g.nome == b.grupo)).collect();
    if !loose.is_empty() {
        out.push(String::new());
        for b in loose {
            beat(&mut out, b);
        }
    }
    for g in &st.grupos {
        out.push(String::new());
        out.push(format!("## {}{}", if g.nome.is_empty() { "Grupo" } else { &g.nome }, range(g.auto, g.de, g.ate)));
        if !g.desc.is_empty() {
            out.push(g.desc.clone());
        }
        for b in st.batidas.iter().filter(|b| b.grupo == g.nome) {
            beat(&mut out, b);
        }
    }
    out.join("\n") + "\n"
}

/// An entry of the structure library.
#[derive(Clone, Debug)]
pub struct Entry {
    pub r#ref: String,
    pub nome: String,
    pub pronta: bool,
    pub capitulos: Option<i64>,
}

pub fn folder(settings_folder: &str) -> String {
    if settings_folder.trim().is_empty() { "Livros/Estruturas".into() } else { settings_folder.trim_matches('/').to_string() }
}

pub fn notes(ix: &Index) -> Vec<String> {
    let mut out: Vec<String> = ix.page_paths().filter(|p| ix.page(p).is_some_and(|pg| book::kind(pg) == "estrutura")).map(String::from).collect();
    out.sort();
    out
}

pub fn list(ix: &Index) -> Vec<Entry> {
    let mut out: Vec<Entry> = BUILTIN.iter().map(|(id, md)| {
        let st = parse(md, id);
        Entry { r#ref: id.to_string(), nome: st.nome, pronta: true, capitulos: st.capitulos }
    }).collect();
    for p in notes(ix) {
        let pg = ix.page(&p).unwrap();
        let nome = pg.yaml["nome"].as_str().map(String::from).unwrap_or_else(|| p.rsplit('/').next().unwrap_or(&p).trim_end_matches(".md").to_string());
        out.push(Entry { r#ref: format!("nota:{p}"), nome, pronta: false, capitulos: pg.yaml["capitulos"].as_i64() });
    }
    out
}

pub fn get(root: &Path, r: &str) -> Option<Structure> {
    if let Some((id, md)) = BUILTIN.iter().find(|(id, _)| *id == r) {
        return Some(parse(md, id));
    }
    let p = r.strip_prefix("nota:")?;
    std::fs::read_to_string(root.join(p)).ok().map(|t| parse(&t, r))
}

/// Write a structure; a built-in one becomes a note of the writer's.
/// Returns the reference it ends up with.
pub fn write(root: &Path, dir: &str, r: Option<&str>, st: &Structure) -> Result<String, String> {
    let md = to_md(st);
    if let Some(p) = r.and_then(|r| r.strip_prefix("nota:")) {
        if root.join(p).exists() {
            std::fs::write(root.join(p), &md).map_err(|e| e.to_string())?;
            return Ok(format!("nota:{p}"));
        }
    }
    std::fs::create_dir_all(root.join(dir)).map_err(|e| e.to_string())?;
    let base = super::safe_name(if st.nome.is_empty() { "Minha estrutura" } else { &st.nome });
    let mut p = format!("{dir}/{base}.md");
    let mut n = 2;
    while root.join(&p).exists() {
        p = format!("{dir}/{base} {n}.md");
        n += 1;
    }
    std::fs::write(root.join(&p), md).map_err(|e| e.to_string())?;
    Ok(format!("nota:{p}"))
}

/// Ids as the Markdown reader computes them ("b" + number, or the name).
pub fn stable_ids(st: &Structure) -> Result<Vec<String>, String> {
    let mut seen: Vec<String> = Vec::new();
    for (k, i) in ordered(st).into_iter().enumerate() {
        let b = &st.batidas[i];
        let slug: String = ascii_slug(&b.nome).chars().take(24).collect();
        let id = format!("b{}", b.num.map(|n| n.to_string()).unwrap_or_else(|| if slug.is_empty() { (k + 1).to_string() } else { slug }));
        if seen.contains(&id) {
            return Err(format!("Duas batidas ficariam iguais ({}). Mude o número ou o nome de uma delas.", b.num.map(|n| format!("número {n}")).unwrap_or_else(|| b.nome.clone())));
        }
        seen.push(id);
    }
    Ok(seen)
}

// ── where beats fall ─────────────────────────────────────────────────────

pub fn is_numbered(c: &Col) -> bool {
    c.numbered && !["prologo", "epilogo", "interludio"].contains(&c.tipo.as_str())
}

pub fn ch_key(c: &Col) -> String {
    match c.planned.as_ref().or(c.plan.as_ref()) {
        Some(p) => format!("plan:{}", p["id"].as_str().unwrap_or("")),
        None => c.path.clone(),
    }
}

fn ch_matches(c: &Col, key: &str) -> bool {
    !key.is_empty() && (c.path == key || ch_key(c) == key)
}

/// Columns (indices into `cols`) a structure application covers, in order.
pub fn scope_of(a: &Json, cols: &[Col]) -> Vec<usize> {
    let numerados = a["numerados"].as_bool().unwrap_or(false);
    let inc: Vec<usize> = (0..cols.len()).filter(|&i| cols[i].include && (!numerados || is_numbered(&cols[i]))).collect();
    let de = a["de"].as_str().unwrap_or("");
    match a["escopo"].as_str().unwrap_or("livro") {
        "parte" => {
            let part = a["parte"].as_str().unwrap_or("");
            inc.into_iter().filter(|&i| cols[i].part == part).collect()
        }
        "capitulo" => (0..cols.len()).filter(|&i| ch_matches(&cols[i], de)).collect(),
        "capitulos" => {
            let Some(i0) = cols.iter().position(|c| ch_matches(c, de)) else { return vec![] };
            if let Some(n) = a["n"].as_u64() {
                return inc.into_iter().filter(|&i| i >= i0).take(n as usize).collect();
            }
            let ate = a["ate"].as_str().unwrap_or("");
            let i1 = cols.iter().rposition(|c| ch_matches(c, ate)).unwrap_or(i0).max(i0);
            inc.into_iter().filter(|&i| i >= i0 && i <= i1).collect()
        }
        _ => inc,
    }
}

pub fn scope_label(a: &Json, cols: &[Col]) -> String {
    match a["escopo"].as_str().unwrap_or("livro") {
        "livro" => "livro inteiro".into(),
        "parte" => a["parte"].as_str().filter(|s| !s.is_empty()).unwrap_or("sem arco").into(),
        _ => {
            let list = scope_of(a, cols);
            let lab = |i: usize| if cols[i].label.chars().all(|c| c.is_ascii_digit()) { cols[i].label.clone() } else { cols[i].title.clone() };
            match list.len() {
                0 => "trecho vazio".into(),
                1 => format!("capítulo {}", lab(list[0])),
                _ => format!("capítulos {}–{}", lab(list[0]), lab(*list.last().unwrap())),
            }
        }
    }
}

/// Where a beat (or group) falls: `x0..x1` as a fraction of the columns
/// `from..=to`, the columns it touches, and whether it fits at all.
#[derive(Clone, Debug, Default)]
pub struct Place {
    pub x0: f64,
    pub x1: f64,
    pub caps: Vec<usize>,
    pub fora: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub beats: HashMap<String, Place>,
    pub groups: HashMap<String, Place>,
    pub from: Option<usize>,
    pub to: usize,
    pub list: Vec<usize>,
}

/// `words[i]` are the words of column i (used when `by_words`).
pub fn layout(a: &Json, st: &Structure, cols: &[Col], words: &[usize], by_words: bool) -> Layout {
    let list = scope_of(a, cols);
    let mut out = Layout { list: list.clone(), ..Default::default() };
    let (Some(&from), Some(&to)) = (list.first(), list.last()) else { return out };
    out.from = Some(from);
    out.to = to;
    let span = (to - from + 1) as f64;
    let col = |i: usize| (i - from) as f64;
    if a["modo"].as_str() == Some("sequencial") {
        for (k, b) in st.batidas.iter().enumerate() {
            let place = match list.get(k) {
                Some(&c) => Place { x0: col(c) / span, x1: (col(c) + 1.0) / span, caps: vec![c], fora: false },
                None => Place { x0: 1.0, x1: 1.0, caps: vec![], fora: true },
            };
            out.beats.insert(b.id.clone(), place);
        }
        for g in &st.grupos {
            let ms: Vec<&Place> = st.batidas.iter().filter(|b| b.grupo == g.nome).filter_map(|b| out.beats.get(&b.id)).filter(|m| !m.fora).collect();
            if !ms.is_empty() {
                let p = Place { x0: ms.iter().map(|m| m.x0).fold(1.0, f64::min), x1: ms.iter().map(|m| m.x1).fold(0.0, f64::max), ..Default::default() };
                out.groups.insert(g.nome.clone(), p);
            }
        }
        return out;
    }
    // positions (0–100%) of each covered chapter, by chapters or by words
    let sizes: Vec<f64> = list.iter().map(|&i| if by_words { words.get(i).copied().unwrap_or(0) as f64 } else { 1.0 }).collect();
    let total: f64 = sizes.iter().sum::<f64>().max(1e-9);
    let mut pos = Vec::new();
    let mut acc = 0.0;
    for s in &sizes {
        pos.push((acc / total * 100.0, (acc + s) / total * 100.0));
        acc += s;
    }
    let to_x = |p: f64| -> f64 {
        for (k, &c) in list.iter().enumerate() {
            let (x, y) = pos[k];
            if p <= y + 1e-9 {
                let f = if y > x { ((p - x) / (y - x)).clamp(0.0, 1.0) } else { 0.0 };
                return (col(c) + f) / span;
            }
        }
        1.0
    };
    for b in &st.batidas {
        let (de, ate) = (b.de(), b.ate());
        let caps: Vec<usize> = list.iter().enumerate().filter(|(k, _)| {
            let (x, y) = pos[*k];
            (y > de + 1e-6 && x < ate - 1e-6) || (de == ate && x <= de && y >= de)
        }).map(|(_, &c)| c).collect();
        out.beats.insert(b.id.clone(), Place { x0: to_x(de), x1: to_x(ate), caps, fora: false });
    }
    for g in &st.grupos {
        if let Some(de) = g.de {
            out.groups.insert(g.nome.clone(), Place { x0: to_x(de), x1: to_x(g.ate.unwrap_or(de)), ..Default::default() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for (id, md) in BUILTIN {
            let st = parse(md, id);
            assert!(!st.batidas.is_empty(), "{id}");
            let st2 = parse(&to_md(&st), id);
            assert_eq!(st.batidas.iter().map(|b| (&b.id, &b.nome, b.num)).collect::<Vec<_>>(), st2.batidas.iter().map(|b| (&b.id, &b.nome, b.num)).collect::<Vec<_>>(), "{id}");
            assert_eq!(st.campos, st2.campos);
        }
        let st = parse(BUILTIN.iter().find(|x| x.0 == "sete-pontos").unwrap().1, "x");
        assert_eq!(st.batidas[0].espelho.as_deref(), Some("b7"));
        let st = parse(BUILTIN[0].1, "tres-atos");
        assert_eq!(st.grupos.len(), 3);
        assert_eq!(st.batidas[0].info("Função"), Some("prender o leitor na primeira página"));
        let t = parse(TEMPLATE, "t");
        // free beats share their group's span
        assert_eq!(t.batidas[1].de, Some(0.0));
        assert_eq!(t.batidas[1].ate, Some(50.0));
    }

    #[test]
    fn places() {
        let col = |p: &str| Col { path: p.into(), title: p.into(), label: p.into(), status: String::new(), part: String::new(), tipo: "capitulo".into(), numbered: true, include: true, planned: None, plan: None };
        let cols: Vec<Col> = (1..=4).map(|i| col(&i.to_string())).collect();
        let st = parse(BUILTIN.iter().find(|x| x.0 == "freytag").unwrap().1, "f");
        let l = layout(&json!({ "escopo": "livro" }), &st, &cols, &[], false);
        let clim = &l.beats["bclimax"];
        assert!((clim.x0 - 0.45).abs() < 1e-9);
        assert_eq!(clim.caps, vec![1, 2]);
        let l = layout(&json!({ "escopo": "livro", "modo": "sequencial" }), &st, &cols, &[], false);
        assert!(l.beats["bdesfecho"].fora);
        assert_eq!(scope_label(&json!({ "escopo": "capitulos", "de": "2", "n": 2 }), &cols), "capítulos 2–3");
    }
}
