# Leitor: arquitetura

O Leitor abre livros em EPUB, documentos em PDF e quadrinhos (CBZ, CBR, CB7 e pastas de imagens) dentro do Ateliet. Ele tem dois compromissos. O primeiro é ser imersivo e minimalista: quando se lê, só a página aparece. O segundo é servir de fonte para o Journal ([JOURNAL.md](JOURNAL.md)): todo trecho pode ser selecionado e guardado, e toda região de página pode ser recortada e posta numa página do Journal.

O Leitor está implementado; a seção 10 diz o que foi feito e onde o código se afastou deste plano. Este documento passa a vir antes do Journal na ordem de trabalho. O Leitor é útil sozinho (lê, grifa, guarda trechos e recortes no vault) e entrega ao Journal o material que ele vai organizar.

## 1. O que já existe no Ateliet

| Peça | Onde | Uso no Leitor |
|---|---|---|
| Desenho de PDF | `core/src/media.rs`, crate `hayro` (Rust puro) | Páginas de PDF. Hoje cada chamada reabre o arquivo; o Leitor mantém o `Pdf` aberto |
| Texto do PDF | `hayro-interpret`: o traço `Device` recebe cada glifo com sua transformação, e `as_unicode()` dá o caractere | Camada de texto para selecionar e copiar |
| ZIP | `core/src/board/zip.rs` | EPUB e CBZ. Hoje ele descompacta tudo de uma vez; o Leitor precisa de um leitor que liste as entradas e extraia só a pedida |
| Compositor | `core/src/typeset/` (Knuth–Plass, hifenização em português, fontes embutidas, rasterização) | Paginar o EPUB com a mesma tipografia da Diagramação |
| Imagens | crate `image`, carregador de prévias do modo imagens (`src/images_ui.rs`) | Páginas de quadrinhos, cache e pré-carregamento |
| Recorte | `moodboard::CropStage`, ferramenta de quadros do storyboard | Recorte de região |
| Temas | `core/src/css_theme.rs`, `src/typo.rs` | Cores e fontes do leitor seguem o tema, com papéis próprios por cima |
| Tela cheia | `ViewportCommand::Fullscreen`, já usado no modo foco do storyboard | Modo imersivo |

## 2. Visão geral

```
┌─────────────────────────────── src/reader_ui/ (interface) ───────────────────────────────┐
│ mod.rs     estado: documento aberto, posição, vista, seleção, cache de páginas            │
│ chrome.rs  a moldura que some: barra de cima, linha de progresso, sumário, Aa, busca      │
│ flow.rs    EPUB paginado ou em rolagem   pdf.rs   páginas em blocos   comic.rs   HQ       │
│ select.rs  seleção de texto e o menu dela    crop.rs   recorte de região                  │
│ shelf.rs   estante (capas, continuar lendo)  side.rs  leitor ao lado do Journal            │
└────────────────────────────────────────────┬─────────────────────────────────────────────┘
                                             │
┌────────────────────────────────────────────▼─────────────────────────────────────────────┐
│ core/src/reader/                                                                          │
│ mod.rs      o traço Document, metadados, sumário, Locator                                 │
│ epub.rs     container, OPF, espinha, nav/NCX     xhtml.rs  XHTML + CSS mínimo → blocos     │
│ paginate.rs blocos → páginas pelo typeset       pdf.rs    páginas e camada de texto       │
│ comic.rs    CBZ, CB7, CBR, pastas; ComicInfo    archive.rs leitura sob demanda            │
│ marks.rs    grifos e posição no vault           capture.rs trechos e recortes → Snippet   │
└───────────────────────────────────────────────────────────────────────────────────────────┘
```

Tudo o que é pesado roda fora da thread da janela, como no resto do Ateliet: abrir e indexar o arquivo, compor capítulos, desenhar páginas de PDF, descompactar imagens. A janela pede uma página e a desenha quando ela chega; até lá mostra a página anterior em baixa resolução ou o papel vazio, nunca um indicador de carregamento piscando.

