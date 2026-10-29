# Journal: estudo e arquitetura

Este documento parte de uma página de referência: um caderno quadriculado aberto, com um trecho de artigo colado, comentários a lápis, uma pintura recortada e uma foto sobreposta. Ele lista os elementos dessa página, diz o que o Ateliet já tem para reproduzi-los e propõe a arquitetura do Journal e das duas entradas que o alimentam além do próprio vault: os trechos e recortes do [Leitor](LEITOR.md) e a digitalização de notas manuscritas. As ferramentas de imagem seguem o modo de colagem do Pinterest, estudado na seção 3.

A decisão que organiza tudo: **o Journal é um caderno**. Ele tem capa, páginas de tamanho fixo, numeradas e em ordem, que se viram uma a uma. Não é um canvas infinito como o Quadro e o Moodboard. E ele tem a aparência do resto do Ateliet: papel liso, linhas finas, nenhuma imitação de couro, espiral ou folha envelhecida. A seção 4 descreve essa forma.

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
14. **Adesivo que sai da página.** Um traço decorativo (uma nuvem ou coroa em contorno) com borda branca, que cobre parte do texto e passa um pouco da borda superior da página, como um adesivo de verdade colado na beira.
15. **Pequenos ornamentos.** Uma estrela com uma fita junto à legenda, também com borda branca.
16. **Textura de impressão e digitalização.** Granulado, cor de fotocópia, sombra da curvatura do caderno. A página foi fotografada ou digitalizada, e as imagens coladas foram impressas antes de serem recortadas.

Daí saem os tipos de item que o Journal precisa ter: papel, guias e linhas, texto citado com fonte, grifo ancorado, texto livre (digitado ou manuscrito), imagem recortada, adesivo com borda, legenda e crédito, com sobreposição e rotação. E sai também a forma: tudo isso acontece dentro de páginas de um caderno, em sequência.

## 2. O que o Ateliet já tem

Boa parte das peças existe e pode ser reaproveitada, sem começar do zero:

| Elemento | Base existente | O que falta |
|---|---|---|
| Página e página dupla | `core/src/board/pages.rs` (tamanhos de página, DPI, páginas unidas com a lombada no meio), `board_ui/spread.rs` | O caderno como sequência de páginas; papéis quadriculado, pautado, pontilhado e Cornell |
| Linhas de margem | Formas e linhas do Quadro (`core/src/quadro/rough.rs`) | Guias de coluna com encaixe |
| Imagens sobre um fundo | Moodboards (`core/src/moodboard.rs`): itens com `x, y, w, h, r`, recorte retangular (`CropStage`), espelhar, cinza, guias de alinhamento, paleta extraída | Máscaras não retangulares, borda de adesivo, filtros de impressão |
| Filtros | `typeset::compose::filter`: `pb`, `sepia`, `noite`, `contraste`, `desfoque` | Granulado, fotocópia, retícula, impressão |
| Desenho à mão | Pincéis do storyboard (`core/src/board/brush.rs`): lápis, caneta, pincel, marcador | Traço vetorial leve guardado no Journal; pressão da caneta (o egui não a entrega hoje) |
| Texto composto com qualidade | Compositor próprio (`core/src/typeset/`): Knuth–Plass, hifenização em português, grade de linhas, PDF | Linhas de largura variável para contornar imagens; texto na pauta do papel |
| Biblioteca de imagens | Modo imagens (`src/images_ui.rs`), CLIP, coleções, Pictogrep, pins do Pinterest | Arrastar da biblioteca para a página |
| Nota editável num painel | Painel de notas do modo imagens | O mesmo painel dentro do Journal |
| Formato que convive com plugin | Quadro: grava Kanban e Excalidraw no formato deles, com um bloco `%% ateliet:quadro %%` | O mesmo padrão para a página de Journal |
| PDF e PNG | `typeset/pdf.rs`, `typeset/raster.rs`, exportação do Quadro | Exportar páginas e cadernos |
| Moldura que some | O modo imersivo do Leitor ([LEITOR.md](LEITOR.md), seção 5) | Reaproveitar no Journal |

O Journal é viável sem trocar de base técnica. Ele junta a página e a lombada do storyboard, a geometria de itens do Moodboard e a tipografia da Diagramação. O trabalho novo de verdade está em quatro lugares: o caderno como objeto (sequência, numeração, virar páginas), o texto que corre na pauta e contorna formas, as máscaras de recorte (manuais e automáticas) e a digitalização.

Há dois limites a dizer logo. O reconhecimento de letra cursiva em português ainda erra bastante em qualquer motor local, então a imagem da página manuscrita continua sendo o original e a transcrição é uma camada auxiliar, corrigível. E o egui 0.31 não repassa a pressão da caneta de mesas digitalizadoras no macOS, então o traço à mão começa com espessura por velocidade, como o storyboard já faz.

## 3. O modo de colagem do Pinterest

O Pinterest tem uma ferramenta de colagens (antes um app separado, o Shuffles) no app principal. O fluxo dela:

