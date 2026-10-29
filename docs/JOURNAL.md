# Journal: estudo e arquitetura

Este documento parte de uma página de referência: um caderno quadriculado aberto, com um trecho de artigo colado, comentários a lápis, uma pintura recortada e uma foto sobreposta. Ele faz três coisas: lista os elementos dessa página, diz o que o Ateliet já tem para reproduzi-los e propõe a arquitetura do modo Journal e das três entradas que o alimentam: a digitalização de notas manuscritas, os trechos de EPUB e os trechos enviados do Kindle. As ferramentas de imagem seguem o modo de colagem do Pinterest, estudado na seção 3.

## 1. Os elementos da página de referência

A página é uma página dupla de caderno, e quase tudo nela se resume a três ações: colar um texto impresso, escrever à mão em volta dele e recortar imagens para pôr por cima. Os elementos, de baixo para cima:

1. **Papel e página dupla.** Duas páginas quadriculadas lado a lado, com a lombada no meio e a borda do bloco de folhas visível à direita. O papel é o fundo de tudo e continua sob as colagens.
2. **Grade de colunas desenhada à mão.** Uma linha horizontal no alto de cada página, um pouco abaixo da borda, e uma linha vertical que separa uma coluna estreita de margem da coluna larga do texto. Na página da esquerda a margem fica à esquerda; na da direita, à direita. É o esquema do método Cornell: texto principal numa coluna, comentários na outra.
3. **Trecho impresso colado.** Um texto do projeto *The Mountain Birch Project* (Mossy Earth), em serifa, ajustado à largura da coluna e continuado na página seguinte. É um recorte de outra fonte, com começo e fim definidos, e não um texto escrito ali.
4. **Grifos sobre o trecho.** Sublinhados a lápis vermelho em frases específicas ("only a mere 4% exists above altitudes of 400 metres", "Mountain birch woodland in Norway provides a good example of such diversity", "largely put down to excessive levels of herbivore pressures on the land"). São anotações ancoradas em partes do texto.
5. **Citação da fonte, à mão.** No alto da margem esquerda: título, autor e URL do vídeo. É a referência bibliográfica do trecho.
6. **Comentário na margem.** Uma reflexão longa a lápis na coluna estreita, ao lado do trecho a que responde.
7. **Pergunta na margem.** Na margem direita, ao lado do fim do trecho: "I wonder why the altar is in the middle of the woods? We shall never truly know." Uma pergunta em aberto, ligada a uma imagem.
8. **Comentário que contorna a imagem.** Na página da direita, o texto manuscrito sobre a pintura corre em linhas que encurtam onde a imagem começa: o texto se ajusta à forma da imagem.
9. **Imagem principal recortada.** Uma impressão da pintura *Betendes Mädchen mit einem Kranz aus Rosen* (Hubert Salentin, 1882), recortada num retângulo, que passa da margem para a área do texto e vai até a borda inferior.
10. **Imagem sobreposta.** Uma foto de vela num castiçal, menor, colada por cima do canto inferior esquerdo da pintura. As duas se sobrepõem: há ordem de camadas.
11. **Legenda da imagem.** Na margem direita, à mão: artista, ano e título no original, logo acima da pintura.
12. **Crédito com data.** No pé da página: "candle: @mrs.mckennabarry on instagram, 11/5/2023". De onde veio a imagem e quando foi colada.
13. **Adesivo desenhado (anjo).** Uma figura de anjo a nanquim com uma estrela, recortada com uma borda branca irregular, como adesivo de corte especial (*die-cut*), presa na linha vertical da margem.
14. **Adesivo que sai da página.** Um traço decorativo (uma nuvem ou coroa em contorno) com borda branca, que cobre parte do texto e passa da borda superior da página. Os elementos não ficam presos à área da página.
15. **Pequenos ornamentos.** Uma estrela com uma fita junto à legenda, também com borda branca.
16. **Textura de impressão e digitalização.** Granulado, cor de fotocópia, sombra da curvatura do caderno. A página foi fotografada ou digitalizada, e as imagens coladas foram impressas antes de serem recortadas.

Daí saem os tipos de item que o Journal precisa ter: papel, guias e linhas, texto citado com fonte, grifo ancorado, texto livre (digitado ou manuscrito), imagem recortada, adesivo com borda, legenda e crédito, com sobreposição, rotação e saída da página permitidas.

## 2. O que o Ateliet já tem

Boa parte das peças existe e pode ser reaproveitada, sem começar do zero:

| Elemento | Base existente | O que falta |
|---|---|---|
| Papel e página dupla | `core/src/board/pages.rs` (tamanhos de página, DPI, páginas unidas com a lombada no meio), `board_ui/spread.rs` | Papéis quadriculado, pautado, pontilhado e Cornell; papel como textura, não só cor |
| Linhas de margem | Formas e linhas do Quadro (`core/src/quadro/rough.rs`) | Guias de coluna com encaixe |
| Imagem livre no canvas | Moodboards (`core/src/moodboard.rs`): itens com `x, y, w, h, r`, recorte retangular (`CropStage`), espelhar, cinza, guias de alinhamento, quadros, paleta extraída | Máscaras não retangulares, borda de adesivo, filtros de impressão |
| Filtros | `typeset::compose::filter`: `pb`, `sepia`, `noite`, `contraste`, `desfoque` | Granulado, fotocópia, retícula, papel envelhecido |
| Desenho à mão | Pincéis do storyboard (`core/src/board/brush.rs`): lápis, caneta, pincel, marcador | Traço vetorial leve guardado no Journal; pressão da caneta (o egui não a entrega hoje) |
| Texto composto com qualidade | Compositor próprio (`core/src/typeset/`): Knuth–Plass, hifenização em português, PDF | Linhas de largura variável para contornar imagens |
| Biblioteca de imagens | Modo imagens (`src/images_ui.rs`), CLIP, coleções, Pictogrep, pins do Pinterest | Arrastar da biblioteca para o Journal |
| Nota editável num painel | Painel de notas do modo imagens | O mesmo painel dentro do Journal |
| Formato que convive com plugin | Quadro: grava Kanban e Excalidraw no formato deles, com um bloco `%% ateliet:quadro %%` | O mesmo padrão para a página de Journal |
| ZIP | `core/src/board/zip.rs` (lê e grava) | Leitor de EPUB por cima dele |
| PDF e PNG | `typeset/pdf.rs`, `typeset/raster.rs`, exportação do Quadro | Exportar páginas do Journal |

A conclusão é que o Journal é viável sem trocar de base técnica. Ele é, na prática, um terceiro canvas, irmão do Quadro e do Moodboard, com o papel e a tipografia do Diagramação. O trabalho novo de verdade está em quatro lugares: o texto que contorna formas, as máscaras de recorte (manuais e automáticas), o leitor de EPUB e a entrada de fora do app (Kindle e digitalização).

Há três limites a dizer logo. Livros comprados na Amazon (AZW3, KFX) têm DRM e não podem ser abertos pelo Ateliet: o caminho para eles é o trecho enviado pelo próprio Kindle (seção 7), não a importação do livro. O reconhecimento de letra cursiva em português ainda erra bastante em qualquer motor local, então a imagem da página manuscrita continua sendo o original e a transcrição é uma camada auxiliar, corrigível. E o egui 0.31 não repassa a pressão da caneta de mesas digitalizadoras no macOS, então o traço à mão começa com espessura por velocidade, como o storyboard já faz.

## 3. O modo de colagem do Pinterest

O Pinterest tem uma ferramenta de colagens (antes um app separado, o Shuffles) no app principal. O fluxo dela:

- **Recorte (cutout).** Abre-se um pin e escolhe-se a parte a recortar. A ferramenta *Select* isola o objeto automaticamente; *Add* e *Erase* refinam a borda com pincel; *Shapes* oferece formas prontas (círculo, estrela, arco, coração) como molde. O recorte vira uma camada nova na colagem.
- **Camadas.** Cada recorte é uma camada; a lista de camadas permite reordenar arrastando, apagar e selecionar.
- **Transformações.** Mover, escalar e girar com dois dedos, espelhar, duplicar, trazer para frente e mandar para trás.
- **Desenho, texto e fundo.** Pincel livre, caixas de texto com fontes e cores, fundo de cor ou de imagem.
- **Origem preservada.** Cada recorte guarda o pin de onde veio, e quem vê a colagem pode abrir o pin original.
- **Remix.** Partir da colagem de outra pessoa como modelo. Desde 2024, há também exportação em vídeo e efeitos de animação nas camadas.

Para o Journal valem o recorte com seleção automática e refinamento por pincel, as formas de molde, a lista de camadas, as transformações e a origem guardada em cada recorte. O remix vira "duplicar página como modelo" (com o Templater, seção 9). Animação e vídeo ficam de fora: não fazem sentido numa página de caderno.

O Journal acrescenta o que o Pinterest não tem e a página de referência pede: borda de adesivo (contorno branco que segue a máscara, com sombra), bordas rasgadas, fita adesiva, filtros de impressão, texto que contorna a imagem e legendas e créditos que ficam presos à imagem quando ela se move.

## 4. Visão geral

