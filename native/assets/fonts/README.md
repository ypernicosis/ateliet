# Fonts bundled with the native app

- `Inter-*.ttf`: [Inter](https://rsms.me/inter/) 4.1 by Rasmus Andersson, SIL Open Font License 1.1 (`LICENSE-Inter.txt`). Subset to Latin, Latin Extended, punctuation, arrows and symbols with `pyftsubset … --layout-features='kern,liga,calt,tnum,case,ss01'`.
- `Lucide.ttf`: a subset of the [Lucide](https://lucide.dev) icon font (lucide-static 0.469.0), ISC License (`LICENSE-Lucide.txt`). The icons in use are listed in `lucide-icons.txt`; `subset.py` regenerates the font and `src/icons.rs`.