- **Recorte (cutout).** Abre-se um pin e escolhe-se a parte a recortar. A ferramenta *Select* isola o objeto automaticamente; *Add* e *Erase* refinam a borda com pincel; *Shapes* oferece formas prontas (círculo, estrela, arco, coração) como molde. O recorte vira uma camada nova na colagem.
- **Camadas.** Cada recorte é uma camada; a lista de camadas permite reordenar arrastando, apagar e selecionar.
- **Transformações.** Mover, escalar e girar com dois dedos, espelhar, duplicar, trazer para frente e mandar para trás.
- **Desenho, texto e fundo.** Pincel livre, caixas de texto com fontes e cores, fundo de cor ou de imagem.
- **Origem preservada.** Cada recorte guarda o pin de onde veio, e quem vê a colagem pode abrir o pin original.
- **Remix.** Partir da colagem de outra pessoa como modelo. Desde 2024, há também exportação em vídeo e efeitos de animação nas camadas.

Para o Journal valem o recorte com seleção automática e refinamento por pincel, as formas de molde, a ordem de camadas, as transformações e a origem guardada em cada recorte. O remix vira "nova página a partir de um modelo" (com o Templater, seção 10). Animação e vídeo ficam de fora: não fazem sentido numa página de caderno. O fundo livre também: num caderno, o fundo é o papel.

O Journal acrescenta o que o Pinterest não tem e a página de referência pede: borda de adesivo (contorno branco que segue a máscara), bordas rasgadas, fita adesiva, filtros de impressão, texto que contorna a imagem e legendas e créditos que ficam presos à imagem quando ela se move.

## 4. O caderno

### 4.1 O que faz dele um caderno

- **Um objeto com começo e fim.** Um caderno tem nome, capa, tamanho (A5, B5, A4, bolso), papel e um número de páginas que cresce conforme se escreve. Começa com uma folha de rosto (nome, data de início) e pode ter um sumário gerado dos títulos das páginas.
- **Páginas fixas.** Cada página tem o tamanho do caderno. Não há rolagem infinita nem zoom livre para fora da página: a vista mostra a página dupla inteira, ou uma página, ou uma ampliação para trabalho fino, e volta à página dupla com um toque.
- **Sequência.** As páginas são numeradas e lidas em ordem. Vira-se a página pelas setas, por gesto no trackpad ou clicando no canto. Uma página nova entra depois da atual (ou no fim); páginas podem ser reordenadas na vista de miniaturas, como se faz ao reorganizar um fichário, mas a numeração acompanha a ordem.
- **Página dupla.** A vista padrão é a página dupla, com a lombada no meio, como na referência. Um item pode atravessar a lombada (uma imagem grande sobre as duas páginas), como as páginas unidas do storyboard.
- **A pauta manda no texto.** Clicar num ponto vazio da página começa a escrever ali, como numa folha. O texto assenta nas linhas do papel (pautado, quadriculado ou pontilhado), com a entrelinha igual ao espaçamento da pauta, e corre entre as margens da coluna. Um texto que não cabe continua na página seguinte, como o trecho da referência.
- **A página tem bordas.** Os itens ficam na página. Um adesivo pode passar um pouco da borda externa, como passaria no papel; o que passa é cortado na exportação, a menos que se peça o contrário. Nada vai para um espaço fora do caderno.
- **Marcador.** O caderno abre na última página em que se trabalhou, como uma fita marcadora. Marcadores extras (poucos) ficam como linguetas finas na borda.
- **Estante.** Os cadernos ficam numa estante: capas lado a lado, cada uma com sua cor e nome. Abrir um caderno é abri-lo na página marcada.

### 4.2 A estética

O Journal parece um caderno pela forma (página, lombada, pauta, sequência) e não pela textura. A aparência segue a do Ateliet:

- **Papel**: uma cor lisa, derivada do fundo do tema (um branco levemente quente no tema claro, um cinza escuro no tema escuro), sem granulado por padrão. A textura de papel existe como opção, discreta.
- **Pauta e grade**: linhas finas na cor do texto com opacidade muito baixa, que somem quando se exporta sem pauta. Margens e colunas Cornell no mesmo traço.
- **Lombada**: uma linha fina e uma sombra muito leve de um lado e do outro, só o bastante para ler a página dupla como um caderno aberto. Sem argolas, costuras ou curvatura.
- **Volume**: a espessura das páginas que faltam é sugerida por duas ou três linhas finas na borda externa, mais grossas quanto mais páginas houver daquele lado.
- **Capa**: uma cor lisa, o nome em Inter ou numa das fontes da Diagramação, e opcionalmente uma imagem. Sem couro ou relevo.
- **Moldura**: como no Leitor, ela some. Enquanto se escreve, só o caderno aparece sobre o fundo do app. As ferramentas ficam numa barra pequena no pé, centrada, que aparece ao mover o ponteiro ou ao selecionar algo e some depois. As propriedades de um item aparecem num balão junto dele, não num painel fixo. Os painéis de material (seção 11) deslizam da esquerda quando chamados e se recolhem ao arrastar algo para a página.
- **Ícones e tipografia**: os do app (Lucide, Inter), a cor de destaque do vault na seleção e nos marcadores. Nenhum elemento decorativo na interface: a decoração é o que o usuário põe na página.