## 3. O modelo comum (`core/src/reader/mod.rs`)

Os três formatos ficam atrás de um traço, e a interface não sabe qual está desenhando, exceto onde a diferença importa (texto que reflui ou página fixa):

```rust
pub trait Document: Send {
    fn meta(&self) -> &Meta;                 // título, autor, idioma, capa, direção (ltr/rtl)
    fn toc(&self) -> &[TocEntry];            // sumário, com Locator de cada entrada
    fn kind(&self) -> Kind;                  // Reflow (EPUB) ou Fixed (PDF, HQ)
    fn pages(&self, view: &View) -> PageCount;          // depende do tamanho e da fonte no EPUB
    fn render(&self, page: usize, view: &View, scale: f32) -> Result<Rgba, String>;
    fn text(&self, page: usize, view: &View) -> Option<TextLayer>;  // None numa HQ ou PDF escaneado
    fn locate(&self, at: &Locator, view: &View) -> usize;           // Locator → página atual
    fn locator(&self, page: usize, view: &View) -> Locator;         // página atual → Locator
}

pub struct TextLayer { pub glyphs: Vec<Glyph>, pub lines: Vec<Line> }
pub struct Glyph { pub ch: String, pub quad: [f32; 4], pub line: u32 }
```

**Posição.** A posição de leitura nunca é guardada como número de página, porque no EPUB a página muda com o tamanho da janela e da fonte. Ela é um `Locator`:

```rust
pub enum Locator {
    Epub  { spine: usize, path: String, offset: usize, quote: Quote }, // caminho no XHTML, como o EPUB CFI
    Pdf   { page: usize, quads: Vec<[f32; 4]>, quote: Option<Quote> },
    Comic { page: usize, rect: Option<[f32; 4]> },                      // região em 0..1
}
pub struct Quote { pub exact: String, pub before: String, pub after: String }
```

A citação com algumas palavras antes e depois permite reencontrar um trecho quando o arquivo muda (outra edição, outro EPUB do mesmo livro).

## 4. Os formatos

### 4.1 EPUB (`epub.rs`, `xhtml.rs`, `paginate.rs`)

- **Leitura**: `META-INF/container.xml` aponta o OPF; o OPF dá metadados, manifesto e espinha; `nav.xhtml` (EPUB 3) ou `toc.ncx` (EPUB 2) dá o sumário. As entradas do ZIP são lidas sob demanda.
- **XHTML em blocos**: um leitor de XML (`quick-xml`) transforma cada documento da espinha em blocos que o compositor já conhece ou passa a conhecer: parágrafo, título, citação, lista, imagem, separador, verso (linhas preservadas), tabela simples, nota de rodapé. Ênfase, negrito, versalete, sobrescrito e links ficam como estilos de trecho.
- **CSS**: só o que muda a leitura: itálico, negrito, versalete, alinhamento, recuo da primeira linha, margens de bloco, `text-align: center` em aberturas e epígrafes, quebra de página antes de títulos. Fontes, cores e tamanhos do livro são ignorados por padrão (o leitor usa os seus). Uma opção "Estilo do livro" liga as fontes embutidas do EPUB, para livros em que a tipografia faz parte da obra.
- **Paginação**: os blocos de cada capítulo passam pelo compositor do Ateliet (`typeset`), com justificação por Knuth–Plass e hifenização no idioma do livro, na largura e na altura da página atual. A composição de um capítulo roda em segundo plano e é guardada em cache por (capítulo, largura, altura, fonte, tamanho); mudar a fonte recompõe e mantém a posição pelo `Locator`. Existe também o modo rolagem contínua, sem páginas.
- **Notas de rodapé**: links com `epub:type="noteref"` abrem a nota numa janelinha sobre o texto, sem sair da página.
- **EPUB de layout fixo** (livros infantis, alguns quadrinhos): quando cada item da espinha é uma página com uma imagem só, o livro é lido como uma HQ (4.3). Layout fixo com HTML arbitrário não é desenhado com fidelidade, porque o Ateliet não tem um motor de HTML; ele cai para o refluxo, com um aviso.
- **DRM**: conteúdo cifrado (`META-INF/encryption.xml` com algoritmo de DRM) é recusado com uma mensagem clara. Os formatos da Amazon (AZW, KFX) não são lidos.

