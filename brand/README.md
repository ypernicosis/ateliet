# marca

## conceito

*Atelier* vem do francês antigo *astelier*, a oficina do carpinteiro, que por sua vez vem de *astele*, a lasca de madeira. A palavra nasceu do resto que o trabalho deixa no chão. O ateliet é a oficina de quem escreve e desenha: notas, imagens, storyboards, páginas de mangá. O símbolo foi construído com a ferramenta que o próprio app oferece para paginar.

É uma página em nanquim, o quadro interno (基本枠) de uma folha de manuscrito, com dois cortes de quadro. Um quadro redondo inserido e um corte inclinado, que atravessa a página e sai pela borda de baixo, formam juntos um "a" minúsculo de um andar. A letra não está desenhada: ela é a medianiz, o espaço entre os quadros. Nos mangás esse espaço marca a passagem do tempo (o *ma*, 間). É ali que a leitura acontece.

O quadro redondo, o miolo do "a", está em azul não-fotográfico, a cor do lápis de esboço. As câmeras de reprodução não registravam esse azul, então o desenhista esboçava nele e passava a limpo em preto por cima. Uma página com um quadro ainda em azul é um trabalho em andamento, e é isso que um ateliê guarda. No ícone grande, a linha de refile azul em volta da página é a guia que vem impressa no papel de manuscrito de mangá.

A haste é inclinada a 13°, o mesmo ângulo das hastes do itálico do logotipo, e o "a" de um andar é o "a" desse itálico, reconstruído com cortes de quadro. O símbolo é a versão desenhada da palavra; o logotipo é a versão escrita.

## elementos

| arquivo | uso |
| --- | --- |
| `icon.svg` | ícone do app (128 px para cima), com a guia de refile |
| `icon-small.svg` | ícone em tamanho óptico pequeno (16 a 64 px): página maior, medianizes mais grossas, sem guia |
| `mark.svg` | o símbolo sozinho, sobre fundos claros |
| `mark-mono.svg` | uma cor (`currentColor`), medianizes vazadas; para gravar, carimbar, favicon monocromático |
| `wordmark.svg` | o logotipo, Newsreader Italic em corpo de display (opsz 72), contornado |
| `wordmark-text.svg` | o mesmo logotipo em corpo de texto (opsz 16), para abaixo de uns 24 px de altura |
| `lockup.svg` / `lockup-dark.svg` | símbolo + logotipo; no fundo escuro a página leva a margem de papel |

`icons/icon.png` e `icons/icon.icns` são gerados a partir de `icon.svg` e `icon-small.svg`.

## regras

- Tudo em minúsculas: o nome, os títulos, as frases da marca. "ateliet", nunca "Ateliet" nem "ATELIET".
- Três cores e nenhuma outra: papel `#F4F2EC`, nanquim `#18181C`, azul não-fotográfico `#9BD3EA` (guia `#86C6E3`). O azul aparece em uma coisa por vez.
- As medianizes são sempre papel. Num fundo escuro a página leva a margem de papel (como em `lockup-dark.svg`), ou se usa `mark-mono.svg` na cor do papel. Nunca medianizes pretas sobre página branca.
- No lockup, a página tem a altura das maiúsculas do logotipo e assenta na linha de base. O espaço entre a página e a palavra é 0,42 do lado da página.
- Não esticar, girar, contornar, sombrear (exceto o ícone do macOS), nem trocar a fonte do logotipo.

## gerar

```sh
pip install fonttools uharfbuzz skia-pathops cairosvg pillow
python3 brand/build.py
```

A geometria está toda no script. As fontes são baixadas do Google Fonts na primeira vez para `~/.cache/ateliet-brand` e não vão para o repositório; o logotipo sai em contornos. Newsreader é da Production Type, licença SIL OFL 1.1.