## 5. Visão geral

```
┌──────────────────────────────────── src/ (interface, egui) ─────────────────────────────────────┐
│ journal_ui/   o Journal: estante, caderno, página dupla, ferramentas, painéis de material       │
│   mod.rs  estado (caderno, página, vista, seleção, desfazer, salvamento)   draw.rs  desenho      │
│   input.rs  ponteiro e teclado    edit.rs  mudanças    chrome.rs  barra e balões que somem      │
│   shelf.rs  estante e capas   pages.rs  miniaturas e ordem   cutout_ui.rs  editor de máscara    │
│   scan_ui.rs  digitalização                                                                      │
│ reader_ui/    o Leitor (EPUB, PDF, HQ): ver LEITOR.md                                            │
│ capture.rs    caixa de entrada: pasta e ateliet://                                               │
└──────────────────────────────────────────────┬──────────────────────────────────────────────────┘
                                               │ chamadas diretas
┌──────────────────────────────────────────────▼──────────────────────────────────────────────────┐
│ core/src/journal/   caderno, página, formato .md, papel e pauta, fluxo de texto, máscaras,       │
│                     adesivos, filtros, PNG e PDF                                                 │
│ core/src/reader/    o Leitor: EPUB, PDF, HQ, localizadores, grifos (LEITOR.md)                   │
│ core/src/capture/   trechos (modelo comum) e caixa de entrada                                    │
│ core/src/ocr/       transcrição: Apple Vision, ONNX, API opcional; tratamento da imagem escaneada│
│ (reaproveitados)    moodboard · board/brush · board/pages · typeset · images · clip · pictogrep  │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Os três princípios do Ateliet continuam valendo: o vault é a fonte da verdade (nada vai para um banco próprio), as gravações em arquivos compartilhados são cirúrgicas, e o trabalho pesado (recorte automático, transcrição, composição do texto, miniaturas das páginas) roda fora da thread da janela e volta por canais.

## 6. O formato

### 6.1 O caderno

Um caderno é uma pasta com uma nota-índice e uma nota por página:

```
Cadernos/
  Natureza/
    Natureza.md            ← journal: caderno (capa, papel, tamanho, marcador)
    001 Folha de rosto.md
    002.md
    003 Bétulas na Escócia.md
    …
```

~~~markdown
---
journal: caderno
tamanho: A5
papel: grade-5mm
colunas: cornell-esquerda
capa: "#2f4a3a"
inicio: 2023-11-01
marcador: 3
---

```dataview
TABLE WITHOUT ID pagina AS "p.", file.link AS "Página", data
FROM "Cadernos/Natureza" WHERE journal = "página" SORT pagina
```
~~~

A ordem das páginas vem do campo `pagina`, não do nome do arquivo; o número no nome é só para a pasta ficar em ordem no Obsidian, e o Ateliet o ajusta ao reordenar. O título da página é opcional: uma página sem título é só um número, como num caderno.

### 6.2 A página

Uma página é uma nota Markdown comum, com `journal: página` no frontmatter, para que o Obsidian, a busca, os backlinks e o Dataview a leiam sem saber nada do Journal. O texto mora no corpo da nota; a geometria mora num bloco escondido no fim, como o Quadro faz com o Kanban e o Excalidraw.

```markdown
---
journal: página
caderno: "[[Natureza]]"
pagina: 3
data: 2023-11-05
fontes:
  - "[[Trechos/Mossy Earth – The Mountain Birch Project]]"
tags: [escócia, florestas]
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
{"v":1,"lado":"direita","items":[ … ]}
%%
```

O papel, o tamanho e as colunas vêm do caderno; uma página só os guarda se forem diferentes (uma página pautada num caderno quadriculado, por exemplo). Cada item com conteúdo tem uma seção com um id de bloco (`^j-…`). O JSON só guarda o que não é texto: posição, tamanho, rotação, camada, máscara, filtros, borda, ligação entre legenda e imagem. Por isso:

- no Obsidian, a página abre como uma nota legível, na ordem de leitura, com as imagens e as transcrições;
- a busca do Ateliet e o Dataview encontram o texto dos trechos, dos comentários e das transcrições;
- os grifos são `==realce==` do Markdown, e um grifo pode virar campo inline (`[grifo:: …]`) para ser consultado;
- links e tags escritos no Journal são links e tags de verdade, com backlinks;
- se alguém editar o texto no Obsidian, o Ateliet relê a seção pelo id e mantém a geometria. Uma seção apagada no Obsidian vira um item sem conteúdo, que o Ateliet mostra marcado em vez de apagar.

O bloco JSON segue as regras do bloco do Quadro: `` ` `` e `%` escapados, chaves desconhecidas preservadas (`extra` com `serde(flatten)`), gravação atômica e releitura do disco antes de gravar. Uma cópia do bloco vai para `.obsidian/ateliet/journal/` para sobreviver a um plugin que reescreva a nota. Um item que atravessa a lombada fica na página da esquerda, marcado como `ambas`.

