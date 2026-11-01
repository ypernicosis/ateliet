# Vídeo integrado à área de trabalho

A revisão de continuidade e acabamento está em [VIDEO_FLUIDEZ.md](VIDEO_FLUIDEZ.md).
Ela substitui as decisões de dicas, suspensão e controles descritas na
primeira integração abaixo.

## Arquitetura e decisão de experiência — revisão de 31/10/2026

O vídeo pertence à sessão do Ateliet. A superfície principal é um bloco no
painel de apoio, dentro da janela do app. Ele reserva espaço no layout e não
encobre o texto. A janela destacada passa a ser uma escolha explícita.

Esta revisão corrige a primeira entrega: uma janela auxiliar por padrão e
um seletor em popup não integravam o vídeo ao trabalho.

### Distribuição do espaço

| Área de trabalho | Lugar do vídeo |
| --- | --- |
| Notas, desenho e storyboard | Acima das ferramentas do painel direito; calendário, estrutura e referências continuam abaixo. |
| Imagens e moodboard | Acima da nota lateral, usando a mesma coluna. |
| Livro aberto ao lado | Acima do leitor, sem criar uma segunda coluna de apoio. |
| Journal, leitura imersiva e foco | Painel direito aberto apenas por solicitação; recolher devolve o espaço ao trabalho. |
| Vídeo recolhido | Faixa baixa no rodapé com título e ação de retomar, disponível em todos os modos. |

A largura usa o divisor já existente. Em janelas estreitas, a navegação à esquerda é temporariamente recolhida
sem alterar sua preferência; as ferramentas da nota passam para um menu.
O vídeo preserva a proporção, com
letterbox quando necessário; o YouTube mantém pelo menos 200 × 200 pontos
visíveis. Em janelas sem espaço suficiente, a mídia pausa e oferece
recolher ou destacar, sem sobrepor a área de criação.

### Percurso principal

1. O botão Vídeo da barra ou ⌘⌥V revela o bloco no painel de apoio.
2. O próprio bloco oferece arquivo local, link do YouTube e recentes.
   Colar + Enter reproduz. Não há diálogo adicional para confirmar o link.
3. Os controles do player permanecem junto do vídeo. Trocar fonte abre o
   seletor no mesmo bloco; a fonte atual continua até a escolha seguinte.
4. Recolher pausa, preserva a posição e devolve o espaço ao editor. A faixa
   inferior permite continuar e revelar o vídeo num único gesto.
5. Destacar move a mesma superfície para uma janela auxiliar. “Voltar ao
   Ateliet” encaixa o mesmo player. Nenhuma troca de modo ou de posição
   recarrega a mídia. Fechar a janela destacada recolhe e pausa.
6. Encerrar a sessão libera player e rede, preservando os recentes.

A reprodução nunca começa ao iniciar o app. Arquivos locais são usados na
origem, sem importação. Arrastar para o bloco de vídeo abre a fonte; arrastar
para a nota mantém o comportamento de anexos. Atalhos não capturam Espaço
ou outros comandos de digitação. Abrir/recolher o vídeo não altera a
preferência de visibilidade dos painéis de notas e imagens.

## Separação de responsabilidades

| Camada | Responsabilidade |
| --- | --- |
| `core/src/work_video.rs` | Fontes, URLs, timestamps e histórico limitado. |
| `src/work_video.rs` | Estado de apresentação (fechado, encaixado, recolhido, destacado), seletor inline, faixa compacta, geometria, comandos e persistência. |
| Painéis do app | Reservar um único retângulo no painel de apoio do modo atual. |
| `src/work_video/native.rs` | Ponte proprietária, restrita à thread principal, usando o handle real da janela do eframe. |
| `native/work_video.m` | Uma superfície nativa reaproveitada; AVPlayerView ou WKWebView; NSPanel criado somente ao destacar. |