### 4.2 PDF (`pdf.rs`)

- **Desenho**: o `hayro` desenha cada página na escala da tela. Em zoom alto, a página é desenhada em blocos (o egui tem limite de tamanho de textura), só os visíveis. O documento analisado fica em memória enquanto está aberto.
- **Camada de texto**: um `Device` próprio passa pela página sem desenhar e anota cada glifo: o caractere (`as_unicode()`) e o quadrilátero (pela transformação do glifo e as métricas da fonte). Os glifos são agrupados em linhas pela linha de base e em blocos por distância, o que dá uma ordem de leitura razoável em uma ou duas colunas. Hifens de fim de linha são juntados ao copiar ("pala-/vra" vira "palavra"), e ligaduras (`ﬁ`) são desfeitas.
- **PDF escaneado** (sem texto): a página se lê e se recorta normalmente, mas não se seleciona texto. A transcrição do Journal (JOURNAL.md, seção 9) pode gerar uma camada de texto depois, página por página.
- **Imersão**: o leitor detecta as margens brancas de cada página e oferece "cortar margens", que amplia o texto no espaço da tela; página simples ou dupla; ajuste à largura ou à página.
- **Sumário**: os marcadores (*outlines*) do PDF; sem eles, só a busca e a lista de páginas.

### 4.3 Quadrinhos (`comic.rs`, `archive.rs`)

- **Arquivos**: CBZ pelo leitor de ZIP sob demanda; CB7 pelo `sevenz-rust` (Rust puro); CBR pelo `bsdtar` do sistema (a libarchive lê RAR4 e RAR5 e já vem no macOS), chamado como o Ateliet já chama o `ffmpeg`, ou pelo crate `unrar` numa feature opcional para quem não tiver o `bsdtar`. Muitos `.cbr` são ZIPs renomeados, então o formato é decidido pelos primeiros bytes, não pela extensão. Uma pasta de imagens também abre como HQ.
- **Ordem e metadados**: páginas em ordem natural de nome (`2` antes de `10`); o `ComicInfo.xml`, quando existe, dá título, série, número, autores e a direção (`Manga="YesAndRightToLeft"` liga a leitura da direita para a esquerda).
- **Vistas**: página simples, página dupla (imagens mais largas que altas são tratadas como páginas duplas inteiras), da direita para a esquerda para mangá, e rolagem vertical contínua para webtoons, escolhida automaticamente quando as páginas são muito altas.
- **Cache**: as páginas vizinhas são descompactadas e decodificadas antes de serem pedidas; o cache guarda poucas páginas em resolução cheia e o resto em miniatura, para que um arquivo de 500 MB não ocupe 500 MB de memória.
- **Leitura por quadro** (fase posterior): detectar os quadros pelas medianizes brancas ou pretas e avançar de quadro em quadro com zoom, como a "Guided View". A geometria de quadros do storyboard (`board/pages.rs`) serve de base.

## 5. A interface imersiva

O Leitor abre como uma aba, como o Quadro, e qualquer `.epub`, `.pdf`, `.cbz`, `.cbr` ou `.cb7` no explorador abre nele. Ao começar a ler, a moldura some.