### 6.3 Os itens (`core/src/journal/mod.rs`)

```rust
pub struct Notebook {
    pub name: String,
    pub size: PageSize,          // A5, B5, A4, bolso ou medida própria
    pub paper: Paper,            // liso, pautado, quadriculado, pontilhado; espaçamento; cor
    pub columns: Columns,        // nenhuma, Cornell (margem interna ou externa), duas colunas
    pub cover: Cover,            // cor, imagem, fonte do nome
    pub pages: Vec<PageRef>,     // caminho e número, na ordem
    pub marker: usize,
}

pub struct Page {
    pub paper: Option<Paper>,    // só quando difere do caderno
    pub items: Vec<Item>,        // ordem = camadas, de baixo para cima
    pub extra: Map<String, Value>,
}

pub struct Item {
    pub id: String,              // também o id do bloco ^j-…
    pub kind: Kind,
    pub x: f64, pub y: f64, pub w: f64, pub h: f64, pub r: f64,   // em mm da página
    pub locked: bool,
    pub across: bool,            // atravessa a lombada
    pub mask: Option<Mask>,      // recorte não retangular
    pub look: Look,              // filtros, borda de adesivo, fita, borda rasgada
    pub anchor: Option<Anchor>,  // legenda presa a uma imagem, grifo preso a um trecho
    pub wrap: Wrap,              // Nenhum, Contorno (caixa) ou Forma (segue a máscara)
    pub extra: Map<String, Value>,
}

pub enum Kind {
    Text { style: TextStyle, flow: Option<Flow> },  // texto na pauta; flow liga à continuação na página seguinte
    Excerpt { source: SourceRef, style: TextStyle },// trecho citado, com origem
    Image { src: ImageRef },                        // arquivo do vault ou id do Pictogrep
    Ink { strokes: Vec<Stroke> },                   // traço à mão (vetorial)
    Scan { src: ImageRef, ocr: Option<OcrRef> },    // página manuscrita digitalizada
    Line { pts: Vec<(f64, f64)>, style: LineStyle },
    Note { path: String, section: Option<String> }, // nota do vault incorporada ao vivo
    Sticker { src: ImageRef },                      // imagem já recortada, da gaveta de adesivos
    Tape { color: String },
}
```

A geometria (caixas, rotação, guias de encaixe, alinhar) vem do `moodboard.rs`, que já resolve tudo isso. Vale extrair essas funções para um módulo comum (`core/src/canvas.rs`) usado pelos dois, em vez de copiá-las. A diferença é que no Journal as guias de encaixe incluem as margens, as colunas e as linhas da pauta.

`ImageRef` aceita os dois jeitos de apontar uma imagem que o Ateliet já usa: caminho no vault, ou o id do Pictogrep (SHA-256 do caminho absoluto) para imagens da biblioteca compartilhada. Como no modo imagens, uma imagem de fora do vault usada numa página é copiada para o vault na hora de gravar, para que a nota funcione no Obsidian.

## 7. As ferramentas de imagem

### 7.1 Recorte (`core/src/journal/mask.rs`, `src/journal_ui/cutout_ui.rs`)

A máscara é guardada em duas partes: uma forma vetorial (polígono, laço livre ou uma das formas de molde) e, opcionalmente, um mapa alfa em PNG ao lado da imagem (`Anexos/journal/<id>.mask.png`), para o que o pincel e a seleção automática produzem. O original nunca é alterado.

- **Formas de molde**: retângulo, círculo, arco, estrela, coração, selo serrilhado, polaroide. Geometria pura no núcleo.
- **Laço e polígono**: o caminho que o usuário traça, suavizado.
- **Pincel de adicionar e apagar**: pinta no mapa alfa com a mesma lógica de pincel do storyboard (`board/brush.rs`), em resolução reduzida e ampliada com borda suave.
- **Seleção automática do objeto**: segmentação de primeiro plano.
  - No macOS 14 ou mais novo: `VNGenerateForegroundInstanceMaskRequest` do Vision, o mesmo motor de "copiar objeto" das Fotos. Chamado por um pequeno binário auxiliar em Swift empacotado no `.app` (ou pelo crate `objc2-vision`), que recebe o caminho da imagem e devolve o PNG da máscara.
  - Nos outros sistemas: um modelo ONNX de recorte (como o BiRefNet leve ou o U²-Net), baixado na primeira vez, como o CLIP já é, rodando pelo mesmo `ort` da feature `vision`.
  - Um clique sobre a imagem escolhe a instância mais próxima do ponto, como o *Select* do Pinterest.
