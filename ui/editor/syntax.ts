// Obsidian-flavoured markdown for CodeMirror 6: frontmatter, wikilinks,
// tags, highlights, comments and math on top of GFM; and a highlighter that
// emits Obsidian's token class names so themes style the editor correctly.

import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { languages } from "@codemirror/language-data";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { Tag, tags as t } from "@lezer/highlight";
import type { MarkdownConfig } from "@lezer/markdown";
import { NodeProp } from "@lezer/common";

/**
 * Obsidian's fork of @codemirror/language exports `tokenClassNodeProp`, and its
 * markdown parser tags every node with HyperMD token classes. Plugins
 * (Dataview inline queries, code-styler, media-extended …) read it.
 */
export const tokenClassNodeProp = new NodeProp<string>();

export const obsidianTags = {
  internalLink: Tag.define(),
  internalLinkMark: Tag.define(),
  hashtag: Tag.define(),
  highlight: Tag.define(),
  highlightMark: Tag.define(),
  comment: Tag.define(),
  math: Tag.define(),
  frontmatter: Tag.define(),
};

const Frontmatter: MarkdownConfig = {
  defineNodes: [{ name: "Frontmatter", block: true }, { name: "FrontmatterMark" }],
  parseBlock: [
    {
      name: "Frontmatter",
      before: "HorizontalRule",
      parse(cx, line) {
        if (cx.lineStart !== 0 || !/^---\s*$/.test(line.text)) return false;
        const start = cx.lineStart;
        const marks = [cx.elt("FrontmatterMark", start, start + 3)];
        let end = -1;
        while (cx.nextLine()) {
          if (/^(---|\.\.\.)\s*$/.test(line.text)) {
            marks.push(cx.elt("FrontmatterMark", cx.lineStart, cx.lineStart + 3));
            end = cx.lineStart + line.text.length;
            cx.nextLine();
            break;
          }
        }
        if (end < 0) end = cx.lineStart;
        cx.addElement(cx.elt("Frontmatter", start, end, marks));
        return true;
      },
    },
  ],
  props: [],
};

const WikiLink: MarkdownConfig = {
  defineNodes: ["WikiLink", "WikiLinkMark", "WikiLinkStart", "WikiLinkEnd", "WikiLinkPipe", "WikiLinkTarget", "WikiLinkAlias", "Embed"],
  parseInline: [
    {
      name: "WikiLink",
      before: "Link",
      parse(cx, next, pos) {
        const embed = next === 33 /* ! */ && cx.char(pos + 1) === 91 && cx.char(pos + 2) === 91;
        if (!embed && !(next === 91 && cx.char(pos + 1) === 91)) return -1;
        const open = embed ? pos + 3 : pos + 2;
        let i = open;
        for (; i < cx.end - 1; i++) {
          const c = cx.char(i);
          if (c === 10 || c === 91) return -1;
          if (c === 93 && cx.char(i + 1) === 93) break;
        }
        if (i >= cx.end - 1 || i === open) return -1;
        const close = i;
        const text = cx.slice(open, close);
        const pipe = text.indexOf("|");
        const children = [cx.elt("WikiLinkStart", pos, open)];
        if (pipe === -1) children.push(cx.elt("WikiLinkTarget", open, close));
        else {
          children.push(cx.elt("WikiLinkTarget", open, open + pipe));
          children.push(cx.elt("WikiLinkPipe", open + pipe, open + pipe + 1));
          children.push(cx.elt("WikiLinkAlias", open + pipe + 1, close));
        }
        children.push(cx.elt("WikiLinkEnd", close, close + 2));
        return cx.addElement(cx.elt(embed ? "Embed" : "WikiLink", pos, close + 2, children));
      },
    },
  ],
};