- **Só a página**: as barras laterais, as abas, a barra de status e a faixa de modos se recolhem (`Ctrl/Cmd+Enter` ou `F` alternam; `Esc` volta). Opcionalmente, a janela vai para tela cheia. O fundo é o papel, sem bordas nem sombras de cartão.
- **Moldura que aparece e some**: mover o ponteiro para o alto mostra uma linha fina com o título, o sumário, a busca, o botão de aparência (Aa) e o de recorte; para baixo, a posição. Ela se esconde depois de dois segundos parada. Nada mais fica na tela enquanto se lê.
- **Progresso**: uma linha de 1 px no pé, quase invisível, com o capítulo e o tempo estimado para o fim do capítulo só quando o ponteiro passa por ela. O tempo vem da velocidade de leitura medida no próprio livro.
- **Virar a página**: setas, espaço, clique nas margens laterais, gesto no trackpad e roda do mouse (no modo rolagem). A troca é imediata ou com um deslize curto; não há animação de folha virando.
- **Aparência (Aa)**: papel claro, sépia, escuro e o do tema do vault; fonte (as embutidas da Diagramação, as do sistema, a do livro); tamanho, entrelinha, margens, justificado ou alinhado à esquerda, hifenização; página simples, dupla ou rolagem. As escolhas valem por formato e podem ser fixadas por livro.
- **Estante** (`shelf.rs`): a primeira tela quando não há livro aberto. Capas em grade, "Continuar lendo" no topo, filtro por formato e por pasta. Os livros vêm das pastas escolhidas em Configurações › Leitor (no vault ou fora dele). Sem números nem gráficos: a Biblioteca do Scriptorium continua sendo o lugar dos números.
- **Sumário e busca**: painéis que deslizam por cima da página e somem ao escolher um item.

O estado do leitor (último livro, posição de cada livro, preferências) fica em `.obsidian/ateliet/leitor.json`, com cada livro identificado pelo hash do início do arquivo e pelo tamanho, para a posição sobreviver a renomear ou mover o arquivo.

## 6. Selecionar e recortar para o Journal

### 6.1 Seleção de texto (`select.rs`)

Funciona no EPUB e no PDF com texto. Arrastar seleciona, encaixando em palavras; duplo clique pega a palavra e triplo, o parágrafo; Shift+clique estende. Ao soltar, um menu pequeno aparece sobre a seleção, só com ícones e um rótulo ao passar o ponteiro:

- **Grifar**, com quatro cores. O grifo fica no livro (6.3).
- **Nota**: grifa e abre um campo curto para escrever ao lado.
- **Journal**: guarda o trecho como `Snippet` (JOURNAL.md, seção 8): uma nota em `Trechos/` com o texto num callout `[!trecho]`, a fonte no frontmatter e o `Locator`. Se uma página do Journal estiver aberta ao lado (6.4), o trecho já entra nela como item de trecho citado.
- **Copiar**: o texto, ou o texto com a citação ("…" — Autor, *Título*, cap./p.), à escolha em Configurações.

### 6.2 Recorte de região (`crop.rs`)

Funciona nos três formatos e é a forma principal de levar imagens: um quadro de HQ, uma figura de um PDF, uma ilustração ou um parágrafo de um EPUB guardado como imagem, com a tipografia do livro.

- A ferramenta de recorte (`R`, ou o ícone na moldura) escurece levemente a página. Arrastar desenha um retângulo; com `Alt`, um laço livre. As bordas do retângulo se ajustam depois, com a proporção travada por `Shift`.
- **Resolução**: o recorte não vem da tela. No PDF, a região é desenhada de novo pelo `hayro` em alta escala (300 ppp ou mais); na HQ, sai da imagem original, sem reamostragem; no EPUB, a página é composta de novo em escala alta. O recorte fica nítido no Journal e na exportação em PDF dele.
- **Imagem do EPUB**: clicar com o botão direito numa imagem do livro oferece "Mandar ao Journal" com o arquivo original, sem recortar.
- **Destino**: o PNG vai para `Anexos/Recortes/<Livro>/<livro> p<página>-<n>.png` (pasta configurável), e um `Snippet` com o embed e o `Locator` vai para `Trechos/`, como no texto. No PDF, o texto que cai dentro da região vai junto, como transcrição, para a busca encontrar o recorte.
- No Journal, o recorte chega como um item de imagem com a origem preservada, e as ferramentas de recorte do Journal (máscara, seleção automática, borda de adesivo) trabalham sobre ele.

