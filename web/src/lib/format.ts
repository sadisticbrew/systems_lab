// Number, register and text helpers shared by the panels. Values from the API are hex strings.

/** A `0x…` (or decimal, optionally negative) string from the API as a bigint. */
export function big(hex: string): bigint {
  const t = hex.trim();
  return t.startsWith("-") ? -BigInt(t.slice(1)) : BigInt(t);
}

/** `v` as `0x…`, zero-padded to `bytes` bytes when given (negative values wrap to two's complement). */
export function hex(v: bigint, bytes?: number): string {
  if (bytes === undefined) return v < 0n ? `-0x${(-v).toString(16)}` : `0x${v.toString(16)}`;
  return `0x${BigInt.asUintN(bytes * 8, v).toString(16).padStart(bytes * 2, "0")}`;
}

export function unsigned(v: bigint, bits: number): bigint {
  return BigInt.asUintN(bits, v);
}

export function signed(v: bigint, bits: number): bigint {
  return BigInt.asIntN(bits, v);
}

/** The 64-bit value as 8 groups of 8 binary digits, high byte first. */
export function binaryGroups(v: bigint): string[] {
  const u = BigInt.asUintN(64, v);
  return Array.from({ length: 8 }, (_, i) => ((u >> BigInt(8 * (7 - i))) & 0xffn).toString(2).padStart(8, "0"));
}

/** The names a general-purpose register is addressed by, widest first. `shift` is the bit offset inside the 64-bit register. */
export const SUBREGS: Record<string, { name: string; bits: number; shift: number }[]> = {};
for (const l of ["a", "b", "c", "d"]) {
  SUBREGS[`r${l}x`] = [
    { name: `r${l}x`, bits: 64, shift: 0 },
    { name: `e${l}x`, bits: 32, shift: 0 },
    { name: `${l}x`, bits: 16, shift: 0 },
    { name: `${l}h`, bits: 8, shift: 8 },
    { name: `${l}l`, bits: 8, shift: 0 },
  ];
}
for (const n of ["si", "di", "bp", "sp"]) {
  SUBREGS[`r${n}`] = [
    { name: `r${n}`, bits: 64, shift: 0 },
    { name: `e${n}`, bits: 32, shift: 0 },
    { name: n, bits: 16, shift: 0 },
    { name: `${n}l`, bits: 8, shift: 0 },
  ];
}
for (let i = 8; i <= 15; i++) {
  SUBREGS[`r${i}`] = [
    { name: `r${i}`, bits: 64, shift: 0 },
    { name: `r${i}d`, bits: 32, shift: 0 },
    { name: `r${i}w`, bits: 16, shift: 0 },
    { name: `r${i}b`, bits: 8, shift: 0 },
  ];
}

export type Prediction = { ok: true; value: bigint } | { ok: true; expr: string } | { ok: false; error: string };

const NUMBER = "(?:0x[0-9a-f]+|\\d+)";
const NUM_RE = new RegExp(`^-?${NUMBER}$`, "i");
const BASE_RE = new RegExp(`^(stack0|heap)\\s*(?:([+-])\\s*(${NUMBER}))?$`, "i");

/** A learner's guess: a number, or `stack0`/`heap` ± an offset (returned as an expression for `api.eval`). */
export function parsePrediction(text: string): Prediction {
  const t = text.trim();
  if (NUM_RE.test(t)) return { ok: true, value: big(t) };
  const m = BASE_RE.exec(t);
  if (m) return { ok: true, expr: `${m[1].toLowerCase()}${m[2] ?? ""}${m[3]?.toLowerCase() ?? ""}` };
  return { ok: false, error: "Enter a number (42, -1, 0x2a) or stack0/heap ± offset" };
}

/** Whether the guess equals the recorded value `raw` (hex) as unsigned `bits`-bit numbers. */
export function predictionMatches(pred: bigint, raw: string, bits: number): boolean {
  return BigInt.asUintN(bits, pred) === BigInt.asUintN(bits, big(raw));
}

/** C-style escapes for `\n \t \r \\ "`, `\xNN` for other control bytes. */
export function escapeBytes(s: string): string {
  let out = "";
  for (const ch of s) {
    const c = ch.codePointAt(0)!;
    if (ch === "\n") out += "\\n";
    else if (ch === "\t") out += "\\t";
    else if (ch === "\r") out += "\\r";
    else if (ch === "\\") out += "\\\\";
    else if (ch === '"') out += '\\"';
    else if (c < 0x20 || c === 0x7f) out += `\\x${c.toString(16).padStart(2, "0")}`;
    else out += ch;
  }
  return out;
}

export type AsmToken = { text: string; cls: "reg" | "imm" | "sym" | "" };

const PREFIXES = "(?:(?:rep|repe|repne|repz|repnz|lock|notrack|bnd)\\s+)*";
const INSN_RE = new RegExp(`^(\\s*${PREFIXES})([a-z][a-z0-9]*)(\\s+)(.*)$`, "i");
const BRANCH_RE = /^(?:call|jmp|j[a-z]+|loop[a-z]*|xbegin)$/i;
const OPERAND_RE = /%[a-z][a-z0-9]*|\$-?(?:0x[0-9a-f]+|\d+)/gi;

/** Splits AT&T assembly into tokens for colouring: `%reg`, `$imm`, and a direct branch or call target. The texts concatenate back to `asm`. */
export function asmTokens(asm: string): AsmToken[] {
  const m = INSN_RE.exec(asm);
  if (!m) return [{ text: asm, cls: "" }];
  const [, lead, mnemonic, gap, operands] = m;
  const out: AsmToken[] = [{ text: lead + mnemonic + gap, cls: "" }];
  if (BRANCH_RE.test(mnemonic) && !/^[*%]|\(/.test(operands)) {
    out.push({ text: operands, cls: "sym" });
    return out;
  }
  let at = 0;
  for (const t of operands.matchAll(OPERAND_RE)) {
    if (t.index > at) out.push({ text: operands.slice(at, t.index), cls: "" });
    out.push({ text: t[0], cls: t[0][0] === "%" ? "reg" : "imm" });
    at = t.index + t[0].length;
  }
  if (at < operands.length) out.push({ text: operands.slice(at), cls: "" });
  return out;
}

/** `value mod 16` of a hex string, e.g. `%rsp` at a call. */
export function mod16(hexValue: string): number {
  return Number(big(hexValue) & 15n);
}
