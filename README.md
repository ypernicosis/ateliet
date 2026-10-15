# Ateliet

Um clone minimalista do Obsidian, com núcleo em **Rust**, que abre **o mesmo vault** que o Obsidian e roda os **plugins da comunidade do Obsidian sem modificação**.

- **Leve**: Tauri 2 + a webview nativa do sistema (WebKit no macOS), em vez de um Chromium embutido. O frontend tem cerca de 0,8 MB minificado, e as linguagens de código são carregadas sob demanda.
- **Compatível**: implementa a API pública `obsidian` (o contrato do `obsidian.d.ts`) e as APIs internas mais usadas pelos plugins populares.
- **Customizável**: temas, snippets CSS, atalhos e plugins são lidos de `.obsidian/`, as mesmas pastas e arquivos do Obsidian.

## Rodando

Requisitos: Rust (stable) e [Deno](https://deno.com) (só para empacotar o frontend; não precisa de Node). Na primeira compilação, o crate `ort` baixa o ONNX Runtime (usado pela busca de imagens) e o liga estaticamente ao binário. Sem acesso a esses binários, compile com `--no-default-features --features ort-dynamic` e aponte `ORT_DYLIB_PATH` para uma `libonnxruntime` instalada no sistema.

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

## Instalando no macOS

O Ateliet é compilado a partir deste repositório, e o botão de atualizar usa o mesmo clone depois. Por isso, guarde o clone num lugar fixo (o exemplo usa `~/Developer/ateliet`) e não o apague nem mova depois de instalar.

```sh
# 1. Ferramentas da Apple (git, compilador). Pule se já estiverem instaladas.
xcode-select --install

# 2. Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# 3. Deno (empacota a interface)
curl -fsSL https://deno.land/install.sh | sh -s -- -y
export PATH="$HOME/.deno/bin:$PATH"

# 4. CLI do Tauri (gera o .app)
cargo install tauri-cli --version "^2" --locked

# 5. O código
mkdir -p ~/Developer && cd ~/Developer
git clone https://github.com/ypernicosis/ateliet.git
cd ateliet

# 6. Compilar (a primeira vez leva alguns minutos)
./build.sh && cargo tauri build --bundles app

# 7. Instalar e abrir
rm -rf /Applications/Ateliet.app
cp -R target/release/bundle/macos/Ateliet.app /Applications/
open /Applications/Ateliet.app
```

Se você já tem um clone, troque os passos 5 a 7 por: feche o Ateliet, entre na pasta do clone, rode `git checkout main && git pull` e depois os passos 6 e 7.

## Atualizando

Na parte de baixo da faixa lateral esquerda fica o botão de atualizar (uma seta para baixo num círculo, também disponível como comando *Update Ateliet*). O Ateliet roda `git fetch` no clone ao abrir e a cada 30 minutos; quando há commits novos no branch acompanhado, o botão fica colorido com um ponto. Ao clicar, ele lista os commits novos e, em *Update and restart*:

1. roda `git pull --ff-only` no clone;
2. empacota a interface (`./build.sh`);
3. compila (`cargo build --release`);
4. troca o executável do app instalado (dentro do `.app`) e reinicia, salvando as notas abertas antes.

O progresso aparece na janela. Se um passo falhar, a mensagem mostra o erro e dá para tentar de novo; o app instalado só é trocado quando tudo compilou. Sem commits novos, o mesmo botão oferece *Rebuild anyway*. O app guarda o caminho do clone em que foi compilado: se você mover o clone, compile e instale de novo a partir do novo lugar (passos 6 e 7).

## Benchmark

`scripts/bench.sh ~/caminho/do/vault` (macOS) abre o mesmo vault no Ateliet e no Obsidian e mede tamanho do app, tempo até a janela aparecer, memória e CPU parado.

## Escrita e imagens

O Ateliet tem dois modos sobre o mesmo vault, alternados pelos dois botões no topo da faixa lateral esquerda ou por `Ctrl/Cmd + Shift + I`:

- **Modo escrita**: as notas no centro. O painel **Pictures** na barra lateral direita busca imagens do vault; clicar insere a imagem no cursor e arrastar solta em qualquer ponto da nota. O comando **Insert picture…** abre a mesma busca num diálogo.
- **Modo imagens**: a biblioteca no centro (mosaico com todas as imagens do vault), as **coleções** à esquerda (as pastas que têm imagens, com capa e contagem) e, à direita, a nota em que você estava escrevendo, editável ali mesmo.

Cada modo guarda seu próprio layout (`.obsidian/ateliet/workspace.json` e `workspace-images.json`).

**Busca por descrição.** Escreva o que a imagem mostra ("mãos segurando uma xícara", "rua à noite com chuva") e as mais parecidas aparecem primeiro. Usa o modelo CLIP (`Xenova/clip-vit-base-patch32`, quantizado em 8 bits), baixado uma única vez (cerca de 150 MB) para a pasta de dados do app. A análise roda neste computador, em segundo plano, poucas imagens por vez, e nenhuma imagem sai da máquina. Os vetores ficam em `.obsidian/ateliet/images/index.bin`. Imagens renomeadas ou movidas são reconhecidas pelo conteúdo e não são analisadas de novo. Antes do download, a busca procura pelos nomes dos arquivos.

**Imagens e notas se cruzam.** O visualizador mostra as notas que usam a imagem e as imagens parecidas com ela. Também dá para inserir a imagem na nota aberta ou criar uma nota nova com ela, e cada coleção pode ter uma nota própria (`<pasta>/<pasta>.md`). No explorador de arquivos, o menu de uma imagem tem "Find similar pictures".

**Trazer imagens.** Arrastar arquivos do sistema ou imagens de um navegador para a biblioteca, ou colar uma imagem ou um link de imagem, salva tudo na pasta aberta (ou na pasta configurada). Pastas fora do vault podem entrar na biblioteca (Settings → Images). Elas ficam onde estão e só são copiadas para o vault quando a imagem é usada numa nota. Arrastar imagens para uma coleção move os arquivos, e os links nas notas são atualizados.

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
| Biblioteca de imagens (layout do modo imagens, ajustes, índice da busca) | `.obsidian/ateliet/workspace-images.json`, `images.json`, `images/index.bin` | **não**: só o Ateliet usa |

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
