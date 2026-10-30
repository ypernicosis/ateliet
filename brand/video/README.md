# vídeo

`ateliet.mp4`: 15 segundos, 1920×1080, 60 fps, com som. Apresenta o app numa metamorfose contínua, sem cortes: cada forma vira a seguinte.

1. **notas.** Um cursor pisca na página e digita "toda obra começa numa [[nota]]." Quando o cursor sai do link, os colchetes recolhem como no Live Preview, e o realce azul fica atrás da palavra.
2. **livro.** A câmera recua e a frase vira o título de uma página composta, com as linhas justificadas entrando em cascata. A página dupla abre, fecha, gira para a vista isométrica e vira uma pilha de folhas.
3. **storyboard.** A folha de cima volta de frente e vira uma página de mangá. O quadro interno se desenha, e o cursor corta os quadros com a ferramenta de quadros, de fora a fora, abrindo a medianiz. Os desenhos começam em azul de lápis e passam a nanquim, e depois entram o balão e as linhas de ação.
4. **imagens.** Os três quadros viram imagens de um mosaico em movimento. A busca por descrição ("lua sobre o mar") filtra e reordena o mosaico, e o cursor escolhe o quadro da lua.
5. **ateliet.** A imagem escolhida vira a página em nanquim, e o símbolo é montado com a mesma ferramenta: o corte inclinado a 13° e o quadro redondo. Depois vem o lockup com a frase "a oficina de quem escreve e desenha".

O vídeo usa só as três cores da marca. O azul aparece em uma coisa por vez: o realce, o corte, o esboço, a seleção e, por fim, o miolo do "a". Tudo vem da geometria: o símbolo e o logotipo são lidos de `../lockup.svg`, o texto é Newsreader e a interface é Inter. Cada quadro é a média de 8 instantes (motion blur de 180°), com grão por cima. O som é sintetizado e segue os eventos do filme.

## gerar

```sh
node brand/video/render.js                        # escreve brand/video/ateliet.mp4
node brand/video/render.js --stills 2.5,7.6 dir   # quadros soltos, para conferir
```

Precisa de Node com Playwright (e o Chromium dele), ffmpeg e python3 com numpy. A Newsreader é baixada do Google Fonts para `~/.cache/ateliet-brand`, como em `build.py`. A animação está em `film.js` (um `renderFrame(f)` que desenha o quadro `f` num canvas), e o som está em `sound.py`.
