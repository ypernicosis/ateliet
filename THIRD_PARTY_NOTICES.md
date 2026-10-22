# Third-party notices

The shared picture library (`src/pictogrep.rs`, `src/pinterest.rs`, `src/shared.rs`, `src/plugin-sdk.js`) and the storyboard's graphite pencil and storyboards-folder format (`ui/storyboard/brush.ts`, `src/storyboard.rs`) follow the file formats, naming rules, plugin API, Pinterest reading approach and drawing code of Pictogrep (https://github.com/tiagohierath/pictogrep), reimplemented in Rust and TypeScript. Pictogrep is distributed under the MIT License:

```text
MIT License

Copyright (c) 2026 Tiago Hierath

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The Clip Studio Paint reader (`src/clipstudio.rs`) follows the `.clip` format as worked out by clip_to_psd (https://github.com/dobrokot/clip_to_psd), reimplemented in Rust. clip_to_psd is distributed under the MIT License:

```text
MIT License

Copyright (c) 2024  

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The built-in Dataview, Templater and Calendar tools (`src/dataview/`, `src/script/`, `src/calendar.rs`, `ui/tools/`) reimplement the query language, script APIs and behaviour of the Dataview (https://github.com/blacksmithgu/obsidian-dataview), Templater (https://github.com/SilentVoid13/Templater) and Calendar (https://github.com/liamcain/obsidian-calendar-plugin) plugins for Obsidian, all distributed under the MIT License, so that notes written for them keep working. No code from those projects is included.

The script engine embeds QuickJS (https://bellard.org/quickjs/, MIT License, Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon) through the rquickjs crate (MIT License).

## Portuguese hyphenation patterns

`core/assets/hyph-pt.txt` holds the patterns of `hyph-pt.tex` from the hyph-utf8 package (Copyright (C) 1987, 1994, 1996, 2015 Pedro J. de Rezende, 1996, 2015 J. Joao Dias Almeida, 2024 Leonardo Araujo and Aline Benevides), under the BSD 3-clause licence reproduced at the top of that file.