### 6.3 Grifos e posição no vault (`marks.rs`)

Pelo princípio do Ateliet (o vault é a fonte da verdade), os grifos não ficam num banco escondido. Cada livro lido ganha, no primeiro grifo, uma nota de livro (pasta `Livros/`, configurável; se já houver uma nota com o mesmo título ou ISBN, é ela):

```markdown
---
leitor: livro
titulo: Sapiens
autor: Yuval Noah Harari
arquivo: "Livros/Arquivos/Sapiens.epub"
---

> [!grifo|amarelo] cap. 3 · p. 58
> O trigo domesticou o homem, e não o contrário.

^g-7k2

> [!grifo|azul] cap. 4
> …
>
> Nota: comparar com o trecho do Mossy Earth sobre pastoreio.

^g-9qa

%% ateliet:leitor
{"v":1,"marks":{"g-7k2":{"loc":{…},"cor":"amarelo","criado":"2026-10-29T14:02"}}}
%%
```

Assim os grifos aparecem no Obsidian, entram na busca e no Dataview, e podem ser ligados de qualquer nota (`[[Sapiens#^g-7k2]]`). Apagar o callout no Obsidian apaga o grifo no Leitor. A posição de leitura fica no `leitor.json`, não na nota, para não reescrever a nota a cada página virada.

### 6.4 O Leitor ao lado do Journal (`side.rs`)

A interação que justifica o leitor estar dentro do Ateliet: o livro de um lado, a página do Journal do outro. O comando "Ler ao lado" divide a janela; a parte do livro continua imersiva (sem moldura) e a do Journal mostra a página. Um trecho selecionado ou um recorte pode ser arrastado diretamente para o ponto da página onde deve ficar, em vez de ir para a caixa de Trechos e ser posto depois. No sentido contrário, clicar na origem de um item no Journal abre o livro naquele ponto, na metade do leitor.

Enquanto o Journal não existir, o Leitor funciona sozinho: os trechos e os recortes vão para `Trechos/` e `Anexos/Recortes/` e podem ser usados em qualquer nota, no Quadro ou num moodboard.

## 7. Convivência com o vault e os plugins

- **Obsidian**: a nota de livro e as notas de trecho são Markdown comum. O Obsidian abre os PDFs sozinho, e os recortes são PNGs comuns.
- **Dataview**: `leitor: livro`, `autor` e os campos dos trechos permitem consultas como "livros com grifos este mês" e "trechos ainda sem página no Journal".
- **Templater**: a nota de livro é criada a partir de um modelo, se houver um em Configurações › Leitor, com os metadados do livro disponíveis no modelo.
- **Scriptorium**: o EPUB que o Scriptorium exporta abre no Leitor, para revisar o próprio livro como leitor.

## 8. Ordem de implementação

1. **A moldura e as HQs**: o traço `Document`, a aba do leitor, o modo imersivo, a estante, CBZ e pastas de imagens, página simples e dupla, da direita para a esquerda, posição em `leitor.json`. As HQs vêm primeiro porque são só imagens e põem de pé tudo o que os outros formatos usam.
2. **Recorte de região** nas HQs, com o `Snippet` e as pastas `Trechos/` e `Anexos/Recortes/`. A partir daqui, o Leitor já alimenta o Journal.
3. **PDF**: desenho com cache e blocos, camada de texto, seleção, cópia, cortar margens, sumário, recorte em alta resolução.
4. **EPUB**: leitura do arquivo, XHTML em blocos, paginação pelo compositor, notas de rodapé, aparência (Aa), rolagem contínua.
5. **Grifos e nota de livro**, nos três formatos.
6. **CBR e CB7**, webtoon, cortar margens automaticamente, leitura por quadro.
7. **Ler ao lado do Journal**, com arrastar trechos e recortes, depois da fase 1 do Journal.

## 9. Dependências novas e riscos