- **Gaveta de adesivos**: todo recorte pode ser guardado como adesivo reutilizável (um PNG com transparência numa coleção "Adesivos" da biblioteca), com a origem preservada.

### 7.2 Aparência (`core/src/journal/look.rs`)

- **Borda de adesivo**: dilatação do alfa da máscara pelo raio escolhido, preenchida de branco (ou outra cor), com uma leve irregularidade opcional para parecer cortada à tesoura. É o que aparece no anjo e na nuvem da referência. A sombra é opcional e curta, coerente com a sombra leve da lombada.
- **Borda rasgada**: deslocamento da máscara por ruído ao longo do contorno, de um lado ou de todos.
- **Fita adesiva**: um retângulo semitransparente de cor lisa, com pontas serrilhadas, que pode prender qualquer item.
- **Filtros**: os do compositor (`pb`, `sepia`, `contraste`, `desfoque`) e quatro novos, no mesmo `compose::filter` para que a tela, o PNG e o PDF fiquem iguais: `granulado` (ruído com semente fixa por item), `fotocopia` (limiar suave com perda de meios-tons), `reticula` (pontos de meio-tom) e `impressao` (saturação menor e preto levantado, como impressora doméstica). Mais o modo de mistura "multiplicar", para que a pauta apareça através de uma impressão.

A ordem das camadas muda pelo balão do item (trazer para frente, mandar para trás) e por atalhos; uma lista de camadas da página abre sob demanda, para páginas cheias. Cada item mostra sua origem (pin, arquivo, livro do Leitor), e um clique na origem a abre.

### 7.3 Texto na pauta e em volta das imagens (`core/src/journal/flow.rs`)

O compositor do Ateliet já compõe texto numa grade de linhas (é o que a Diagramação usa para alinhar o livro). No Journal, a grade é a pauta do papel: a entrelinha é o espaçamento da pauta (ou um múltiplo dela, para fontes maiores) e a primeira linha de base cai numa linha do papel. No papel liso, a grade existe sem aparecer.

Para contornar imagens, o compositor passa a aceitar uma largura por linha: para cada linha, a largura útil é a da coluna menos as áreas dos itens marcados com `wrap`. Com `wrap: Contorno` a área é a caixa do item; com `wrap: Forma`, o contorno da máscara mais uma folga. O Knuth–Plass já trabalha com larguras por linha, então a mudança é entregar a largura certa a cada linha, não trocar o algoritmo.

Quando o texto passa do fim da coluna, a sobra vai para uma caixa ligada (`flow`) na página seguinte, criada no mesmo lugar da coluna; mover ou redimensionar uma caixa recompõe a sequência.

O texto parado é desenhado pelo compositor numa textura e, quando se clica nele, vira o editor de Markdown do Ateliet no lugar (o mesmo das notas, com Live Preview), na largura da caixa. É a mesma troca entre leitura e edição que o Live Preview faz por bloco.

O texto pode usar a fonte do tema, as fontes embutidas da Diagramação ou uma fonte manuscrita (uma fonte livre como Caveat, embutida, ou uma do sistema), para comentários que devem parecer escritos à mão sem ter sido.

## 8. Os trechos

Todo material de texto que chega de fora da página vira um **trecho**, e o Journal só conhece esse objeto. Assim, levar um parágrafo do Leitor, de uma página web ou do próprio manuscrito do Scriptorium é a mesma operação.

```rust
// core/src/capture/mod.rs
pub struct Snippet {
    pub id: String,
    pub text: String,            // o trecho, em Markdown
    pub image: Option<String>,   // o recorte de região, quando é uma imagem (LEITOR.md, 6.2)
    pub note: Option<String>,    // comentário que veio junto
    pub source: SourceRef,
    pub captured: DateTime,
    pub origin: Origin,          // Leitor, Web, Scriptorium, Manual
}

pub struct SourceRef {
    pub title: String,
    pub author: Option<String>,
    pub book: Option<String>,    // nota do livro no vault ([[Livros/…]]), se houver
    pub url: Option<String>,
    pub locator: Option<Locator>,// onde no livro (LEITOR.md, seção 3)
}
```

Cada trecho vira uma nota curta em `Trechos/` (pasta configurável), com o texto num callout `[!trecho]` e a fonte no frontmatter. Notas, e não um JSON escondido, porque assim os trechos ficam pesquisáveis, ligáveis e consultáveis pelo Dataview mesmo sem o Journal ("todos os trechos de tal livro", "trechos ainda não usados"). Quando um trecho é posto numa página, a página ganha um link para a nota do trecho, e o trecho ganha o backlink: de cada trecho dá para ver em que página do caderno se escreveu sobre ele.

**O Leitor** ([LEITOR.md](LEITOR.md)) é a fonte principal: a seleção de texto e o recorte de região de EPUBs, PDFs e quadrinhos produzem trechos com um `Locator` que reabre o livro no ponto exato.