const TAG_CHAR = /[^\s#!-,.:-@\[-^`{-~]/u;
const Hashtag: MarkdownConfig = {
  defineNodes: ["Hashtag", "HashtagMark"],
  parseInline: [
    {
      name: "Hashtag",
      parse(cx, next, pos) {
        if (next !== 35 /* # */) return -1;
        const before = pos > cx.offset ? cx.slice(pos - 1, pos) : " ";
        if (!/[\s(\[{]/.test(before) && pos !== cx.offset) return -1;
        let end = pos + 1;
        while (end < cx.end) {
          const ch = cx.slice(end, end + 1);
          if (!TAG_CHAR.test(ch)) break;
          end++;
        }
        if (end === pos + 1) return -1;
        if (/^\d+$/.test(cx.slice(pos + 1, end))) return -1;
        return cx.addElement(cx.elt("Hashtag", pos, end, [cx.elt("HashtagMark", pos, pos + 1)]));
      },
    },
  ],
};

const Highlight: MarkdownConfig = {
  defineNodes: ["Highlight", "HighlightMark"],
  parseInline: [
    {
      name: "Highlight",
      parse(cx, next, pos) {
        if (next !== 61 || cx.char(pos + 1) !== 61 || cx.char(pos + 2) === 61) return -1;
        for (let i = pos + 2; i < cx.end - 1; i++) {
          if (cx.char(i) === 10) return -1;
          if (cx.char(i) === 61 && cx.char(i + 1) === 61 && i > pos + 2) {
            return cx.addElement(cx.elt("Highlight", pos, i + 2, [cx.elt("HighlightMark", pos, pos + 2), cx.elt("HighlightMark", i, i + 2)]));
          }
        }
        return -1;
      },
    },
  ],
};

const Comment: MarkdownConfig = {
  defineNodes: ["ObsidianComment"],
  parseInline: [
    {
      name: "ObsidianComment",
      before: "Emphasis",
      parse(cx, next, pos) {
        if (next !== 37 || cx.char(pos + 1) !== 37) return -1;
        for (let i = pos + 2; i < cx.end - 1; i++) {
          if (cx.char(i) === 37 && cx.char(i + 1) === 37) return cx.addElement(cx.elt("ObsidianComment", pos, i + 2));
        }
        return cx.addElement(cx.elt("ObsidianComment", pos, cx.end));
      },
    },
  ],
};

/** Inline code as flat siblings (mark, text, mark), like Obsidian's parser. */
const FlatInlineCode: MarkdownConfig = {
  defineNodes: ["InlineCodeMark", "InlineCodeText"],
  parseInline: [
    {
      name: "FlatInlineCode",
      before: "InlineCode",
      parse(cx, next, pos) {
        if (next !== 96 /* ` */) return -1;
        if (pos > cx.offset && cx.char(pos - 1) === 96) return -1;
        let n = 0;
        while (cx.char(pos + n) === 96) n++;
        for (let i = pos + n; i < cx.end; i++) {
          if (cx.char(i) !== 96) continue;
          let m = 0;
          while (cx.char(i + m) === 96) m++;
          if (m === n) {
            cx.addElement(cx.elt("InlineCodeMark", pos, pos + n));
            if (i > pos + n) cx.addElement(cx.elt("InlineCodeText", pos + n, i));
            return cx.addElement(cx.elt("InlineCodeMark", i, i + n));
          }
          i += m - 1;
        }
        return -1;
      },
    },
  ],
};

const InlineMath: MarkdownConfig = {
  defineNodes: ["InlineMath"],
  parseInline: [
    {
      name: "InlineMath",
      parse(cx, next, pos) {
        if (next !== 36 /* $ */) return -1;
        const dbl = cx.char(pos + 1) === 36;
        const open = dbl ? 2 : 1;
        if (!dbl && (cx.char(pos + 1) === 32 || cx.char(pos + 1) === -1)) return -1;
        for (let i = pos + open; i < cx.end; i++) {
          if (cx.char(i) === 92) { i++; continue; }
          if (cx.char(i) === 36 && (!dbl || cx.char(i + 1) === 36)) {
            if (!dbl && cx.char(i - 1) === 32) return -1;
            if (i === pos + open) return -1;
            return cx.addElement(cx.elt("InlineMath", pos, i + open));
          }
        }
        return -1;
      },
    },
  ],
};

const styling: MarkdownConfig = {
  props: [],
};

import { styleTags } from "@lezer/highlight";
styling.props = [
  styleTags({
    Frontmatter: obsidianTags.frontmatter,
    FrontmatterMark: t.processingInstruction,
    "WikiLink Embed": obsidianTags.internalLink,
    "WikiLinkMark WikiLinkStart WikiLinkEnd WikiLinkPipe": obsidianTags.internalLinkMark,
    Hashtag: obsidianTags.hashtag,
    Highlight: obsidianTags.highlight,
    HighlightMark: obsidianTags.highlightMark,
    ObsidianComment: obsidianTags.comment,
    InlineMath: obsidianTags.math,
    InlineCodeText: t.monospace,
    InlineCodeMark: t.processingInstruction,
  }),
  tokenClassNodeProp.add({
    InlineCodeMark: "formatting formatting-code inline-code",
    InlineCodeText: "inline-code",
    InlineCode: "inline-code",
    CodeMark: "formatting formatting-code-block",
    ATXHeading1: "header header-1",
    ATXHeading2: "header header-2",
    ATXHeading3: "header header-3",
    ATXHeading4: "header header-4",
    ATXHeading5: "header header-5",
    ATXHeading6: "header header-6",
    HeaderMark: "formatting formatting-header header",
    Emphasis: "em",
    StrongEmphasis: "strong",
    EmphasisMark: "formatting formatting-em",
    Strikethrough: "strikethrough",
    StrikethroughMark: "formatting formatting-strikethrough strikethrough",
    Highlight: "highlight",
    HighlightMark: "formatting formatting-highlight highlight",
    WikiLink: "hmd-internal-link",
    Embed: "hmd-internal-link hmd-embed",
    WikiLinkTarget: "hmd-internal-link",
    WikiLinkAlias: "hmd-internal-link link-alias",
    WikiLinkStart: "formatting-link formatting-link-start",
    WikiLinkEnd: "formatting-link formatting-link-end",
    WikiLinkPipe: "hmd-internal-link link-alias-pipe",
    Link: "link",
    LinkMark: "formatting formatting-link link",
    URL: "string url",
    Hashtag: "hashtag hashtag-end meta",
    HashtagMark: "formatting formatting-hashtag hashtag hashtag-begin meta",
    ObsidianComment: "comment",
    InlineMath: "math",
    Blockquote: "quote quote-1",
    QuoteMark: "formatting formatting-quote formatting-quote-1 quote quote-1",
    ListMark: "formatting formatting-list formatting-list-ul list-1",
    TaskMarker: "formatting formatting-task",
    Frontmatter: "hmd-frontmatter",
    FrontmatterMark: "def hmd-frontmatter",
    HorizontalRule: "hr",
    FencedCode: "hmd-codeblock",
    CodeText: "hmd-codeblock",
    CodeInfo: "formatting formatting-code-block hmd-codeblock",
  }),
];

let mdSupport: ReturnType<typeof markdown> | null = null;
/** One shared LanguageSupport, so every editor uses the same Language instance. */
export function obsidianMarkdown() {
  return (mdSupport ||= markdown({
    base: markdownLanguage,
    codeLanguages: languages,
    addKeymap: true,
    extensions: [Frontmatter, WikiLink, Hashtag, Highlight, Comment, FlatInlineCode, InlineMath, styling],
  }));
}

/** Map syntax tags to the class names Obsidian themes target. */
export const obsidianHighlight = HighlightStyle.define([
  { tag: t.heading1, class: "cm-header cm-header-1" },
  { tag: t.heading2, class: "cm-header cm-header-2" },
  { tag: t.heading3, class: "cm-header cm-header-3" },
  { tag: t.heading4, class: "cm-header cm-header-4" },
  { tag: t.heading5, class: "cm-header cm-header-5" },
  { tag: t.heading6, class: "cm-header cm-header-6" },
  { tag: t.strong, class: "cm-strong" },
  { tag: t.emphasis, class: "cm-em" },
  { tag: t.strikethrough, class: "cm-strikethrough" },
  { tag: t.link, class: "cm-link" },
  { tag: t.url, class: "cm-url" },
  { tag: t.monospace, class: "cm-inline-code" },
  { tag: t.quote, class: "cm-quote" },
  { tag: t.processingInstruction, class: "cm-formatting" },
  { tag: t.contentSeparator, class: "cm-hr" },
  { tag: t.list, class: "cm-list" },
  { tag: obsidianTags.internalLink, class: "cm-hmd-internal-link" },
  { tag: obsidianTags.internalLinkMark, class: "cm-formatting cm-formatting-link" },
  { tag: obsidianTags.hashtag, class: "cm-hashtag" },
  { tag: obsidianTags.highlight, class: "cm-highlight" },
  { tag: obsidianTags.highlightMark, class: "cm-formatting cm-formatting-highlight" },
  { tag: obsidianTags.comment, class: "cm-comment" },
  { tag: obsidianTags.math, class: "cm-math" },
  { tag: obsidianTags.frontmatter, class: "cm-hmd-frontmatter" },
  // code inside fenced blocks
  { tag: t.keyword, class: "cm-keyword" },
  { tag: [t.atom, t.bool, t.special(t.variableName)], class: "cm-atom" },
  { tag: [t.number], class: "cm-number" },
  { tag: [t.definition(t.variableName), t.function(t.variableName)], class: "cm-def" },
  { tag: t.variableName, class: "cm-variable" },
  { tag: [t.typeName, t.className, t.namespace], class: "cm-type" },
  { tag: [t.propertyName], class: "cm-property" },
  { tag: [t.operator, t.operatorKeyword], class: "cm-operator" },
  { tag: [t.comment, t.lineComment, t.blockComment], class: "cm-comment" },
  { tag: [t.string, t.special(t.string), t.regexp], class: "cm-string" },
  { tag: [t.meta, t.annotation], class: "cm-meta" },
  { tag: [t.tagName], class: "cm-tag" },
  { tag: [t.attributeName], class: "cm-attribute" },
  { tag: [t.bracket, t.punctuation], class: "cm-bracket" },
  { tag: t.invalid, class: "cm-error" },
]);

export const obsidianSyntaxHighlighting = syntaxHighlighting(obsidianHighlight);

/** Prism-style token classes for code blocks in reading view. */
export const readingHighlight = HighlightStyle.define([
  { tag: t.keyword, class: "token keyword" },
  { tag: [t.atom, t.bool], class: "token boolean" },
  { tag: t.number, class: "token number" },
  { tag: [t.definition(t.variableName), t.function(t.variableName), t.function(t.propertyName)], class: "token function" },
  { tag: [t.typeName, t.className, t.namespace], class: "token class-name" },
  { tag: t.propertyName, class: "token property" },
  { tag: [t.operator, t.operatorKeyword], class: "token operator" },
  { tag: [t.comment, t.lineComment, t.blockComment], class: "token comment" },
  { tag: [t.string, t.special(t.string)], class: "token string" },
  { tag: t.regexp, class: "token regex" },
  { tag: [t.meta, t.annotation], class: "token important" },
  { tag: t.tagName, class: "token tag" },
  { tag: t.attributeName, class: "token attr-name" },
  { tag: [t.bracket, t.punctuation, t.separator], class: "token punctuation" },
  { tag: t.variableName, class: "token variable" },
]);
