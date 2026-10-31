# Vídeo durante o trabalho

## Decisão antes da implementação

O vídeo pertence à sessão do aplicativo, não à nota, ao livro ou ao modo de
trabalho. Trocar de aba, cofre, Journal, Imagens, Leitor ou modo foco não
recria o player. O espaço de escrita mantém suas dimensões.

Uma janela auxiliar nativa, inicialmente de 480 × 270 pontos de vídeo, é a
superfície principal. Pode ser movida, redimensionada e fixada acima das
outras janelas. Controles conhecidos e proporção preservada permitem usá-la
como referência visual ou companhia sem aprender outro ambiente.

## Percurso

- Botão Vídeo na barra de ferramentas e comandos pesquisáveis na paleta.
- Um pequeno seletor oferece arquivo do computador, URL do YouTube e recentes.
  Colar um link só inicia a reprodução depois de confirmar; a área de
  transferência e o texto do editor não são interceptados.
- Arquivos locais são referenciados, sem cópia para o cofre. Vídeos abertos
  pela árvore de arquivos usam a mesma sessão. Soltar arquivos no seletor
  abre o vídeo; soltar na nota conserva a inserção de anexos existente.
- Reabrir um recente retoma sua posição. Iniciar o aplicativo nunca abre ou
  reproduz vídeos automaticamente. Uma sessão nunca produz dois players.
- Fechar ou minimizar pausa. Fechar libera decodificação e rede. Um comando
  explícito traz a janela de volta; a posição permanece nos recentes.
- A barra de estado oferece acesso à sessão. O atalho para abrir o seletor
  funciona também nos modos imersivos, sem capturar Espaço no editor.

## Camadas

| Camada | Responsabilidade |
| --- | --- |
| `core/src/work_video.rs` | Fontes tipadas, validação estrita de URLs e timestamps, histórico limitado e atualização das posições; sem janela ou rede. |
| `src/work_video.rs` | Seletor egui, comandos da sessão, ponte com backend, persistência global fora do cofre. |
| `src/work_video/native.rs` | FFI com propriedade explícita e uso restrito à thread principal. |
| `native/work_video.m` | NSPanel; AVPlayerView para arquivo, WKWebView para YouTube, callbacks de estado e descarte de recursos. |

AVKit fornece áudio e vídeo sincronizados, seek, volume, tela cheia e PiP
para formatos reconhecidos pelo macOS, sem ffmpeg ou tráfego externo.
WKWebView só é criado ao abrir YouTube e mantém o player oficial visível,
com controles e marca. Não há download, extração de áudio, API key nem
substituição de anúncios. O referer usa o Bundle ID do Ateliet.

A camada nativa devolve snapshots pequenos, sem copiar quadros para a GPU
do egui. Progresso não provoca renderização contínua do editor. A thread
principal não percorre bibliotecas nem decodifica mídia. O estado
é consultado em baixa frequência e gravado de forma atômica e espaçada,
somente quando mudou, num arquivo próprio ao lado de `ateliet.json`.

## Contratos e falhas

- Aceitar apenas HTTP(S), hosts conhecidos do YouTube e IDs de 11 caracteres;
  aceitar watch, youtu.be, shorts, live, embed e timestamps compartilhados.
- Nenhuma string de URL arbitrária entra como JavaScript. Dados são JSON
  serializado. A ponte de mensagens só aceita mensagens do documento principal.
- Falha de codec/arquivo removido permanece visível e permite trocar a fonte.
- Rede, bloqueio de autoplay e erros 100/101/150/153 do YouTube têm mensagem
  acionável; o navegador é uma saída explícita.
- Fechar, substituir e sair invalidam callbacks anteriores, pausam e liberam
  o backend anterior. Posição e título só atualizam a fonte correspondente.
- Builds fora do macOS continuam compilando, mas apresentam a limitação e
  oferecem abertura externa. O backend incorporado desta entrega é macOS.
- Música/Trilha conserva controles independentes; nenhuma aplicação externa
  é pausada sem solicitação do usuário.

## Validação

Testes unitários cobrem normalização e rejeição de URLs, timestamps,
deduplicação e limite de recentes. Compilar o backend Objective-C com
warnings e o app Rust; verificar manualmente arquivo com áudio, retomada,
troca de fonte, fechar/minimizar, redimensionamento, fixação, navegação entre
modos, teclado e YouTube online (incluindo vídeo com incorporação proibida).
Testes de rede exigem acesso ao YouTube e não são substituídos por mocks.

`native/tests/work_video_test.m` é um teste de ciclo de vida independente
(o comando de compilação está no próprio arquivo). Gera uma referência
temporária, verifica avanço de posição, pausa, fixação, fechamento, liberação,
reabertura e callbacks antigos. Retorna 77 se o macOS não disponibilizar o
encoder para gerar a referência; isso significa teste não executado.

Na implementação inicial, a compilação de produção e a variante sem features
padrão passaram, assim como os 197 testes do núcleo (um teste de rede já
marcado como ignorado). O backend Objective-C compilou com `-Wall -Wextra
-Werror`. O teste de interface sem janela nativa também passou: o seletor,
com oito títulos longos, cabe numa área de 640 × 400 e não inicia mídia ao
abrir. A reprodução real não foi validada neste ambiente: LaunchServices
e os serviços de mídia do macOS recusaram a sessão de teste, e o encoder
retornou AVFoundation -11834 / OSStatus -12903. Continuam pendentes os
percursos gráficos e a reprodução online descritos acima.

## Referências consultadas

- Apple HIG, Playing video: https://developer.apple.com/design/human-interface-guidelines/playing-video
- Apple AVPlayerView: https://developer.apple.com/documentation/avkit/avplayerview
- Apple NSPanel: https://developer.apple.com/documentation/appkit/nspanel
- YouTube, Required Minimum Functionality: https://developers.google.com/youtube/terms/required-minimum-functionality
- YouTube IFrame Player API: https://developers.google.com/youtube/iframe_api_reference

As escolhas de superfície e fluxo são decisões de projeto para o Ateliet;
não constituem resultados de testes com usuários.