**A caixa de entrada** (`core/src/capture/inbox.rs`, `src/capture.rs`) recebe o que vem de fora do app:

- **Pasta**: `.obsidian/ateliet/inbox/`. Qualquer arquivo `.json` ali com o formato de `Snippet`, um `.txt` ou `.md` simples, ou uma imagem, é importado e apagado. Com o vault sincronizado (iCloud Drive, Syncthing), é por aqui que chegam as fotos de páginas manuscritas tiradas no celular (seção 9).
- **Esquema de URL**: `ateliet://capture?text=…&title=…&url=…`, registrado no `Info.plist` (`CFBundleURLTypes`) no macOS e num arquivo `.desktop` no Linux, para levar uma seleção do navegador por um favorito.
- Cada trecho que chega mostra um aviso discreto com "Pôr na página aberta" e "Guardar". O painel **Trechos** do Journal lista os que ainda não foram usados.

## 9. Digitalizar as notas escritas à mão (`core/src/ocr/`, `src/journal_ui/scan_ui.rs`)

A digitalização tem duas metades, e só a segunda depende de reconhecimento de texto.

### 9.1 A imagem

1. **Entrada**: uma foto do celular (pela caixa de entrada, seção 8, ou pela Câmera de Continuidade do macOS), um arquivo de scanner ou uma imagem arrastada.
2. **Página**: detecção do quadrilátero da folha (bordas e o maior contorno com quatro cantos), correção de perspectiva e recorte, com os quatro cantos ajustáveis à mão.
3. **Limpeza**: balanço de branco pelo papel, uniformização da iluminação (divisão por uma versão muito desfocada da própria imagem) e, opcionalmente, retirada do quadriculado ou das pautas.
4. **Tinta separada**: um limiar adaptativo transforma o papel em transparência e deixa só os traços. A tinta vira um item `Scan` sem fundo, que se assenta sobre o papel do Journal como se tivesse sido escrita nele, pode ser recolorida (grafite, nanquim, azul) e recortada como qualquer imagem. É assim que um desenho feito num caderno de papel, como o anjo da referência, vira um adesivo.
5. **Página inteira**: uma folha digitalizada pode também virar uma página do caderno, com a tinta sobre o papel do caderno, para quem escreve no papel e guarda no Ateliet.

Tudo isso é processamento de imagem simples, no núcleo, com o crate `image` que o Ateliet já usa.

### 9.2 A transcrição

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

O resultado vai para o corpo da nota como um callout `[!transcricao]` dobrado sob a imagem (6.2). Assim a transcrição é pesquisável no Ateliet, no Obsidian e no Dataview, enquanto a página mostra a letra original. As caixas de cada palavra ficam no JSON, o que permite três coisas: a busca destaca a palavra na imagem; o usuário corrige a transcrição clicando na linha errada; e uma linha manuscrita pode ser convertida em texto digitado na pauta, no mesmo lugar, se ele quiser.

## 10. Convivência com os plugins

O Ateliet não roda plugins da comunidade; os "plugins" com que o Journal precisa conviver são os que ele traz embutidos e os que o Obsidian continua rodando no mesmo vault. O formato da seção 6 foi escolhido para isso, e cada um ganha uma ligação concreta:

- **Obsidian**: o caderno é uma pasta de notas Markdown legíveis, em ordem; imagens são embeds `![[…]]`; o bloco `%% … %%` é um comentário que o Obsidian esconde. Nenhum plugin do Obsidian é necessário para ler um caderno.
- **Dataview**: o frontmatter (`journal`, `caderno`, `pagina`, `data`, `fontes`, `tags`) e os campos inline nos grifos permitem consultas como "páginas deste caderno", "trechos de tal livro ainda sem página", "páginas com imagens de tal coleção". A nota-índice do caderno já traz o sumário como uma consulta. Uma consulta pode também virar um item de texto vivo numa página.
- **Templater**: um modelo de página é uma página de Journal comum numa pasta de modelos. Ao criar uma página a partir dele, o Templater roda no texto (datas, prompts) e a geometria é copiada. Um caderno inteiro pode ter um modelo padrão para as páginas novas.
- **Notas diárias e Calendário**: um caderno pode ser marcado como diário; "Página de hoje" acrescenta a página com a data no fim do caderno (ou abre a de hoje, se já existir), e o calendário mostra os dias que têm página.
- **Moodboards**: selecionar itens de um moodboard e "Mandar ao Journal" leva as imagens com recorte, giro e espelho (os campos são os mesmos) para a página aberta.
- **Quadro**: uma página de Journal pode ser um cartão num quadro Kanban ou num desenho do Excalidraw, com prévia.
- **Scriptorium**: um trecho pode vir de um capítulo do próprio manuscrito (origem `Scriptorium`, com link para o capítulo), e uma página do Journal pode entrar como página composta no livro.
- **Pictogrep e Pinterest**: imagens e recortes da biblioteca compartilhada são guardados pelo id do Pictogrep; um recorte feito no Journal pode ser salvo de volta na biblioteca como adesivo, com o pin de origem.
- **Busca e backlinks**: tudo o que é texto está no corpo das notas.

