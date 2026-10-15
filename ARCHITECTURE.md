# Arquitetura do Ateliet

```
┌──────────────────────────── webview (WebKit / WebView2 / WebKitGTK) ────────────────────────────┐
│  plugins da comunidade (main.js)  ──require("obsidian")──▶  ui/obsidian/  (API reimplementada) │
│                                   ──require("@codemirror/*")──▶ mesmas instâncias do editor   │
│                                   ──require("fs"|"path"|…)──▶  ui/obsidian/nodeshim.ts         │
│  ui/app.ts (App) · ui/core/ (core plugins) · ui/editor/ (CM6: sintaxe + Live Preview)          │
│  ui/images/ (biblioteca de imagens: modos escrita/imagens, galeria, coleções, painel)          │
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
│   images.rs    biblioteca de imagens: varredura, índice persistente, busca vetorial e por nome │
│   clip.rs      CLIP: tokenizer BPE, pré-processamento, inferência ONNX (ort)                   │
│ src/process.rs child_process para plugins (spawn/exec, stdout em eventos)                      │
│ src/gallery.rs indexador em segundo plano por janela, download do modelo, comandos images_*    │
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

## Biblioteca de imagens (`src/images.rs`, `src/clip.rs`, `src/gallery.rs`, `ui/images/`)

- **O vault é a biblioteca.** Toda imagem do vault (jpg, png, webp, gif) faz parte dela; as coleções são as pastas. Pastas externas entram por configuração e são lidas no lugar, com chaves em caminho absoluto.
- **Índice** (`.obsidian/ateliet/images/index.bin`, formato `ATI1`): por imagem, mtime, tamanho, dimensões, digest SHA-256 e o vetor CLIP de 512 dimensões. Ele é gravado de forma atômica. Vetores de outro modelo são descartados na leitura. Um arquivo novo cujo digest bate com o de um registro órfão (renomeado ou movido) herda o vetor.
- **Indexador** (`Gallery::work`): uma thread por janela analisa as imagens pendentes em lotes de 8. Para cada imagem: decodificação com orientação EXIF, miniatura de 480 px gravada no cache, 224×224 normalizado e o modelo de visão. A varredura inicial reaproveita as dimensões já gravadas. Mudanças no vault chegam pelos eventos do `Vault` (JS → `images_refresh`); as pastas externas têm um watcher próprio.
- **Busca**: a frase vira vetor pelo modelo de texto (com cache), e o ranking é o produto escalar com um pequeno bônus para nomes de arquivo que batem. Imagens ainda não analisadas aparecem no fim quando o nome bate. "Semelhantes" usa o vetor da própria imagem.
- **Miniaturas**: `ateliet://…/__thumb?path=…&w=…`, com cache em disco cujo nome inclui mtime e tamanho (mudou, invalida).
- **Modos**: `App.layoutFile` aponta para o layout do modo atual. A troca salva as notas abertas e o layout do modo que sai, depois carrega o do outro.
- **Modelo**: `Xenova/clip-vit-base-patch32` (q8) numa revisão fixa, baixado para `<dados do app>/ateliet/models/`. `ATELIET_CLIP_DIR` aponta para outra pasta com `tokenizer.json`, `text_model.onnx` e `vision_model.onnx`. Os testes de `clip.rs` rodam contra arquivos reais quando `ATELIET_CLIP_TEST_DIR` está definido.

## Depuração

Com `ATELIET_DEBUG=<pasta>`, o console do webview é espelhado em `<pasta>/console.log`, o conteúdo de `<pasta>/eval.js` é avaliado no app e o comando `debug_snapshot` gera um PNG da webview (macOS). Fora desse modo, os comandos de depuração ficam inativos.
