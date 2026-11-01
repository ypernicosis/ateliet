# Vídeo: continuidade e acabamento

## Diagnóstico antes da implementação

O player já está integrado à coluna de apoio. A interrupção relatada tem
causas observáveis no código:

- `unobscured_rect` trata dicas, menus e janelas como interrupções equivalentes;
  qualquer interseção remove o vídeo inteiro e pausa o transporte.
- Ao ocultar, a ponte muda a view para um retângulo de tamanho zero. O
  WebKit precisa recompor o player na volta.
- O fundo egui escreve “Preparando vídeo” permanentemente. Ocultar uma
  fonte pronta parece um novo carregamento.
- O estado `playing = false` mistura pausa, carregamento e buffering. A
  intenção de continuar pode se perder durante uma interrupção.
- Efeitos nativos acontecem até em passes egui que serão descartados.

## Decisões de experiência

A escrita é o centro. O vídeo mantém sua posição e sua sessão enquanto a
pessoa navega, escreve, usa dicas e redimensiona o painel. A reprodução só
é suspensa quando fica de fato invisível, é recolhida ou recebe uma pausa.
Uma pausa do usuário prevalece sobre qualquer retomada automática.

O cabeçalho terá o título da fonte, reproduzir/pausar, menu e recolher.
Trocar fonte, destacar, abrir fora e encerrar ficam no menu. O seletor
continua inline, abaixo da mídia, e não movimenta o vídeo ao abrir. O
painel ganha um divisor vertical arrastável para escolher a altura do vídeo;
a altura é preservada entre sessões. A faixa recolhida usa a mesma linguagem
visual, com título à esquerda e uma ação de revelar.

O estado informa abertura, reprodução, espera de dados, pausa e falha.
Buffering usa os controles oficiais do player. O fundo egui não mostra
mensagem de carregamento: o wrapper informa conexão antes de criar o iframe,
e esse texto desaparece quando o YouTube substitui o elemento pelo player.
Falhas oferecem tentar novamente na posição atual ou trocar a fonte.
Uma mudança de layout nunca se apresenta como carregamento de mídia.

Dicas do app que se aproximam do player são reposicionadas fora dele.
O menu do próprio vídeo abre ao lado da coluna, mantendo a reprodução.
Janelas e menus que realmente cobrem a mídia continuam recebendo prioridade;
a suspensão mantém a geometria e a intenção de reprodução. O player oficial
do YouTube permanece sem controles ou máscaras sobre sua superfície.

## Execução

1. Separar estado de transporte, intenção e visibilidade. Corrigir buffering,
   callbacks tardios e comandos enviados antes de o YouTube estar pronto.
2. Preservar o frame nativo ao ocultar; evitar trabalho nativo em passes
   descartados e reposicionar dicas antes da decisão de suspensão.
3. Reduzir controles permanentes, permitir ajuste de altura, introduzir
   recuperação explícita e remover indicadores de carregamento falsos.
4. Dar acabamento ao atualizador: preparação em segundo plano com limite de
   compilação, texto orientado ao uso e detalhes técnicos recolhidos.
5. Testar as transições que provocavam o problema, o ciclo de pausa/retomada
   e a composição da interface; compilar e inspecionar prévias reais do egui.

## Critérios para aceitar a mudança

| Percurso | Resultado esperado |
| --- | --- |
| Dica sobre um botão junto ao vídeo | Dica deslocada, player continua visível, nenhum comando de pausa. |
| Abrir e fechar uma janela sobre o vídeo | View mantém tamanho e fonte; retoma se havia intenção de tocar. |
| Buffering antes da suspensão | Retoma a intenção de tocar; não é confundido com pausa manual. |
| Pausar, abrir e fechar janela | Continua pausado. |
| Recolher antes do YouTube ficar pronto | Não toca escondido quando a API responde. |
| Passes de medição do egui | Não alteram a view nativa. |
| Trocar fonte e mudar altura | Editor permanece utilizável; largura estável; sem duplicar o player. |
| Atualização em segundo plano | Até dois processos de compilação, menor prioridade no Unix, reinício escolhido pela pessoa. |

A verificação usa testes de transporte independentes de decoder e um teste
AppKit da geometria real, além de testes egui e build. A reprodução real e
YouTube online serão tentados quando o ambiente permitir. Resultados de
simulação não serão apresentados como reprodução validada.

## Referências técnicas

- AVPlayer.TimeControlStatus: https://developer.apple.com/documentation/avfoundation/avplayer/timecontrolstatus-swift.property
- YouTube IFrame API: https://developers.google.com/youtube/iframe_api_reference
- Requisitos do player: https://developers.google.com/youtube/terms/required-minimum-functionality
- egui 0.31, Context::will_discard e transform_layer_shapes (fontes da versão usada pelo app).

## Resultados da revisão

- 30 testes do executável passaram, incluindo a geração de sete prévias para
  inspeção visual. Cobrem dica junto ao vídeo, menu fora da mídia, janela
  sobreposta, altura ajustável, largura estável e abertura imediata no modo
  Imagens com o painel de notas fechado, além do foco e substituição imediata
  do link pelo teclado sem retomar uma fonte pausada.
- Quatro testes do modelo passaram, incluindo migração do histórico antigo e
  persistência/sanitização da altura do painel.
- `work_video_web_test.m` executa o JavaScript de produção em JavaScriptCore
  com um player assíncrono controlado. Passaram abertura tardia, recolhimento
  antes de ready, buffering, pausa manual, comandos repetidos/desatualizados,
  retorno rápido, término e erros do YouTube.
- `work_video_test.m --layout` usa o controlador de produção e uma view AppKit
  real. Passaram geometria, encaixe/destaque/retorno e manutenção do frame ao
  ocultar. Um probe de transporte verifica espera de dados, pausa manual e
  ready tardio sem depender de um decoder.

Os testes de transporte acima verificam as decisões do app, não a rede ou o
decodificador. Os serviços de mídia do ambiente já recusaram a geração de
referência com OSStatus -12903; reprodução audiovisual real, YouTube online e
composição final sobre OpenGL permanecem fora da validação realizada.

Comandos de verificação adicionais estão nos cabeçalhos dos dois testes em
`native/tests/`. A compilação de produção e a checagem Objective-C com warnings como erros
também passaram antes de publicar a mudança.