## 11. A interface

O Journal é o terceiro modo na faixa lateral esquerda, ao lado de Escrita e Imagens (`Ctrl/Cmd+Shift+J`), com seu próprio layout guardado em `workspace-journal.json`. Uma página de Journal também abre como aba no modo escrita, mostrando a página dupla daquele ponto do caderno.

- **Estante**: a primeira tela do modo, quando nenhum caderno está aberto. Capas em fila, "Novo caderno" (nome, tamanho, papel, colunas, cor), e o último caderno em destaque.
- **O caderno aberto**: a página dupla no centro, sobre o fundo do app, sem outra moldura. Virar a página: setas, gesto no trackpad, clique no canto inferior. `Ctrl/Cmd+G` vai a uma página pelo número. A vista de miniaturas (todas as páginas em pares, como um caderno visto de cima) serve para navegar e reordenar.
- **Barra de ferramentas**: pequena, no pé, centrada, e escondida enquanto não é necessária: selecionar, escrever, caneta, imagem, recorte, linha, fita, adesivo. Uma ferramenta por vez, sem submenus abertos.
- **Balão do item**: ao selecionar um item, um balão junto dele mostra só o que serve para aquele tipo: girar, recortar, borda de adesivo, filtro, contornar texto, camada, origem.
- **Material**: três painéis que deslizam da esquerda quando chamados (pela barra ou por atalho) e se recolhem quando algo é arrastado para a página. Cada um usa o que já existe no app:
  - **Imagens**: a biblioteca do modo imagens (coleções, busca por nome e por descrição com o CLIP, imagens parecidas, a gaveta de adesivos). Arrastar para a página cria um item; arrastar sobre outra imagem a coloca por cima.
  - **Notas**: notas recentes, busca e as notas abertas no modo escrita. Arrastar uma nota a incorpora ao vivo (o texto acompanha a nota), e com Alt a copia para a pauta. Uma nota aberta no painel pode ser editada ali mesmo, com a barra do painel de notas do modo imagens.
  - **Trechos**: os trechos ainda não usados (do Leitor, da caixa de entrada, digitalizações pendentes) e as notas de `Trechos/`.
- **Ler ao lado**: o Leitor numa metade da janela e o caderno na outra, para arrastar trechos e recortes direto para a página (LEITOR.md, 6.4).
- **Modo escrita**: um comando "Mandar ao Journal" na seleção do editor e no menu de imagens; o painel Pictures ganha "Pôr na página aberta".
- **Modo imagens**: "Mandar ao Journal" na seleção da biblioteca e no visualizador.

Exportação: PNG e PDF por página, por intervalo ou do caderno inteiro (com capa), pelo rasterizador e pelo PDF do compositor, que já embutem fontes e imagens; em páginas simples ou em pares, com ou sem a pauta.

## 12. Ordem de implementação

Cada fase entrega algo usável sozinho e prepara a próxima. O Leitor ([LEITOR.md](LEITOR.md), fases 1 a 5) vem antes: ele já guarda trechos e recortes no vault, e o Journal os recebe quando chegar.

1. **O caderno**: `journal/mod.rs`, os formatos da nota-índice e da página com testes de ida e volta byte a byte (como os do Quadro), papéis e colunas, numeração e ordem. Na interface: estante, capa, página dupla, virar páginas, miniaturas, a moldura que some.
2. **Escrever e colar**: texto na pauta com continuação na página seguinte; imagens; `canvas.rs` extraído do moodboard; mover, girar, camadas, desfazer; arrastar dos painéis de Imagens e Notas.
3. **Trechos**: o modelo `Snippet`, notas em `Trechos/`, caixa de entrada por pasta, esquema `ateliet://`, o painel Trechos. Ligação com o Leitor e "Ler ao lado" (LEITOR.md, fase 7).
4. **Aparência**: recorte por formas, laço e pincel; borda de adesivo, fita, borda rasgada; os quatro filtros novos no compositor; gaveta de adesivos.
5. **Texto contornando imagens**: largura por linha no compositor, troca entre texto composto e editor no lugar, fontes manuscritas.
6. **Digitalização**: perspectiva, limpeza, tinta separada, folha digitalizada como página; transcrição pelo Apple Vision; correção de linhas; motor ONNX e a opção pela API.
7. **Recorte automático**: segmentação de primeiro plano (Vision e ONNX).
8. **Integrações restantes**: modelos com Templater, página do dia, moodboard → Journal, cartão no Quadro, exportação do caderno.

## 13. Dependências novas e riscos

