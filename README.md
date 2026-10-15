# Ateliet

Um clone minimalista do Obsidian, com núcleo em **Rust**, que abre **o mesmo vault** que o Obsidian e roda os **plugins da comunidade do Obsidian sem modificação**.

- **Leve**: Tauri 2 + a webview nativa do sistema (WebKit no macOS), em vez de um Chromium embutido. O frontend tem cerca de 0,8 MB minificado, e as linguagens de código são carregadas sob demanda.
- **Compatível**: implementa a API pública `obsidian` (o contrato do `obsidian.d.ts`) e as APIs internas mais usadas pelos plugins populares.
- **Customizável**: temas, snippets CSS, atalhos e plugins são lidos de `.obsidian/`, as mesmas pastas e arquivos do Obsidian.

## Rodando

Requisitos: Rust (stable) e [Deno](https://deno.com) (só para empacotar o frontend; não precisa de Node).

```sh
./build.sh            # empacota ui/ → dist/
cargo run -- ~/caminho/do/vault
```

Sem argumento, o Ateliet reabre o último vault ou mostra o seletor, que já lista os vaults conhecidos pelo Obsidian.

App instalável (macOS `.app`/`.dmg`, Windows `.msi`, Linux `.deb`/`.AppImage`):

```sh
cargo install tauri-cli --version "^2"
./build.sh && cargo tauri build
```

## Usando o mesmo vault no Obsidian e no Ateliet

| O quê | Onde fica | Compartilhado? |
| --- | --- | --- |
| Notas, anexos, pastas | o próprio vault | sim |
| Plugins (`main.js`, `data.json`) | `.obsidian/plugins/` | sim |
| Lista de plugins ativos | `.obsidian/community-plugins.json` | sim |
| Core plugins ligados | `.obsidian/core-plugins.json` | sim |
| Preferências | `.obsidian/app.json`, `appearance.json` | sim (merge: chaves desconhecidas são preservadas) |
| Atalhos | `.obsidian/hotkeys.json` | sim |
| Temas e snippets | `.obsidian/themes/`, `.obsidian/snippets/` | sim |
| Favoritos, notas diárias, templates, tipos de propriedade | `bookmarks.json`, `daily-notes.json`, `templates.json`, `types.json` | sim |
| Layout de abas | `.obsidian/ateliet/workspace.json` | **não**: o Ateliet só *lê* o `workspace.json` do Obsidian na primeira abertura |

Mudanças feitas por um app aparecem ao vivo no outro: o watcher em Rust detecta e o vault concilia criação, edição, remoção e renomeação. Quando o Obsidian altera o `data.json` de um plugin, o Ateliet chama `onExternalSettingsChange()` nesse plugin.

## O que funciona

- Editor CodeMirror 6 com **Live Preview** (marcação escondida fora do cursor; checkboxes, bullets, imagens, callouts e propriedades como widgets), modo código-fonte e **modo de leitura** renderizado pelo Rust.
- Wikilinks, embeds (notas com `#heading`/`^bloco`, imagens, áudio, vídeo, PDF), tags, callouts dobráveis, realce, comentários `%% %%`, matemática (MathJax), mermaid, notas de rodapé e tarefas com status personalizados.
- Explorador de arquivos (arrastar para mover, renomear inline, ordenação), busca global em Rust (`path:`, `file:`, `tag:`, `"frase"`, `-exclusão`, `OR`, `/regex/`), Quick Switcher, Paleta de Comandos, backlinks (com menções não linkadas), links de saída, tags, outline, favoritos, notas diárias, templates, contagem de palavras e page preview ao passar o mouse.
- Renomear arquivos **atualiza os links** no vault inteiro.
- Abas, splits, barras laterais redimensionáveis, histórico voltar/avançar por aba e arrastar abas.
- Configurações no estilo Obsidian: editor, arquivos e links, aparência, atalhos (gravando no `hotkeys.json`), core plugins e plugins da comunidade, com **instalação direta do catálogo oficial** de plugins e temas.

Plugins testados num vault real: Dataview (incluindo queries inline no Live Preview), Tasks, Kanban (inclusive criar cartões pelo editor inline), Excalidraw, Templater, Calendar, Style Settings, Make.md, PDF++, Code Styler, Advanced Tables, Novel Word Count, Mermaid Tools, Mehrmaid, ePub Reader, Docxer, Terminal e Blender Viewer.

## Limitações conhecidas

- O editor do Obsidian é um modo CM5 ("hypermd") dentro do CM6. O Ateliet usa o parser lezer-markdown com as mesmas classes de token (`tokenClassNodeProp`), mas extensões que **substituem** a linguagem do editor são ignoradas. Por exemplo, o realce de `<% %>` do Templater não aparece; os comandos e templates dele funcionam.
- Não há Canvas, Graph view, Bases, Sync nem Publish. O Excalidraw recebe um stub mínimo do Canvas para embutir notas.
- Do Node/Electron existem shims, não o ambiente completo: `fs` (síncrono e assíncrono), `path`, `os`, `child_process` (spawn/exec/execSync via Rust, sem PTY), `stream` básico, `electron.shell`/`remote` parciais. Módulos nativos como `node-pty` não funcionam.
- Janelas pop-out abrem como abas.

## Arquitetura

Veja [ARCHITECTURE.md](ARCHITECTURE.md).
