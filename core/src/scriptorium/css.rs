//! The book's CSS for flowing text (EPUB, the web reader), from the layout
//! settings. Same rules and class names as the plugin.

use serde_json::Value as Json;

pub const TEXT_STYLE_CSS: &str = r#"
.sc-s-grito{font-weight:800;text-transform:uppercase;letter-spacing:.04em;font-size:1.22em}
.sc-s-sussurro{font-size:.84em;font-style:italic;opacity:.66;letter-spacing:.02em}
.sc-s-manuscrito{font-family:"Snell Roundhand","Apple Chancery","Savoye LET","Brush Script MT",cursive;font-size:1.2em;line-height:1}
.sc-s-runa{font-family:"Luminari","Trattatello","Herculanum","Cinzel Decorative",fantasy;letter-spacing:.04em}
.sc-s-maquina{font-family:"American Typewriter","Courier Prime","Courier New",monospace;font-size:.92em}
.sc-s-tela{font-family:"SF Mono",Menlo,"Courier New",monospace;font-size:.86em;background:rgba(0,0,0,.08);padding:0 .25em;border-radius:2px}
.sc-s-glitch{text-shadow:.06em 0 rgba(255,0,72,.75),-.06em 0 rgba(0,190,255,.75)}
.sc-s-tremido{display:inline-block;font-style:oblique;animation:scShake .32s infinite steps(2)}
.sc-s-sangue{color:#8a0b0b;font-weight:600}
.sc-s-brilho{text-shadow:0 0 .35em currentColor,0 0 .12em currentColor}
.sc-s-versalete{font-variant:small-caps;letter-spacing:.05em}
.sc-s-gigante{font-size:2.2em;line-height:1}
.sc-s-minusculo{font-size:.66em}
.sc-s-espacado{letter-spacing:.32em}
.sc-s-invertido{display:inline-block;transform:scaleX(-1)}
.sc-s-de-cabeca{display:inline-block;transform:rotate(180deg)}
.sc-s-vertical{writing-mode:vertical-rl;display:inline-block;line-height:1.05}
.sc-s-apagado{opacity:.3}
.sc-s-borrado{filter:blur(.6px);opacity:.85}
.sc-s-riscado{text-decoration:line-through wavy}
.sc-s-ouro{color:#a8812f}
.sc-s-sombra{text-shadow:.08em .08em 0 rgba(0,0,0,.35)}
.sc-s-contorno{color:transparent;-webkit-text-stroke:.035em #222}
@keyframes scShake{0%{transform:translate(0,0)}50%{transform:translate(.03em,-.03em)}100%{transform:translate(-.03em,.02em)}}
@media print{.sc-s-tremido{animation:none}}
"#;

pub const CALLOUT_BOOK_CSS: &str = r#"
.sc-callout{text-indent:0;margin:var(--sc-lh,1.4em) 0}
.sc-callout p{text-indent:0}
.sc-callout-title{font-family:var(--sc-title-font);font-variant:small-caps;letter-spacing:.06em;margin-bottom:.25em}
.sc-callout:not([class*="sc-c-"]),.sc-c-note,.sc-c-info,.sc-c-nota{border-left:.5mm solid #999;padding-left:3mm}
.sc-c-carta{background:#f6efdf;border:.3mm solid #d8c9a6;padding:4mm 5mm;font-style:italic}
.sc-c-carta p{margin-bottom:.45em}
.sc-c-diario{background:repeating-linear-gradient(to bottom,transparent 0,transparent calc(1.4em - .25mm),#c9d6e3 calc(1.4em - .25mm),#c9d6e3 1.4em);line-height:1.4em;padding:2mm 5mm;font-family:"Bradley Hand","Noteworthy","Segoe Print",cursive}
.sc-c-tela{background:#0f1a1c;color:#7fe0c4;font-family:"SF Mono",Menlo,monospace;font-size:.84em;padding:3mm 4mm;border-radius:1.5mm;text-align:left}
.sc-c-tela p{text-align:left}
.sc-c-mensagem,.sc-c-mensagem-eu{max-width:74%;border-radius:4mm;padding:2mm 3.5mm;font-family:-apple-system,"Helvetica Neue",Arial,sans-serif;font-size:.86em;text-align:left;margin-top:.4em;margin-bottom:.4em}
.sc-c-mensagem p,.sc-c-mensagem-eu p{text-align:left}
.sc-c-mensagem{background:#e9e9ee;color:#111;margin-right:auto}
.sc-c-mensagem-eu{background:#2f7bf6;color:#fff;margin-left:auto}
.sc-c-mensagem .sc-callout-title,.sc-c-mensagem-eu .sc-callout-title{font-size:.75em;opacity:.7;font-variant:normal;letter-spacing:0}
.sc-c-poema{font-style:italic;margin-left:12%}
.sc-c-poema p{text-align:left}
.sc-c-epigrafe{margin-left:28%;font-size:.9em;font-style:italic;display:flex;flex-direction:column-reverse}
.sc-c-epigrafe p{text-align:right}
.sc-c-epigrafe .sc-callout-title{font-style:normal;text-align:right;margin:.3em 0 0}
.sc-c-epigrafe .sc-callout-title::before{content:"— "}
.sc-c-destaque{font-size:1.3em;font-style:italic;text-align:center;border-top:.3mm solid;border-bottom:.3mm solid;padding:2mm 4%}
.sc-c-destaque p{text-align:center}
.sc-c-inscricao{text-align:center;font-family:"Luminari","Cinzel Decorative","Trattatello",fantasy;letter-spacing:.06em;border:.6mm double currentColor;padding:3mm}
.sc-c-inscricao p{text-align:center}
.sc-c-noticia{font-family:"Times New Roman",Times,serif;font-size:.84em;border-top:.7mm solid;border-bottom:.3mm solid;padding:2mm 0}
.sc-c-noticia .sc-callout-title{font-variant:normal;font-weight:800;font-size:1.45em;text-transform:uppercase;line-height:1.1;letter-spacing:0}
.sc-c-noticia .sc-callout-body{column-count:2;column-gap:4mm}
.sc-c-placa{text-align:center;text-transform:uppercase;font-family:"Avenir Next Condensed",Impact,sans-serif;letter-spacing:.1em;border:.6mm solid;padding:3mm}
.sc-c-placa p{text-align:center}
.sc-c-sonho{font-style:italic;opacity:.75;margin-left:8%;margin-right:8%}
.sc-c-flashback{font-style:italic;border-left:.4mm solid #999;padding-left:4mm}
.sc-c-colunas .sc-callout-body{column-count:2;column-gap:5mm}
.sc-c-centro p{text-align:center}
.sc-c-direita p{text-align:right}
"#;

fn s<'a>(cfg: &'a Json, k: &str) -> &'a str {
    cfg[k].as_str().unwrap_or("")
}

fn f(cfg: &Json, k: &str, d: f64) -> f64 {
    cfg[k].as_f64().unwrap_or(d)
}

fn b(cfg: &Json, k: &str) -> bool {
    cfg[k].as_bool().unwrap_or(false)
}

/// Space between paragraphs, in lines ("auto": half a line when there is no indent).
pub fn para_gap(cfg: &Json) -> f64 {
    match s(cfg, "espacoParagrafo") {
        "" | "auto" => {
            if f(cfg, "recuo", 0.0) == 0.0 {
                0.5
            } else {
                0.0
            }
        }
        "meia" => 0.5,
        "linha" => 1.0,
        _ => 0.0,
    }
}

pub fn font_family(name: &str, fallback: &str) -> String {
    let n = name.replace(['"', '\''], "");
    let n = n.trim();
    let fb = if fallback.is_empty() { "\"Iowan Old Style\", \"Palatino Linotype\", Palatino, Georgia, serif" } else { fallback };
    if n.is_empty() { fb.to_string() } else { format!("\"{n}\", {fb}") }
}

fn case_css(k: &str) -> &'static str {
    match k {
        "alta" => "text-transform:uppercase;font-variant:normal;",
        "versalete" => "font-variant:small-caps;text-transform:none;",
        "versalete-total" => "font-variant:all-small-caps;text-transform:none;",
        _ => "font-variant:normal;text-transform:none;",
    }
}

/// The parts that follow the book's style (chapter openings, drop caps…).
pub fn style_css(cfg: &Json, scope: &str) -> String {
    let p = if scope.is_empty() { String::new() } else { format!("{scope} ") };
    let acc = s(cfg, "corDestaque");
    let c = |d: &str| if acc.is_empty() { d.to_string() } else { acc.to_string() };
    let align = match s(cfg, "tituloAlinhamento") {
        "esquerda" => "left",
        "direita" => "right",
        _ => "center",
    };
    let rule_margin = match align {
        "left" => ".55em auto .55em 0",
        "right" => ".55em 0 .55em auto",
        _ => ".55em auto",
    };
    let cap_font = if s(cfg, "capitularFonte").is_empty() { "var(--sc-title-font)".to_string() } else { font_family(s(cfg, "capitularFonte"), "") };
    let cap_n = f(cfg, "capitularLinhas", 3.0).clamp(2.0, 6.0);
    let cap_val = format!("{cap_n}{}", if s(cfg, "capitularEstilo") == "elevada" { " 1" } else { "" });
    let rotulo_caixa = if s(cfg, "rotuloCaixa").is_empty() { "versalete" } else { s(cfg, "rotuloCaixa") };
    let lbl_space = if rotulo_caixa == "normal" { 0.04 } else { 0.2 };
    let rt = f(cfg, "rotuloTamanho", 0.95);
    let mut out = String::new();
    out += &format!("{p}.sc-ch-head{{text-align:{align}}}\n");
    out += &format!("{p}.sc-ch-num{{font-family:var(--sc-title-font);{}letter-spacing:{lbl_space}em;font-size:{rt}em;{}color:{}}}\n", case_css(rotulo_caixa), if rt > 1.4 { "line-height:1.1;" } else { "" }, c("#6b5a45"));
    out += &format!(
        "{p}.sc-ch-title{{font-family:var(--sc-title-font);font-weight:{};font-size:{}em;line-height:1.15;margin:.3em 0 .2em;letter-spacing:{}em;{}{}{}}}\n",
        f(cfg, "tituloPeso", 500.0),
        f(cfg, "tituloTamanho", 2.0),
        f(cfg, "tituloEspacamento", 0.0),
        case_css(s(cfg, "tituloCaixa")),
        if b(cfg, "tituloItalico") { "font-style:italic;" } else { "" },
        if s(cfg, "corTitulos").is_empty() { String::new() } else { format!("color:{};", s(cfg, "corTitulos")) }
    );
    out += &format!("{p}.sc-ch-orn{{color:{}}}\n", c("#8a6d3b"));
    out += &format!("{p}.sc-ch-rule{{width:28%;height:0;margin:{rule_margin};color:{}}}\n", c("#8a6d3b"));
    out += &format!("{p}.sc-ch-rule.sc-rule-fino{{border-top:.3mm solid currentColor}}\n{p}.sc-ch-rule.sc-rule-curto{{width:9%;border-top:.45mm solid currentColor}}\n{p}.sc-ch-rule.sc-rule-duplo{{border-top:.25mm solid currentColor;border-bottom:.25mm solid currentColor;height:.7mm}}\n{p}.sc-ch-rule.sc-rule-pontilhado{{border-top:.6mm dotted currentColor}}\n{p}.sc-ch-rule.sc-rule-largo{{width:100%;border-top:.3mm solid currentColor}}\n");
    let cap_color = if s(cfg, "capitularCor").is_empty() { c("#3d2b1a") } else { s(cfg, "capitularCor").to_string() };
    out += &format!("{p}p.sc-dropcap::first-letter{{initial-letter:{cap_val};-webkit-initial-letter:{cap_val};font-family:{cap_font};color:{cap_color};margin-right:.08em}}\n");
    if b(cfg, "versaleteInicio") && s(cfg, "versaleteAlcance") != "palavras" {
        out += &format!("{p}p.sc-opener::first-line{{font-variant:small-caps;letter-spacing:.03em}}\n");
    }
    out += &format!("{p}.sc-lead{{font-variant:small-caps;letter-spacing:.03em}}\n{p}.sc-scene{{color:{}}}\n{p}.sc-part-label{{color:{}}}\n", c("#6d5a40"), c("#6b5a45"));
    if !s(cfg, "corTitulos").is_empty() {
        let t = s(cfg, "corTitulos");
        out += &format!("{p}.sc-part-title,{p}.sc-toc-title,{p}.sc-backmatter-title,{p}h2,{p}h3{{color:{t}}}\n");
    }
    out
}

/// CSS for flowing reading (EPUB and the web reader).
pub fn reflow_css(cfg: &Json, extra: &str) -> String {
    let title_font = font_family(s(cfg, "fonteTitulos"), "\"Cormorant\", Baskerville, Georgia, serif");
    let gap = para_gap(cfg);
    let cap_n = f(cfg, "capitularLinhas", 3.0).clamp(2.0, 6.0);
    format!(
        r#"
body{{margin:0;padding:0;--sc-title-font:{title_font};--sc-lh:1.5em}}
.sc-text{{font-family:{body};line-height:1.55;{hyph}text-align:{align}}}
.sc-text p{{margin:0;text-indent:{indent}em}}
{gap_rule}
.sc-text p.sc-first{{text-indent:0}}
.sc-text p.sc-dropcap::first-letter{{{float}font-size:{cap:.2}em;line-height:.82;margin:.06em .08em 0 0}}
.sc-text h1,.sc-text h2,.sc-text h3,.sc-text h4{{font-family:var(--sc-title-font);font-weight:500;text-align:center;hyphens:none;line-height:1.2}}
.sc-ch-head{{text-align:center;margin:3em 0 2.5em;text-indent:0;hyphens:none;page-break-after:avoid;break-after:avoid}}
.sc-ch-orn{{letter-spacing:.3em}}
.sc-ch-art img{{max-width:100%;max-height:40vh}}
.sc-ch-epi{{margin:1.5em 0 0 25%;font-style:italic;font-size:.9em;text-align:right}}
.sc-ch-epi cite{{display:block;font-style:normal;font-variant:small-caps}}
.sc-scene{{text-align:center;margin:1.2em 0;letter-spacing:.35em;font-family:var(--sc-title-font);text-indent:0}}
figure{{margin:1.2em 0;text-align:center;text-indent:0;page-break-inside:avoid;break-inside:avoid}}
figure img{{max-width:100%;height:auto}}
.sc-fig-esquerda{{float:left;width:45%;margin:.2em 1em .5em 0}}
.sc-fig-direita{{float:right;width:45%;margin:.2em 0 .5em 1em}}
.sc-fig-esquerda img,.sc-fig-direita img{{width:100%}}
.sc-contour{{shape-margin:.8em}}
img.sc-fig{{height:auto}}
.sc-fig-pagina,.sc-fig-sangria,.sc-fig-dupla,.sc-fig-composta{{page-break-before:always;break-before:page;page-break-after:always;break-after:page;margin:0}}
.sc-fig-composta img,.sc-fig-composta svg{{width:100%;height:auto;max-height:100vh;object-fit:contain}}
figcaption{{font-size:.82em;font-style:italic;margin-top:.3em}}
.sc-inline-img{{height:1.1em;vertical-align:-.15em}}
blockquote{{margin:1em 6%;font-size:.95em}}
.sc-pagebreak{{page-break-after:always;break-after:page;height:0}}
.sc-footnotes{{font-size:.85em;border-top:1px solid rgba(128,128,128,.5);margin-top:2em;padding-top:.5em}}
.sc-footnotes p{{text-indent:0}}
.sc-fnref{{font-size:.7em;vertical-align:super;line-height:0}}
.sc-fnref a,.sc-fnback{{text-decoration:none}}
.sc-table{{border-collapse:collapse;margin:1em auto;font-size:.9em}}
.sc-table td,.sc-table th{{border:1px solid rgba(128,128,128,.5);padding:.2em .5em}}
.sc-part-page{{text-align:center;padding-top:30vh;page-break-before:always}}
.sc-part-label{{font-variant:small-caps;letter-spacing:.3em}}
.sc-part-title{{font-family:var(--sc-title-font);font-size:2.2em}}
.sc-titlepage{{text-align:center;padding-top:15vh}}
.sc-tp-title{{font-family:var(--sc-title-font);font-size:2.4em;line-height:1.1}}
.sc-tp-sub{{font-style:italic;margin-top:.5em}}
.sc-tp-author{{font-variant:small-caps;letter-spacing:.25em;margin-top:2em}}
.sc-dedic,.sc-epigraph{{padding-top:25vh;font-style:italic;text-align:center}}
.sc-epigraph{{text-align:right;margin-left:20%}}
.sc-epigraph cite{{display:block;font-style:normal;font-variant:small-caps}}
.sc-credits{{font-size:.8em;margin-top:30vh}}
.sc-credits p{{text-indent:0}}
{text_styles}
{style}
{callouts}
{css_extra}
{extra}
"#,
        body = font_family(s(cfg, "fonte"), ""),
        hyph = if b(cfg, "hifenizar") { "hyphens:auto;-webkit-hyphens:auto;" } else { "" },
        align = if b(cfg, "justificar") { "justify" } else { "left" },
        indent = f(cfg, "recuo", 1.3),
        gap_rule = if gap > 0.0 { format!(".sc-text p + p{{margin-top:{gap}em}}") } else { String::new() },
        float = if s(cfg, "capitularEstilo") == "elevada" { "" } else { "float:left;" },
        cap = cap_n * 1.13,
        text_styles = TEXT_STYLE_CSS,
        style = style_css(cfg, ""),
        callouts = CALLOUT_BOOK_CSS,
        css_extra = s(cfg, "cssExtra"),
    )
}