O app calcula a geometria em pontos egui; a ponte converte para as coordenadas
da view AppKit, inclusive escala e orientação. A view é filha da janela
real do eframe, não uma janela escolhida por título ou ordem do sistema.
O encaixe só muda frame/parent; não recria o decodificador. Nenhum quadro
atravessa a FFI. O egui consulta pequenos snapshots uma vez por segundo.

A sincronização acontece depois de desenhar os painéis e diálogos. Se um
janela cobre o retângulo, a view nativa é temporariamente ocultada e
pausada para não interceptar cliques nem encobrir o diálogo. Ao liberar o
espaço, só retoma se estava tocando antes dessa interrupção. Recolher ou
fechar é uma pausa explícita, sem retomada automática.

## Contratos

- Um único backend por sessão; mudar nota, cofre ou modo não muda a fonte.
- Trocar fonte, encerrar e sair invalidam callbacks atrasados.
- IDs/hosts do YouTube são validados; conteúdo arbitrário não vira script.
- Player oficial com controles e marca visíveis; sem extração de áudio,
  downloads ou reprodução escondida. Referer identificado pelo Bundle ID.
- Erros e recuperação ficam abaixo do vídeo, nunca por cima dos controles.
- Arquivo ausente, codec não reconhecido, erro de rede ou incorporação
  proibida oferecem troca de fonte e abertura externa.
- Histórico global de oito fontes, posição gravada atomicamente e em baixa
  frequência. Nenhuma mídia é carregada só para abrir o seletor.
- O backend integrado é macOS; outras plataformas conservam abertura externa.

## Validação

Os testes do executável passaram: 23 casos, incluindo seis regressões do
vídeo. Cobrem os seis modos do app, retomada da apresentação, preservação
das preferências de painéis, espaço de escrita em 640 × 400, seletor com
títulos longos, sobreposição de diálogos e estabilidade da largura ao trocar
a fonte, além da faixa recolhida no modo foco. A prévia egui por CPU foi executada separadamente e suas cinco
imagens foram inspecionadas; o vídeo nessas imagens é um placeholder.

O teste AppKit `native/tests/work_video_test.m --layout` passou, verificando
parentesco da view, conversão de coordenadas/escala, ausência de NSPanel na
abertura padrão e identidade da superfície ao destacar e voltar. O build
de produção e a compilação Objective-C com `-Wall -Wextra -Werror` passaram.

A parte de reprodução do teste nativo retornou 77 (não executada): o serviço
de encoder do macOS recusou gerar a referência, com AVFoundation -11800 /
OSStatus -12903. A reprodução com áudio, o YouTube online e a composição
AppKit sobre o OpenGL em uma sessão gráfica real ainda precisam de
validação. Os testes de layout não demonstram esses comportamentos.

Para repetir a inspeção sem iniciar mídia:

```sh
ATELIET_VIDEO_PREVIEW=target/video-layout cargo test -p ateliet --no-default-features --offline render_layout_preview -- --ignored
```

## Base de design consultada

- Apple HIG, Layout: https://developer.apple.com/design/human-interface-guidelines/layout
- Apple HIG, Split views: https://developer.apple.com/design/human-interface-guidelines/split-views
- Apple HIG, Playing video: https://developer.apple.com/design/human-interface-guidelines/playing-video
- Apple, Inspectors in SwiftUI: https://developer.apple.com/videos/play/wwdc2023/10161/
- Apple, View hierarchy: https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/CocoaViewsGuide/WorkingWithAViewHierarchy/WorkingWithAViewHierarchy.html
- YouTube, Required Minimum Functionality: https://developers.google.com/youtube/terms/required-minimum-functionality
- YouTube IFrame Player API: https://developers.google.com/youtube/iframe_api_reference

As escolhas acima são decisões de projeto para o fluxo do Ateliet, apoiadas
nessas referências e na estrutura existente. Não são resultados de pesquisa
com usuários.
