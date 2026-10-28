# Arquitetura do Ateliet

```
┌──────────────────────────────── src/ (crate `ateliet`, interface em egui) ────────────────────────────────┐
│ main.rs     janela (eframe/glow), ícone, vault inicial                                                    │
│ app.rs      estado: vault, abas, splits, histórico, salvamento, ações, arrastar/colar, modelos             │
│ editor.rs · live.rs · markdown.rs   editor com realce, Live Preview por bloco, árvore e desenho do Markdown│
│ align.rs · typo.rs · media_ui.rs    alinhamento por nota, tipografia do tema, players e PDF nas notas      │
│ panels.rs · commands.rs · settings_pages.rs   barras laterais, paleta e atalhos, configurações             │
│ images_ui.rs · vision.rs   modo imagens, coleções, visualizador, moodboards, busca por descrição (CLIP)    │
│ board_ui.rs, board_ui/   storyboard: quadros, camadas, pincéis, ferramentas, referências, exportações      │
│ sc/         Scriptorium, Enredo, Mundo, Mapas, Diagramação, Capa, Compositor, Trilha                       │
│ dv.rs · templater.rs · daily.rs   Dataview, Templater, notas diárias e calendário                          │
│ vault.rs · host.rs · config.rs · update.rs   arquivos e watcher, caches, configurações, autoatualização    │
└──────────────────────────────────────────────────┬────────────────────────────────────────────────────────┘
                                                   │ chamadas diretas (sem IPC)
┌──────────────────────────────────────────────────▼────────────────────────────────────────────────────────┐
│ core/ (crate `ateliet_core`, sem janela, testável)                                                        │
│   text.rs · syntax.rs · metadata.rs · render.rs   frontmatter, wikilinks, tags, callouts, metadados        │
│   search.rs · vault.rs · relink.rs                busca com a sintaxe do Obsidian, listagem, renomear links │
│   dataview/ · script/ (QuickJS) · calendar.rs · datefmt.rs   Dataview, dataviewjs, Templater, datas         │
│   images.rs · clip.rs · pictogrep.rs · pinterest.rs · moodboard.rs   imagens, CLIP, Pictogrep, moodboards  │
│   board/ · clipstudio.rs                          storyboard: documento, pintura, composição, .clip       │
│   scriptorium/ · typeset/                         livros, enredo, mundo; composição de páginas e PDF       │
│   css_theme.rs · texmath.rs · texlayout.rs        temas (cores, tipografia, fontes), TeX em texto e caixas │
│   mermaid.rs, mermaid/ · media.rs                 diagramas Mermaid; páginas de PDF, vídeo pelo ffmpeg     │
└───────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

## Crates

O repositório é um workspace com dois crates, mais um terceiro vazio:
- **`core/` (`ateliet_core`)**: tudo o que não depende de janela. A busca de imagens por CLIP (ONNX Runtime) entra pelas features `ort-bundled` e `ort-dynamic`.
- **`.` (`ateliet`)**: o app, desenhado com egui, sem webview nem GTK. As features são `vision` (CLIP com ONNX Runtime embutido, padrão), `vision-dynamic` (ONNX Runtime do sistema) e `audio` (rodio, padrão).
- **`compat/tauri` (`tauri-compat`)**: um crate sem código, ligado como dependência `tauri`. Existe só para que o atualizador da versão anterior do Ateliet (feita com Tauri), que compila com `--features tauri/custom-protocol` e roda `build.sh` antes, consiga compilar esta versão e trocar o executável instalado por ela.

## Princípios

1. **O vault é a fonte da verdade.** Nada fica num banco próprio. O índice é recalculado a cada abertura e mantido por eventos do watcher.
2. **Escrita mínima e cirúrgica nos arquivos compartilhados.** As configurações de `.obsidian/` são gravadas com *merge* das chaves alteradas, preservando as desconhecidas. O que é só do Ateliet fica em `.obsidian/ateliet/`.
3. **Nada de trabalho pesado na thread da janela.** Índices, imagens, composição de páginas, scripts e a atualização rodam em threads e entregam o resultado por canais; a janela redesenha quando ele chega.

## Autoatualização (`src/update.rs`)

`build.rs` grava no binário o commit (`ATELIET_GIT_COMMIT`), o caminho do clone (`ATELIET_SOURCE_DIR`) e as features usadas (`ATELIET_FEATURES`). O app roda `git fetch` no clone e compara o upstream com o commit gravado; para atualizar, roda `git pull --ff-only` e `cargo build -p ateliet --release` com as mesmas features, copia `target/release/ateliet` para o lado do executável em uso (`.ateliet-update`), troca os dois por renomeação e, no macOS, reassina o `.app` ad hoc. Um binário que está no próprio `target/` do clone é recompilado no lugar, com o mesmo perfil.

## Resolução de links

A ordem de tentativa é a mesma do Obsidian:
1. caminho exato (com e sem `.md`);
2. caminho relativo à pasta da nota de origem;
3. por nome, sem diferenciar maiúsculas: se houver vários candidatos, vence o da mesma pasta, senão o de caminho mais curto.

## Ferramentas embutidas (`core/src/dataview/`, `core/src/script/`, `core/src/calendar.rs`)

Dataview, Templater, Calendar e o Moodboard do Pictogrep eram plugins e agora fazem parte do Ateliet. A lógica está no núcleo; `src/dv.rs`, `src/templater.rs` e `src/daily.rs` só desenham e aplicam o resultado.

- **Índice** (`dataview/index.rs`): cada nota vira uma página com frontmatter, campos inline (linha inteira `chave:: valor`, `[chave:: valor]` e `(chave:: valor)` fora de código, sob o nome original e o canônico, minúsculo e com `-`), tags (com as tags-pai em `file.tags`), links resolvidos na mesma ordem do Obsidian, back-links, itens de lista e tarefas (texto, status, filhos, seção, datas com emoji 📅 ✅ ➕ 🛫 ⏳), e `file.day` a partir do nome ou do campo `date`. O índice é construído em paralelo 3 s depois de o vault abrir, ou na primeira consulta, e os eventos do watcher do vault relêem só os caminhos alterados.
- **Valores e expressões** (`value.rs`, `parse.rs`, `eval.rs`): os tipos do Dataview (texto, número, booleano, data, duração com partes de calendário, link, lista, objeto, função), a mesma ordenação entre tipos, campos com `-` no nome, `row["Nome com espaço"]`, `this`, indexação de listas que mapeia (`rows.file.link`), links que dão acesso aos campos da nota (`[[A]].status`), lambdas e cerca de 70 funções. `date(today)` e `dur(1 week)` são literais.
- **Consultas** (`query.rs`): `FROM` monta o conjunto (tags com subtags, pastas ou nota, `[[link]]` = quem aponta para ela, `outgoing([[link]])`, `and`/`or`/negação). Depois as operações rodam na ordem escrita. `TASK` usa as tarefas como linhas, com os campos da página por baixo, e mostra subtarefas sob a tarefa-mãe. O resultado sai como JSON com datas e durações já formatadas (formato Luxon das configurações, nomes em inglês ou português).
- **Scripts** (`script/`): cada bloco roda num runtime QuickJS novo, com limite de memória e de tempo (o tempo esperando o usuário num prompt não conta). O `prelude.js` monta `dv` (com `DataArray` por Proxy, que faz o `pages.file.name` funcionar), `tp`, `DateTime`/`Duration` e `moment` sobre uma única função do host (`__host(nome, json)`), que responde do índice em Rust. O `dv.el`/`dv.container` gravam uma árvore de elementos e as tabelas e listas entram nela na ordem em que o script as cria. O Templater é compilado para o corpo de uma função assíncrona (`tR += …`), então as variáveis de `<%* %>` valem nos `<% %>` seguintes. Os prompts e suggesters vão para a janela do app, enquanto a thread do script espera a resposta.

## Storyboard (`core/src/board/`, `src/board_ui.rs`, `src/board_ui/`)

- **Clip Studio** (`core/src/clipstudio.rs`): o arquivo é uma sequência de chunks depois de `CSFCHUNK`; `SQLi` é um banco SQLite (lido com rusqlite) com a árvore de camadas e `Exta` guarda os pixels em blocos zlib de 256×256 (plano alfa + BGRx), ligados ao banco por `Layer.LayerRenderMipmap → Mipmap → MipmapInfo → Offscreen.BlockData`. Pastas são achatadas (visibilidade, opacidade e máscaras descem), máscaras ativas são aplicadas, o papel vira cor, preenchimentos chapados vêm de `GradationFillInfo`. Os testes comparam pixel a pixel com o clip_to_psd quando `ATELIET_CLIP_SAMPLE` e `ATELIET_CLIP_REFERENCE` estão definidos.

## Moodboards (`core/src/moodboard.rs`, `src/images_ui/moodboard.rs`)

Eram o plugin Moodboard do Pictogrep. Os quadros continuam no formato e no lugar do plugin: com biblioteca compartilhada, `data/plugins/moodboard.json` da pasta do Pictogrep (as chaves `index`, `board:<id>` e `prefs`); sem ela, `.obsidian/ateliet/moodboard.json` no cofre. Uma imagem é guardada pelo id do Pictogrep (SHA-256 do caminho absoluto), então os quadros abrem nos dois apps. Cada gravação relê o arquivo e troca só as próprias chaves. O núcleo tem o documento, a geometria (rotação, caixas, alinhamento por guias, organizar, recorte) e a paleta; a interface desenha o quadro com o `Painter` do egui (imagens como malhas com UV, para girar, recortar e espelhar) e usa as prévias do modo imagens no tamanho em que aparecem.