- **Crates**: `objc2-vision` ou um auxiliar em Swift no macOS, para o recorte automático e a transcrição. Os modelos ONNX de recorte e de escrita são baixados na primeira vez, como o CLIP, e entram pela feature `vision`, para que um build sem ela continue funcionando, só sem os recursos automáticos. As dependências do Leitor estão no documento dele.
- **Pressão da caneta**: o egui não a fornece no macOS hoje. O traço usa velocidade; ler a pressão exigiria tratar os eventos do `winit` diretamente.
- **Edição dos dois lados**: uma página pode ser editada no Obsidian enquanto o Ateliet a tem aberta. A releitura pelo watcher e a gravação que relê o disco antes (como no Quadro) resolvem o caso comum; um conflito na mesma seção fica com a versão do disco e avisa. Reordenar páginas renomeia arquivos, e o `relink.rs` já atualiza os links para eles.
- **Texto que continua**: a ligação entre caixas de páginas diferentes é a parte mais delicada do modelo, porque cada página é um arquivo. A sequência fica guardada nas duas pontas (`flow` com o id da caixa seguinte e da anterior) e é refeita pela ordem das páginas quando uma delas some.
- **Transcrição**: a qualidade em letra cursiva em português é o maior risco técnico do projeto, e ele é contido pelo desenho: a imagem é sempre o original, e a transcrição só serve para busca e para quem quiser corrigi-la.

## 14. Estado da implementação

O que já está no código, fase por fase, e o que falta. O núcleo está em `core/src/journal/`, `core/src/capture/` e `core/src/canvas.rs`; a interface em `src/journal_ui/` e `src/journal_mode.rs`.

1. **O caderno: feito.** Nota-índice e páginas no formato da seção 6, com testes de ida e volta byte a byte, de junção com mudanças feitas no disco e de restauração do bloco perdido. Papéis (liso, pautado, quadriculado, pontilhado), colunas (Cornell nas quatro posições, duas colunas), tamanhos. Estante com capas, caderno novo com folha de rosto, página dupla com lombada e volume, virar páginas (setas, `PageUp`/`PageDown`, deslize no trackpad, `Ctrl/Cmd+G`), marcador, miniaturas com reordenação (os arquivos são renomeados e os links seguem), página nova no fim ou depois da atual, apagar página, a barra que some. Falta: marcadores extras em lingueta, a vista de uma página só, virar clicando no canto, escolher a imagem da capa pela interface (`capa-imagem` no frontmatter já é lida).
2. **Escrever e colar: feito.** Texto na pauta, com continuação na página seguinte na coluna de mesmo papel; imagens do painel de Imagens; notas ao vivo e cópia com `Alt` pelo painel de Notas; mover, girar, redimensionar, encaixe nas linhas da página e nos outros itens, camadas, travar, duplicar, desfazer; itens que passam para a outra página ou atravessam a lombada; legenda presa à imagem. Diferente do plano: o texto é editado como Markdown puro num campo no lugar e exibido com negrito, itálico, grifo, links e tags formatados, e não pelo editor Live Preview do modo escrita. Falta: a lista de camadas sob demanda.
3. **Trechos: em parte.** Feitos o modelo `Snippet`, as notas em `Trechos/`, a caixa de entrada por pasta, o painel Trechos (com os já usados marcados) e "Journal: Mandar a seleção ao Journal". Faltam o esquema `ateliet://` e tudo o que depende do Leitor, que ainda não existe ("Ler ao lado", recortes de região).
4. **Aparência: em parte.** Feitos o recorte retangular, as formas de molde, a borda de adesivo com sombra, a fita adesiva, os quatro filtros novos no compositor e a gaveta de adesivos (o recorte vira um PNG com transparência em `Adesivos/`, e a forma dele fica em `.obsidian/ateliet/journal/adesivos.json` para a borda seguir o contorno). "Deixar a pauta aparecer" deixa a imagem um pouco translúcida, em vez de um modo de mistura de verdade. Faltam o laço e o pincel de máscara e a borda rasgada.
5. **Texto contornando imagens: não feito**, a não ser as fontes manuscritas (Caveat, embutida). O campo `wrap` já existe no formato.
6. **Digitalização: não feita.** As fotos que chegam pela caixa de entrada viram trechos com imagem, que podem ser postos na página, mas sem perspectiva, limpeza, separação da tinta nem transcrição.
7. **Recorte automático: não feito.**
8. **Integrações: em parte.** Feitos a página do dia nos cadernos diários, "Mandar ao Journal" no menu das imagens do modo imagens e a consulta Dataview da nota-índice. Faltam os modelos com o Templater, moodboard → Journal, a página como cartão no Quadro, a página aberta como aba no modo escrita e a exportação em PNG e PDF.

## Fontes consultadas

- Pinterest, [Create a collage](https://help.pinterest.com/en/article/create-a-collage)
- Metricool, [Pinterest-Collagen: So nutzt du die neuen Features](https://metricool.com/de/pinterest-collage-neue-features/)
- Outfy, [Pinterest new collage features](https://www.outfy.com/blog/pinterest-new-collage-features/)
- Social Media Today, [Pinterest adds collage remix and sharing options](https://www.socialmediatoday.com/news/pinterest-adds-collage-remix-sharing-options/727961/)