```
┌──────────────────────────────────── src/ (interface, egui) ─────────────────────────────────────┐
│ journal_ui/   modo Journal: canvas, ferramentas, camadas, painéis de Notas, Imagens e Trechos    │
│   mod.rs  estado (página, vista, seleção, desfazer, salvamento)   draw.rs  desenho               │
│   input.rs  ponteiro e teclado    edit.rs  mudanças    panel.rs  barras, inspetor, menus         │
│   cutout_ui.rs  editor de máscara   reader_ui.rs  leitor de EPUB   scan_ui.rs  digitalização     │
│ capture.rs    caixa de entrada: servidor local, ateliet://, clippings do Kindle, notificações    │
└──────────────────────────────────────────────┬──────────────────────────────────────────────────┘
                                               │ chamadas diretas
┌──────────────────────────────────────────────▼──────────────────────────────────────────────────┐
│ core/src/journal/   modelo, formato .md, papel, fluxo de texto, máscaras, adesivos, filtros, PNG │
│ core/src/epub/      leitura de EPUB: OPF, espinha, sumário, XHTML → blocos, localizadores        │
│ core/src/capture/   trechos (modelo comum), caixa de entrada, clippings, notebook HTML           │
│ core/src/ocr/       transcrição: Apple Vision, ONNX, API opcional; tratamento da imagem escaneada│
│ (reaproveitados)    moodboard · board/brush · typeset · quadro/rough · images · clip · pictogrep  │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Os três princípios do Ateliet continuam valendo: o vault é a fonte da verdade (nada vai para um banco próprio), as gravações em arquivos compartilhados são cirúrgicas, e o trabalho pesado (recorte automático, transcrição, leitura de EPUB, composição do texto) roda fora da thread da janela e volta por canais.

## 5. O modelo e o formato da página

### 5.1 O arquivo

Uma página de Journal é uma nota Markdown comum, com `journal` no frontmatter, para que o Obsidian, a busca, os backlinks e o Dataview a leiam sem saber nada do Journal. O texto mora no corpo da nota; a geometria mora num bloco escondido no fim, como o Quadro faz com o Kanban e o Excalidraw.

```markdown
---
journal: página
data: 2023-11-05
caderno: "[[Cadernos/Natureza]]"
fontes:
  - "[[Trechos/Mossy Earth – The Mountain Birch Project]]"
tags: [journal, escócia, florestas]
---