- **Crates**: `quick-xml` (EPUB), `sevenz-rust` (CB7) e, numa feature opcional, `unrar`. O CBR usa o `bsdtar` do sistema por padrão, para não trazer o código do unRAR (que tem licença própria) ao binário.
- **Ordem do texto no PDF**: o agrupamento em linhas e blocos acerta páginas de uma e duas colunas; diagramações complexas (jornais, revistas) podem dar uma ordem errada na seleção. O recorte de região continua funcionando nelas.
- **Fidelidade do EPUB**: sem motor de HTML, tabelas complexas, layout fixo com HTML e CSS elaborado não ficam iguais ao original. A escolha é deliberada: o Leitor recompõe o texto com a tipografia do Ateliet, em vez de imitar um navegador.
- **Memória**: PDFs e HQs grandes pedem cache com limite e leitura sob demanda; o leitor de ZIP atual, que descompacta tudo, não serve e precisa de uma versão por entrada.
- **DRM**: livros com DRM não abrem.

## 10. Estado da implementação

O núcleo está em `core/src/reader/` (`archive`, `xml`, `epub`, `flow`, `pdf`, `comic`, `marks`, `state`), com testes; a interface em `src/reader_ui/` (`mod`, `worker`, `chrome`, `keep`, `shelf`) e `src/reader_mode.rs`. O que já funciona:

- **Abrir**: qualquer `.epub`, `.pdf`, `.cbz`, `.cbr`, `.cb7` ou `.cbt` clicado no explorador, num link (`[[livro.epub]]`) ou no botão "abrir" de um PDF embutido abre numa aba do Leitor, e não mais no app do sistema. O formato é decidido pelos primeiros bytes (um `.cbr` que é zip abre como zip; um zip com `container.xml` é EPUB). A estante (botão de livro na faixa lateral, ou "Leitor: Abrir a estante") mostra os livros do vault com capa e "Continuar lendo". A posição de cada livro fica em `.obsidian/ateliet/leitor.json`, pela impressão digital do arquivo.
- **EPUB**: pacote, espinha, sumário por `nav.xhtml` ou `toc.ncx`, CSS mínimo (itálico, negrito, versalete, maiúsculas, alinhamento, recuo, margens de bloco, quebra de página, `display: none`, seletores de descendente), notas de rodapé em `<aside epub:type="footnote">` fora do texto e abertas num balão pela referência (também referências curtas numéricas sem `epub:type`), imagens, listas, citações, `<pre>`, tabelas simples (células em linha), recusa de DRM, layout fixo de imagens lido como HQ. A paginação usa o compositor da Diagramação (Knuth–Plass e hifenização) na largura e altura da janela, com cache por capítulo; mudar fonte, tamanho ou janela recompõe e mantém o ponto pelo `Locator`.
- **PDF**: desenho pelo hayro, camada de texto por um `Device` que anota cada glifo (inclusive texto invisível de PDFs com OCR), espaços inferidos pela distância, hifens de fim de linha juntados ao copiar, ligaduras desfeitas, sumário pelos marcadores (destinos diretos e nomeados), links internos e externos, cortar margens, ajuste à página ou à largura, zoom (Ctrl/Cmd+roda ou pinça) e rolagem dentro da página.
- **Quadrinhos**: CBZ lido entrada por entrada; CBR, CB7 e CBT descompactados uma vez numa pasta de cache pelo `bsdtar` (já vem no macOS como `tar`), ou pelo `unrar`/`unar`/`7z` quando não houver; ordem natural; `ComicInfo.xml` (título, série, número, autores, mangá da direita para a esquerda); pastas do arquivo como sumário.
- **Moldura**: barra que aparece com o ponteiro no alto (estante, sumário, busca, grifos, aparência, recorte, ler ao lado, imersivo), linha de progresso no pé com o capítulo e as páginas que faltam, modo imersivo (`F` ou `Ctrl/Cmd+Enter`) que esconde a faixa, as barras laterais, as abas e a barra de status. Página simples ou dupla (automática em janela larga), setas, espaço, PageUp/PageDown, clique nas margens, gesto no trackpad e roda.
- **Aparência (Aa)**: papel claro, sépia, escuro ou do tema; fonte (as embutidas e as do sistema), tamanho, entrelinha, margens, justificado, hifenização; uma ou duas páginas; copiar com citação. As escolhas valem por formato.
- **Busca** no livro inteiro, com as ocorrências marcadas na página.
- **Seleção**: arrastar (encaixando em palavras), duplo clique (palavra), triplo (parágrafo), Shift para estender. O menu sobre ela grifa em quatro cores, abre uma nota, manda ao Journal, copia e, com um caderno aberto, tem uma alça para arrastar o trecho até a página do Journal.
- **Grifos** na nota do livro em `Livros/`, como na seção 6.3, criada no primeiro grifo ou trecho, com um link "Abrir no Leitor". Clicar num grifo o seleciona para trocar a cor, escrever a nota ou removê-lo. A nota é relida quando muda no disco: apagar o callout no Obsidian apaga o grifo; mudar a cor no cabeçalho muda a cor.
- **Trechos e recortes**: o trecho vai para `Trechos/` com a fonte, o link da nota do livro e o `Locator` no frontmatter (`local`). O recorte (`R` ou o botão) é desenhado de novo em cerca de 300 ppp (PDF pelos pontos da página, EPUB composto de novo em escala alta, HQ na imagem original), gravado em `Anexos/Recortes/<Livro>/` e guardado como trecho com a imagem e, quando há texto na região, a transcrição. Botão direito numa imagem do EPUB manda o arquivo original.
- **Ler ao lado**: o livro numa metade da janela do Journal (botão de livro na barra do Journal, botão na barra do Leitor, ou "Leitor: Ler ao lado do Journal"). Um trecho ou recorte mandado de lá entra na página aberta; arrastado pela alça, entra no ponto onde é solto. No sentido contrário, o balão de um trecho ou de uma imagem posta pelo Leitor tem "Abrir no Leitor", que abre o livro ao lado no ponto de origem (o trecho é achado pelas `fontes` da página).
- **Ler ao lado das notas**: o mesmo leitor à direita das notas (botão de estante na barra de abas, botão na barra do Leitor, ou "Leitor: Ler ao lado das notas"); a aba que mostrava o livro fecha e o painel direito se esconde para dar lugar a ele. As notas continuam podendo ficar em dois painéis. Um trecho ou recorte mandado de lá ("Pôr na nota") é guardado em `Trechos/` como sempre e entra como `![[trecho]]` na nota em uso, no cursor.

