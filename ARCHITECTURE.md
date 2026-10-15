# Arquitetura do Ateliet

```
┌──────────────────────────── webview (WebKit / WebView2 / WebKitGTK) ────────────────────────────┐
│  plugins da comunidade (main.js)  ──require("obsidian")──▶  ui/obsidian/  (API reimplementada) │
│                                   ──require("@codemirror/*")──▶ mesmas instâncias do editor   │
│                                   ──require("fs"|"path"|…)──▶  ui/obsidian/nodeshim.ts         │
│  ui/app.ts (App) · ui/core/ (core plugins) · ui/editor/ (CM6: sintaxe + Live Preview)          │
└──────────────────────────────┬───────────────────────────────────┬─────────────────────────────┘
                     invoke (IPC Tauri)                  ateliet:// (protocolo próprio)
┌──────────────────────────────▼───────────────────────────────────▼─────────────────────────────┐
│ src/main.rs   comandos: vault, fs, metadados, render, busca, http, processos, janelas          │
│ src/lib.rs    núcleo puro (testável):                                                          │
│   text.rs      índice de linhas com posições UTF-16 (o que o JS espera)                        │
│   syntax.rs    frontmatter, %%comentários%%, [[wikilinks]], #tags, ^ids, callouts, ==realce==  │
│   metadata.rs  markdown → CachedMetadata (links, embeds, tags, headings, sections, listItems…) │
│   render.rs    markdown → blocos HTML com linhas de origem (reading view / post-processors)    │
│   search.rs    busca paralela (rayon) com a sintaxe do Obsidian                                │
│   vault.rs     listagem, stat, cópia                                                           │
│ src/process.rs child_process para plugins (spawn/exec, stdout em eventos)                      │
└─────────────────────────────────────────────────────────────────────────────────────────────────┘
```

## Princípios

1. **O vault é a fonte da verdade.** Nada fica num banco próprio. O índice de metadados é recalculado pelo Rust (em paralelo) a cada abertura e mantido por eventos do watcher.
2. **O contrato é o `obsidian.d.ts`.** Cada classe exportada (Plugin, Vault, Workspace, MetadataCache, MarkdownView, Modal, Setting…) é reimplementada com o mesmo comportamento observável, a mesma estrutura de DOM e os mesmos nomes de classes CSS, porque os temas dependem disso.
3. **Carregamento de plugins idêntico ao do Obsidian.** O `main.js` é avaliado como `function(require, module, exports)`, o export default é instanciado com `new Plugin(app, manifest)` e depois vêm `load()` e `loadCSS()`. O `require` expõe `obsidian`, os pacotes `@codemirror/*` e `@lezer/*` (as mesmas instâncias usadas pelo editor, então extensões de plugins funcionam), os aliases antigos do CM6 e os shims de Node.
4. **Escrita mínima e cirúrgica nos arquivos compartilhados.** As configurações são gravadas com *merge* das chaves alteradas. A lista de plugins preserva a ordem e as entradas desconhecidas. O layout fica num arquivo próprio.
5. **Robustez contra plugins.** Erros de plugins são isolados (tryTrigger, try/catch em post-processors e no estado do editor). Um `TextFileView` só grava depois de o conteúdo do arquivo estar carregado, o que evita sobrescrever uma nota com um editor ainda vazio.

## Fluxo de inicialização (`ui/main.ts`)

1. `vault_init` (Rust): lista arquivos e inicia o watcher do vault.
2. `App` é criado, `app.json`/`appearance.json` são carregados e os core plugins registrados.
3. Tema, snippets e variáveis CSS (fonte, cor de destaque) são aplicados.
4. `vault_metadata_all` (Rust, em paralelo) monta o `MetadataCache` e os links são resolvidos.
5. Os core plugins são ligados conforme o `core-plugins.json`.
6. Os plugins da comunidade carregam, na ordem do `community-plugins.json`.
7. O layout é carregado (`.obsidian/ateliet/workspace.json`, ou o `workspace.json` do Obsidian na primeira vez) e `onLayoutReady` é disparado.

## Resolução de links (`MetadataCache.getFirstLinkpathDest`)

A ordem de tentativa é a mesma do Obsidian:
1. caminho exato (com e sem `.md`);
2. caminho relativo à pasta da nota de origem;
3. por nome, sem diferenciar maiúsculas: se houver vários candidatos, vence o da mesma pasta, senão o de caminho mais curto.

## Live Preview (`ui/editor/livepreview.ts`)

- `lineClasses` (ViewPlugin): classes de linha `HyperMD-header-N`, `HyperMD-list-line`, `HyperMD-quote`, `HyperMD-codeblock`… nos dois modos.
- `livePreviewPlugin` (ViewPlugin, `Prec.low`): esconde a marcação fora do cursor e adiciona widgets de checkbox, bullet, linha horizontal e imagem. A precedência baixa garante que decorações de plugins (por exemplo, as queries inline do Dataview) vençam.
- `blockWidgets` (StateField): callouts e propriedades como widgets de bloco. Decorações que atravessam linhas precisam vir de um StateField.
- A gramática (`ui/editor/syntax.ts`) estende o lezer-markdown e anota cada nó com `tokenClassNodeProp`, usando as classes HyperMD do Obsidian (`inline-code`, `formatting formatting-code`…). Código inline vira nós irmãos planos, como no parser do Obsidian.
- O DOM do editor reproduz o do Obsidian: `.cm-scroller > .cm-sizer > (.inline-title, .cm-contentContainer > (.cm-gutters, .cm-content))`.

## Depuração

Com `ATELIET_DEBUG=<pasta>`, o console do webview é espelhado em `<pasta>/console.log`, o conteúdo de `<pasta>/eval.js` é avaliado no app e o comando `debug_snapshot` gera um PNG da webview (macOS). Fora desse modo, os comandos de depuração ficam inativos.