## ^j-txt1
> [!trecho] The Mountain Birch Project · Mossy Earth
> Montane woodland is a virtually extinct habitat in Scotland. Of the 5.1% of
> native forests in the Scottish Highlands, ==only a mere 4% exists above
> altitudes of 400 metres==. …
> — [vídeo](https://mossy.earth/projects/the-mountain-birch-project)

## ^j-mg1
Esse vídeo me foi recomendado no YouTube e eu cliquei porque há algo na
Escócia… #paisagem

## ^j-img1
![[Salentin – Betendes Mädchen.jpg]]
Hubert Salentin, 1882. *Betendes Mädchen mit einem Kranz aus Rosen*.

## ^j-scan1
![[Scans/2023-11-05 p1.jpg]]
> [!transcricao]- Transcrição
> This painting feels so fairytale to me. The painter does not do it justice…

%% ateliet:journal
{"v":1,"paper":{"preset":"grade-5mm","size":"A5","spread":true}, "items":[ … ]}
%%
```

Cada item com conteúdo tem uma seção com um id de bloco (`^j-…`). O JSON só guarda o que não é texto: posição, tamanho, rotação, camada, máscara, filtros, borda, ligação entre legenda e imagem. Por isso:

- no Obsidian, a página abre como uma nota legível, na ordem de leitura, com as imagens e as transcrições;
- a busca do Ateliet e o Dataview encontram o texto dos trechos, dos comentários e das transcrições;
- os grifos são `==realce==` do Markdown, e um grifo pode virar campo inline (`[grifo:: …]`) para ser consultado;
- links e tags escritos no Journal são links e tags de verdade, com backlinks;
- se alguém editar o texto no Obsidian, o Ateliet relê a seção pelo id e mantém a geometria. Uma seção apagada no Obsidian vira um item sem conteúdo, que o Ateliet mostra marcado em vez de apagar.

O bloco JSON segue as regras do bloco do Quadro: `` ` `` e `%` escapados, chaves desconhecidas preservadas (`extra` com `serde(flatten)`), gravação atômica e releitura do disco antes de gravar. Uma cópia do bloco vai para `.obsidian/ateliet/journal/` para sobreviver a um plugin que reescreva a nota.

Um **caderno** é uma pasta (ou uma nota-índice com `journal: caderno`) cujas páginas têm `caderno` e `pagina` no frontmatter. A página dupla é a vista padrão: duas notas, uma em cada lado da lombada, ou uma nota com `spread: true` quando a composição atravessa a lombada (como as páginas unidas do storyboard).

### 5.2 Os itens (`core/src/journal/mod.rs`)

```rust
pub struct Page {
    pub paper: Paper,            // preset, tamanho, cor, textura, página dupla
    pub guides: Vec<Guide>,      // colunas e linhas de margem (Cornell, 2 colunas…)
    pub items: Vec<Item>,        // ordem = camadas, de baixo para cima
    pub extra: Map<String, Value>,
}

pub struct Item {
    pub id: String,              // também o id do bloco ^j-…
    pub kind: Kind,
    pub x: f64, pub y: f64, pub w: f64, pub h: f64, pub r: f64,   // em mm da página
    pub locked: bool,
    pub page: Side,              // Esquerda, Direita ou Ambas (atravessa a lombada)
    pub mask: Option<Mask>,      // recorte não retangular
    pub look: Look,              // filtros, borda de adesivo, sombra, fita, borda rasgada
    pub anchor: Option<Anchor>,  // legenda presa a uma imagem, grifo preso a um trecho
    pub wrap: Wrap,              // Nenhum, Contorno (caixa) ou Forma (segue a máscara)
    pub extra: Map<String, Value>,
}

pub enum Kind {
    Text { style: TextStyle },                 // texto livre: digitado ou com fonte manuscrita
    Excerpt { source: SourceRef, style: TextStyle }, // trecho citado, com origem
    Image { src: ImageRef },                    // arquivo do vault ou id do Pictogrep
    Ink { strokes: Vec<Stroke> },               // traço à mão (vetorial)
    Scan { src: ImageRef, ocr: Option<OcrRef> },// página manuscrita digitalizada
    Line { pts: Vec<(f64, f64)>, style: LineStyle },
    Note { path: String, section: Option<String> }, // nota do vault incorporada ao vivo
    Sticker { src: ImageRef },                  // imagem já recortada, da gaveta de adesivos
    Tape { color: String, pattern: String },
}
```

A geometria (caixas, rotação, guias de encaixe, alinhar, organizar) vem do `moodboard.rs`, que já resolve tudo isso no mesmo sistema de coordenadas. Vale extrair essas funções para um módulo comum (`core/src/canvas.rs`) usado pelos dois, em vez de copiá-las.

`ImageRef` aceita os dois jeitos de apontar uma imagem que o Ateliet já usa: caminho no vault, ou o id do Pictogrep (SHA-256 do caminho absoluto) para imagens da biblioteca compartilhada. Como no modo imagens, uma imagem de fora do vault usada numa página é copiada para o vault na hora de gravar, para que a nota funcione no Obsidian.

## 6. As ferramentas de imagem

### 6.1 Recorte (`core/src/journal/mask.rs`, `src/journal_ui/cutout_ui.rs`)

A máscara é guardada em duas partes: uma forma vetorial (polígono, laço livre ou uma das formas de molde) e, opcionalmente, um mapa alfa em PNG ao lado da imagem (`Anexos/journal/<id>.mask.png`), para o que o pincel e a seleção automática produzem. O original nunca é alterado.

- **Formas de molde**: retângulo, círculo, arco, estrela, coração, selo serrilhado, polaroide. Geometria pura no núcleo.
- **Laço e polígono**: o caminho que o usuário traça, suavizado.
- **Pincel de adicionar e apagar**: pinta no mapa alfa com a mesma lógica de pincel do storyboard (`board/brush.rs`), em resolução reduzida e ampliada com borda suave.
- **Seleção automática do objeto**: segmentação de primeiro plano.
  - No macOS 14 ou mais novo: `VNGenerateForegroundInstanceMaskRequest` do Vision, o mesmo motor de "copiar objeto" das Fotos. Chamado por um pequeno binário auxiliar em Swift empacotado no `.app` (ou pelo crate `objc2-vision`), que recebe o caminho da imagem e devolve o PNG da máscara.
  - Nos outros sistemas: um modelo ONNX de recorte (como o BiRefNet leve ou o U²-Net), baixado na primeira vez, como o CLIP já é, rodando pelo mesmo `ort` da feature `vision`.
  - Um clique sobre a imagem escolhe a instância mais próxima do ponto, como o *Select* do Pinterest.
- **Gaveta de adesivos**: todo recorte pode ser guardado como adesivo reutilizável (um PNG com transparência numa coleção "Adesivos" da biblioteca), com a origem preservada.

### 6.2 Aparência (`core/src/journal/look.rs`)

- **Borda de adesivo**: dilatação do alfa da máscara pelo raio escolhido, preenchida de branco (ou outra cor), com uma leve irregularidade opcional para parecer cortada à tesoura e uma sombra curta. É o que aparece no anjo e na nuvem da referência.
- **Borda rasgada**: deslocamento da máscara por ruído ao longo do contorno, de um lado ou de todos.
- **Fita adesiva**: um retângulo semitransparente com textura e pontas serrilhadas, que pode prender qualquer item.
- **Filtros**: os do compositor (`pb`, `sepia`, `contraste`, `desfoque`) e quatro novos, no mesmo `compose::filter` para que a tela, o PNG e o PDF fiquem iguais: `granulado` (ruído com semente fixa por item), `fotocopia` (limiar suave com perda de meios-tons), `reticula` (pontos de meio-tom) e `impressao` (saturação menor e preto levantado, como impressora doméstica). Mais o modo de mistura "multiplicar", para que o papel apareça através de uma impressão.

As camadas aparecem numa lista no inspetor, com arrastar para reordenar, ocultar, travar e apagar, como no Pinterest. Cada item mostra sua origem (pin, arquivo, livro, Kindle), e um clique na origem a abre.

### 6.3 Texto que contorna imagens (`core/src/journal/flow.rs`)

O compositor do Ateliet quebra parágrafos com Knuth–Plass sobre uma largura fixa. Para o Journal ele passa a aceitar uma função de largura por linha: para cada linha (pela altura da linha e o y em que ela cai), a largura útil é o intervalo da coluna menos as áreas dos itens marcados com `wrap`. Com `wrap: Contorno` a área é a caixa do item; com `wrap: Forma`, o contorno da máscara mais uma folga. O Knuth–Plass já trabalha com larguras por linha, então a mudança é entregar a largura certa a cada linha, não trocar o algoritmo.

O texto parado é desenhado pelo compositor numa textura (como as páginas do Diagramação) e, quando se clica nele, vira um editor de Markdown do Ateliet no lugar (o mesmo editor das notas, com Live Preview), no tamanho da caixa. É a mesma troca entre leitura e edição que o Live Preview faz por bloco.

O texto pode usar a fonte do tema, as fontes embutidas do Diagramação ou uma fonte manuscrita (uma fonte livre como Caveat ou Homemade Apple, embutida, ou uma fonte do sistema), para comentários que devem parecer escritos à mão sem ter sido.

## 7. As entradas: trechos de EPUB, do Kindle e de fora

Todas as entradas produzem o mesmo objeto, um **trecho**, e o Journal só conhece esse objeto. Assim, colar do EPUB, do Kindle, de uma página web ou do próprio manuscrito do Scriptorium é a mesma operação.

```rust
// core/src/capture/mod.rs
pub struct Snippet {
    pub id: String,
    pub text: String,            // o trecho, em Markdown
    pub note: Option<String>,    // comentário que veio junto (a nota do Kindle, por exemplo)
    pub source: SourceRef,
    pub captured: DateTime,
    pub origin: Origin,          // Epub, Kindle, KindleClippings, Web, Servico, Manual
}

pub struct SourceRef {
    pub title: String,
    pub author: Option<String>,
    pub book: Option<String>,    // nota do livro no vault ([[Livros/…]]), se houver
    pub url: Option<String>,
    pub locator: Option<Locator>,// onde no livro: capítulo, CFI, posição do Kindle, página
}
```

Cada trecho vira uma nota curta em `Trechos/` (pasta configurável), com o texto num callout `[!trecho]` e a fonte no frontmatter. Notas, e não um JSON escondido, porque assim os trechos ficam pesquisáveis, ligáveis e consultáveis pelo Dataview mesmo sem o Journal ("todos os trechos de tal livro", "trechos ainda não usados"). Quando um trecho é posto numa página, a página ganha um link para a nota do trecho, e o trecho ganha o backlink: de cada trecho dá para ver onde se escreveu sobre ele.

### 7.1 EPUB (`core/src/epub/`, `src/journal_ui/reader_ui.rs`)

- **Leitura**: o EPUB é um ZIP (o `board/zip.rs` lê); `META-INF/container.xml` aponta o OPF; o OPF dá metadados, manifesto e espinha; `nav.xhtml` (EPUB 3) ou `toc.ncx` (EPUB 2) dá o sumário. Cada documento da espinha é convertido de XHTML em blocos (parágrafos, títulos, citações, listas, imagens, ênfases) por um leitor de XML pequeno (`quick-xml`, a única dependência nova aqui). O CSS do livro é ignorado; o texto é desenhado com a tipografia do tema.
- **Biblioteca**: os EPUBs ficam onde o usuário quiser (no vault, em `Livros/`, ou numa pasta de fora). Cada livro aberto ganha (se o usuário quiser) uma nota de livro com capa, autor e uma consulta Dataview dos seus trechos; a nota fica compatível com as notas de livro que o Scriptorium e os plugins de leitura já usam.
- **Leitor**: uma aba de leitura com rolagem contínua por capítulo, sumário à esquerda, busca no livro. Selecionar texto mostra uma barra com "Grifar", "Mandar ao Journal" e "Nova nota sobre isto".
- **Localizador**: cada trecho guarda o índice na espinha, o caminho do elemento (no estilo do EPUB CFI) e os deslocamentos de texto, mais o próprio texto citado com algumas palavras antes e depois. Se o EPUB mudar (outra edição), o trecho é reencontrado pelo texto, como fazem os grifos da web. Clicar na fonte de um trecho abre o livro naquele ponto.
- **DRM**: arquivos com `META-INF/encryption.xml` cifrando o conteúdo são recusados com uma mensagem clara.

### 7.2 Kindle

Não há um caminho único; o Kindle é fechado, e cada aparelho permite uma coisa diferente. O Ateliet oferece os quatro caminhos possíveis, todos terminando num trecho:

1. **Kindle no iPhone ou iPad (o caminho principal).** No app do Kindle, selecionar um trecho mostra "Compartilhar", que entrega o texto citado com título e autor à folha de compartilhamento do sistema. Um atalho do app Atalhos, "Mandar ao Ateliet", fornecido pronto pelo Ateliet (um arquivo `.shortcut` e as instruções), recebe esse texto e o entrega de um destes jeitos:
   - escreve um arquivo na caixa de entrada do vault (`.obsidian/ateliet/inbox/<data>-<id>.json`), quando o vault está no iCloud Drive: funciona mesmo com o Mac desligado, e o Ateliet importa na próxima vez que o watcher vir o arquivo;
   - ou faz um POST ao servidor local do Ateliet no Mac, na mesma rede (7.3), para o trecho aparecer na hora.
2. **Kindle no Mac.** O app do Kindle no Mac permite copiar a seleção (com um limite por livro definido pela editora). O Ateliet registra um item no menu Serviços do macOS ("Mandar ao Ateliet"), declarado no `Info.plist` do `.app`, e um atalho de teclado global opcional que lê a área de transferência. O Ateliet reconhece o formato da cópia do Kindle (texto, seguido de "Kindle Edition", autor e posição) e separa a fonte do texto.
3. **Kindle de tinta eletrônica.** O aparelho não envia nada pela rede, mas grava cada grifo e nota em `documents/My Clippings.txt`. Quando o Kindle é ligado ao computador por USB (o volume `Kindle` aparece), o Ateliet oferece importar os grifos novos. O parser lê os blocos separados por `==========` (título e autor, tipo, posição e data, texto), em inglês e em português, e lembra o que já importou pelo hash de cada bloco.
4. **Exportação do caderno do Kindle.** O app do Kindle exporta as notas e grifos de um livro em HTML (por e-mail no celular, por arquivo no computador). Soltar esse arquivo no Ateliet importa todos os trechos.

O que não dá para fazer, e o documento deixa registrado para não virar promessa: ler a seleção dentro do app do Kindle sem que o usuário toque em "Compartilhar" ou "Copiar", e baixar os grifos de `read.amazon.com` sem um login da Amazon guardado pelo Ateliet (o que não vale o risco). Os limites de cópia da editora continuam valendo nos caminhos 1 e 2.

### 7.3 Caixa de entrada e servidor local (`core/src/capture/inbox.rs`, `src/capture.rs`)

- **Pasta da caixa de entrada**: `.obsidian/ateliet/inbox/`. Qualquer arquivo `.json` ali com o formato de `Snippet` (ou um `.txt`/`.md` simples) é importado e apagado. É a porta mais simples e funciona com qualquer automação: Atalhos, Tasker no Android, um script.
- **Esquema de URL**: `ateliet://capture?text=…&title=…&author=…&url=…`, registrado no `Info.plist` (`CFBundleURLTypes`) no macOS e num arquivo `.desktop` no Linux. Serve para o Atalhos no próprio Mac e para favoritos do navegador.
- **Servidor local**: um servidor HTTP mínimo (`tiny_http`, numa thread) em `127.0.0.1`, com `POST /capture`. Desligado por padrão para a rede; ligado, escuta na rede local com um token gerado pelo app e mostrado como código QR em Configurações, para que só o atalho configurado consiga enviar. O Pictogrep já usa uma porta local; o Ateliet usa outra.
- Cada trecho que chega mostra um aviso discreto ("Trecho de *Livro* chegou") com "Pôr na página aberta" e "Guardar". O painel **Trechos** do Journal lista os que ainda não foram usados.

## 8. Digitalizar as notas escritas à mão (`core/src/ocr/`, `src/journal_ui/scan_ui.rs`)

A digitalização tem duas metades, e só a segunda depende de reconhecimento de texto.

### 8.1 A imagem

1. **Entrada**: uma foto do celular (pela caixa de entrada do iCloud, como os trechos, ou pela Câmera de Continuidade do macOS), um arquivo de scanner ou uma imagem arrastada.
2. **Página**: detecção do quadrilátero da folha (bordas e o maior contorno com quatro cantos), correção de perspectiva e recorte, com os quatro cantos ajustáveis à mão.
3. **Limpeza**: balanço de branco pelo papel, uniformização da iluminação (divisão por uma versão muito desfocada da própria imagem) e, opcionalmente, retirada do quadriculado ou das pautas.
4. **Tinta separada**: um limiar adaptativo transforma o papel em transparência e deixa só os traços. A tinta vira um item `Scan` sem fundo, que pode ser posto sobre o papel do Journal, recolorido (lápis, nanquim, azul) e recortado como qualquer imagem. É assim que um desenho feito no caderno, como o anjo da referência, vira um adesivo.

Tudo isso é processamento de imagem simples, no núcleo, com o crate `image` que o Ateliet já usa.

### 8.2 A transcrição

Um único traço no núcleo e três motores, escolhidos em Configurações:

```rust
pub trait Recognizer {
    fn recognize(&self, img: &RgbaImage, langs: &[&str]) -> Result<Vec<Line>, String>;
}
pub struct Line { pub text: String, pub bbox: [f32; 4], pub confidence: f32, pub words: Vec<Word> }
```

- **Apple Vision (macOS, padrão)**: `VNRecognizeTextRequest` em modo preciso, que reconhece letra de mão, com português e inglês na lista de idiomas. Local, gratuito e sem rede. Chamado pelo mesmo auxiliar em Swift do recorte automático.
- **Modelo ONNX local (Linux e Windows)**: detecção de linhas mais um reconhecedor de linha manuscrita (da família TrOCR), baixado na primeira vez. É o mais fraco em português e em letra cursiva; serve para busca, não para transcrição fiel.
- **Claude pela API (opcional, desligado por padrão)**: a página é enviada a um modelo com visão, que transcreve mantendo a disposição das linhas. É o melhor para letra difícil e para páginas mistas com desenho. Exige chave de API e manda a imagem para fora do computador, então só roda quando o usuário pede, página por página.

O resultado vai para o corpo da nota como um callout `[!transcricao]` dobrado sob a imagem (5.1). Assim a transcrição é pesquisável no Ateliet, no Obsidian e no Dataview, enquanto a página mostra a letra original. As caixas de cada palavra ficam no JSON, o que permite três coisas: a busca destaca a palavra na imagem; o usuário corrige a transcrição clicando na linha errada; e uma linha manuscrita pode ser convertida em texto digitado no mesmo lugar, se ele quiser.

## 9. Convivência com os plugins

O Ateliet não roda plugins da comunidade; os "plugins" com que o Journal precisa conviver são os que ele traz embutidos e os que o Obsidian continua rodando no mesmo vault. O formato da seção 5 foi escolhido para isso, e cada um ganha uma ligação concreta:

- **Obsidian**: a página é uma nota Markdown legível; imagens são embeds `![[…]]`; o bloco `%% … %%` é um comentário que o Obsidian esconde. Nenhum plugin do Obsidian é necessário para ler um Journal.
- **Dataview**: o frontmatter (`journal`, `data`, `caderno`, `fontes`, `tags`) e os campos inline nos grifos permitem consultas como "páginas deste caderno", "trechos de tal livro ainda sem página", "páginas com imagens de tal coleção". Uma consulta Dataview pode também virar um item de texto vivo na página.
- **Templater**: um modelo de página é uma página de Journal comum numa pasta de modelos. Ao criar uma página a partir dele, o Templater roda no texto (datas, prompts) e a geometria é copiada. É o "remix" do Pinterest aplicado a si mesmo.
- **Notas diárias e Calendário**: um caderno pode ser marcado como diário; "Página de hoje" cria ou abre a página com a data, e o calendário mostra os dias que têm página.
- **Moodboards**: selecionar itens de um moodboard e "Mandar ao Journal" leva as imagens com recorte, giro e espelho (os campos são os mesmos). O contrário também: uma página pode virar moodboard.
- **Quadro**: uma página de Journal pode ser um cartão num quadro Kanban ou num desenho do Excalidraw, com prévia.
- **Scriptorium**: um trecho pode vir de um capítulo do próprio manuscrito (origem `Manual`, com link para o capítulo), e uma página do Journal pode entrar como página composta no livro.
- **Pictogrep e Pinterest**: imagens e recortes da biblioteca compartilhada são guardados pelo id do Pictogrep; um recorte feito no Journal pode ser salvo de volta na biblioteca como adesivo, com o pin de origem.
- **Busca e backlinks**: tudo o que é texto está no corpo da nota.

## 10. A interface

O Journal é o terceiro modo na faixa lateral esquerda, ao lado de Escrita e Imagens (`Ctrl/Cmd+Shift+J`), com seu próprio layout guardado em `workspace-journal.json`. Uma página de Journal também abre como aba no modo escrita, como um Quadro.

- **Centro**: a página dupla sobre a mesa, com zoom e rolagem; o papel, as guias e os itens. Os itens podem sair da página, como o adesivo da referência; na exportação, o que sai fica cortado ou vai junto, à escolha.
- **Barra de ferramentas**: selecionar, texto, trecho, imagem, recorte, caneta, linha, fita, adesivo, e a escolha do papel.
- **Esquerda**: três abas que trazem material para a página, cada uma usando o que já existe no app:
  - **Imagens**: a biblioteca do modo imagens (coleções, busca por nome e por descrição com o CLIP, imagens parecidas, a gaveta de adesivos). Arrastar para a página cria um item; arrastar sobre outra imagem a coloca por cima.
  - **Notas**: notas recentes, busca e as notas abertas no modo escrita. Arrastar uma nota a incorpora ao vivo (o texto acompanha a nota), e com Alt a copia. Uma nota aberta no painel pode ser editada ali mesmo, com a barra do painel de notas do modo imagens.
  - **Trechos**: a caixa de entrada (Kindle, EPUB, web, digitalizações pendentes), os livros em EPUB e as notas de `Trechos/`.
- **Direita**: o inspetor do item selecionado (posição, giro, máscara, borda de adesivo, filtros, contorno de texto, origem) e a lista de camadas.
- **Modo escrita**: um comando "Mandar ao Journal" na seleção do editor e no menu de imagens; o painel Pictures ganha "Pôr na página aberta".
- **Modo imagens**: "Mandar ao Journal" na seleção da biblioteca e no visualizador.

Exportação: PNG e PDF por página ou por caderno, pelo rasterizador e pelo PDF do compositor (que já embutem fontes e imagens), em página simples ou dupla.

## 11. Ordem de implementação

Cada fase entrega algo usável sozinho e prepara a próxima.

1. **Núcleo da página**: `journal/mod.rs`, o formato `.md` com o bloco JSON e testes de ida e volta byte a byte (como os do Quadro); `canvas.rs` extraído do moodboard; papéis e guias. Na interface, o modo Journal com imagens e texto simples, mover, girar, camadas, desfazer. Arrastar do modo imagens e do modo notas.
2. **Aparência**: recorte por formas, laço e pincel; borda de adesivo, sombra, fita, borda rasgada; os quatro filtros novos no compositor; gaveta de adesivos.
3. **Trechos**: o modelo `Snippet`, notas em `Trechos/`, caixa de entrada por pasta, esquema `ateliet://`, importação de `My Clippings.txt` e do HTML do caderno do Kindle, o atalho para iPhone. O painel Trechos.
4. **EPUB**: leitor, sumário, seleção, localizadores, nota do livro.
5. **Texto contornando imagens**: largura por linha no compositor, troca entre texto composto e editor no lugar, fontes manuscritas.
6. **Digitalização**: perspectiva, limpeza, tinta separada; transcrição pelo Apple Vision; correção de linhas; motor ONNX e a opção pela API.
7. **Recorte automático e servidor local**: segmentação de primeiro plano (Vision e ONNX); servidor HTTP com token e código QR; o serviço do macOS.
8. **Integrações restantes**: modelos com Templater, página do dia, moodboard ↔ Journal, cartão no Quadro, exportação do caderno.

## 12. Dependências novas e riscos

- **Crates**: `quick-xml` (EPUB), `tiny_http` (servidor local), `global-hotkey` (atalho global, opcional), `objc2-vision` ou um auxiliar em Swift no macOS. Os modelos ONNX de recorte e de escrita são baixados na primeira vez, como o CLIP, e entram pela feature `vision`, para que um build sem ela continue funcionando, só sem os recursos automáticos.
- **Pressão da caneta**: o egui não a fornece no macOS hoje. O traço usa velocidade; ler a pressão exigiria tratar os eventos do `winit` diretamente.
- **Edição dos dois lados**: a página pode ser editada no Obsidian enquanto o Ateliet a tem aberta. A releitura pelo watcher e a gravação que relê o disco antes (como no Quadro) resolvem o caso comum; um conflito na mesma seção fica com a versão do disco e avisa.
- **Transcrição**: a qualidade em letra cursiva em português é o maior risco técnico do projeto, e ele é contido pelo desenho: a imagem é sempre o original, e a transcrição só serve para busca e para quem quiser corrigi-la.
- **Kindle**: depende de comportamentos da Amazon que podem mudar (o "Compartilhar" do app, o formato da cópia, o `My Clippings.txt`). Cada parser fica isolado e testado com exemplos reais, e a caixa de entrada genérica continua funcionando se um deles quebrar.

## Fontes consultadas

- Pinterest, [Create a collage](https://help.pinterest.com/en/article/create-a-collage)
- Metricool, [Pinterest-Collagen: So nutzt du die neuen Features](https://metricool.com/de/pinterest-collage-neue-features/)
- Outfy, [Pinterest new collage features](https://www.outfy.com/blog/pinterest-new-collage-features/)
- Social Media Today, [Pinterest adds collage remix and sharing options](https://www.socialmediatoday.com/news/pinterest-adds-collage-remix-sharing-options/727961/)
- Hongkiat, [How to export Kindle notes and highlights](https://www.hongkiat.com/blog/export-kindle-highlights/)
- Web Highlights, [How to export Kindle highlights](https://web-highlights.com/tutorials/pdf-web-highlighter/how-to-export-kindle-highlights)