Onde o código se afastou do plano:

- O `Locator` é uma estrutura só, não um enum: `arquivo`, `capitulo` e `posicao` (EPUB), `pagina` (PDF e HQ, a partir de 1), `regiao` e `citacao`. No EPUB a posição conta letras do capítulo sem os espaços, e não um caminho no XHTML como o EPUB CFI; no PDF, `posicao` é o índice do glifo na página. A citação reencontra o ponto quando o arquivo muda.
- O `^id` do grifo fica numa linha própria depois do callout, que é onde o Obsidian reconhece o id de um bloco complexo, e não no fim do cabeçalho.
- As preferências ficam no painel Aa (e no `leitor.json`), não em Configurações › Leitor. As pastas são fixas (`Livros/`, `Trechos/`, `Anexos/Recortes/`) e a estante só mostra livros do vault.
- O tempo estimado até o fim do capítulo virou o número de páginas que faltam.

Ainda não feito: o laço livre no recorte, a opção "Estilo do livro" com as fontes do EPUB, o modo de rolagem contínua do EPUB, a leitura vertical de webtoons e a leitura por quadro, tela cheia de verdade, o modelo do Templater para a nota do livro, pastas fora do vault na estante e o `crate` `unrar` opcional.

## Fontes consultadas

- Código do `hayro-interpret` 0.8 (traço `Device`, `Glyph::as_unicode`), em `~/.cargo/registry` depois de `cargo fetch`.
