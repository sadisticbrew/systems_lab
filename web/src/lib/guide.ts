// Parses a guide (Markdown plus the `@go` and `@predict` line directives) into blocks the Guide tab renders.
// The directive rules mirror `src/guide.rs`, so a line is a directive here exactly when `check-guide` sees one.

import type { WatchType } from "./api";

/** A run of text; `code` and `bold` come from `` `code` `` and `**bold**`. */
export type Span = { text: string; code?: boolean; bold?: boolean };

export type Predict = {
  kind: "predict";
  line: number;
  expr: string;
  /** From `as TYPE`; `null` means the type the server picks for the expression. */
  type: WatchType | null;
  loc: string;
  after: boolean;
  /** Key of the stored answer in `notes.guide`. */
  key: string;
  /** One entry per paragraph of the `> ` lines that follow the directive. */
  explanation: Span[][];
};

export type Block =
  | { kind: "heading"; level: 1 | 2 | 3; spans: Span[] }
  | { kind: "para"; spans: Span[] }
  | { kind: "list"; items: Span[][] }
  | { kind: "code"; text: string }
  | { kind: "go"; line: number; loc: string }
  | Predict
  | { kind: "bad"; line: number; text: string; error: string };

const WATCH_TYPES: WatchType[] = [
  "value", "u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64", "f32", "f64", "ptr", "str", "bytes",
];

/** Splits `text` at `` `code` `` and `**bold**`; an unmatched marker stays literal text. */
export function parseInline(text: string): Span[] {
  const spans: Span[] = [];
  let plain = "";
  let bold = false;
  const flush = () => {
    if (plain) spans.push(bold ? { text: plain, bold } : { text: plain });
    plain = "";
  };
  let i = 0;
  while (i < text.length) {
    if (text[i] === "`") {
      const end = text.indexOf("`", i + 1);
      if (end > i + 1) {
        flush();
        spans.push({ text: text.slice(i + 1, end), code: true, ...(bold && { bold }) });
        i = end + 1;
        continue;
      }
    }
    if (text.startsWith("**", i) && (bold || text.indexOf("**", i + 2) > i + 2)) {
      flush();
      bold = !bold;
      i += 2;
      continue;
    }
    plain += text[i++];
  }
  flush();
  return spans;
}

/** `rest` is the line after `@predict`, starting with its space: ` EXPR [as TYPE] at LOCATOR [after]`. */
function parsePredict(rest: string, line: number): Predict | string {
  const at = rest.lastIndexOf(" at ");
  if (at < 0) return "@predict needs `at LOCATOR`";
  const left = rest.slice(0, at);
  const words = rest.slice(at + 4).split(/\s+/).filter(Boolean);
  if (words.length === 0) return "@predict needs a locator after `at`";
  if (words.length > 2 || (words.length === 2 && words[1] !== "after")) {
    return `only \`after\` may follow the locator, found '${words.slice(1).join(" ")}'`;
  }
  let expr = left.trim();
  let type: WatchType | null = null;
  const as = left.lastIndexOf(" as ");
  if (as >= 0 && !/\s/.test(left.slice(as + 4))) {
    const name = left.slice(as + 4);
    type = WATCH_TYPES.find((t) => t === name.toLowerCase()) ?? null;
    if (type === null) return `unknown type '${name}'`;
    expr = left.slice(0, as).trim();
  }
  if (expr === "") return "@predict needs an expression";
  const after = words[1] === "after";
  return { kind: "predict", line, expr, type, loc: words[0], after, key: `${expr}|${words[0]}|${after ? "after" : ""}`, explanation: [] };
}

/** The line after `word` when `word` is its first word. */
function keyword(line: string, word: string): string | null {
  return line.startsWith(word) && (line.length === word.length || line[word.length] === " ") ? line.slice(word.length) : null;
}

export function parseGuide(markdown: string): Block[] {
  const blocks: Block[] = [];
  let para: string[] = [];
  let items: string[] = [];
  let fence: string[] | null = null;
  // The directive still collecting its `> ` lines, and those lines so far.
  let owner: Predict | null = null;
  let quoted: string[] = [];

  const endExplanation = () => {
    if (!owner) return;
    const paragraphs: string[][] = [[]];
    for (const q of quoted) {
      if (q === "") paragraphs.push([]);
      else paragraphs[paragraphs.length - 1].push(q);
    }
    owner.explanation = paragraphs.filter((p) => p.length).map((p) => parseInline(p.join(" ")));
    owner = null;
    quoted = [];
  };
  const flushText = () => {
    if (para.length) blocks.push({ kind: "para", spans: parseInline(para.join(" ")) });
    if (items.length) blocks.push({ kind: "list", items: items.map(parseInline) });
    para = [];
    items = [];
  };

  for (const [i, raw] of markdown.split("\n").entries()) {
    const line = raw.trimEnd();
    if (owner) {
      if (line === ">" || line.startsWith("> ")) {
        quoted.push(line.slice(2));
        continue;
      }
      if (line === "") continue;
      endExplanation();
    }
    if (line.startsWith("```")) {
      if (fence) {
        blocks.push({ kind: "code", text: fence.join("\n") });
        fence = null;
      } else {
        flushText();
        fence = [];
      }
      continue;
    }
    if (fence) {
      fence.push(line);
      continue;
    }
    const go = keyword(line, "@go");
    const predict = keyword(line, "@predict");
    if (go !== null || predict !== null) {
      flushText();
      if (go !== null) {
        const loc = go.trim().split(/\s+/)[0];
        blocks.push(loc ? { kind: "go", line: i + 1, loc } : { kind: "bad", line: i + 1, text: line, error: "@go needs a locator" });
        continue;
      }
      const p = parsePredict(predict!, i + 1);
      if (typeof p === "string") {
        blocks.push({ kind: "bad", line: i + 1, text: line, error: p });
      } else {
        blocks.push(p);
        owner = p;
      }
      continue;
    }
    const heading = /^(#{1,3}) +(.+)$/.exec(line);
    if (heading) {
      flushText();
      blocks.push({ kind: "heading", level: heading[1].length as 1 | 2 | 3, spans: parseInline(heading[2].trim()) });
    } else if (line.startsWith("- ")) {
      if (para.length) flushText();
      items.push(line.slice(2).trim());
    } else if (line === "") {
      flushText();
    } else if (items.length && /^\s/.test(line)) {
      items[items.length - 1] += ` ${line.trim()}`;
    } else {
      if (items.length) flushText();
      para.push(line.trim());
    }
  }
  endExplanation();
  flushText();
  if (fence) blocks.push({ kind: "code", text: fence.join("\n") });
  return blocks;
}

/** What the learner has typed into a prediction field and not yet checked; survives switching tabs. */
export const drafts = new Map<string, string>();
