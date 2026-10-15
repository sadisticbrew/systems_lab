// Types mirror the SystemsLab HTTP API contract.

export type EndInfo = { reason: "exit" | "signal" | "max-steps"; code?: number; signal?: string };
export type Info = {
  program: string;
  argv: string[];
  from: string;
  steps: number;
  end: EndInfo;
  aslr_disabled: boolean;
  unattributed_bytes: number;
  has_dwarf: boolean;
  functions: { name: string; loc: string }[];
};
export type MarkerKind = "call" | "ret" | "syscall" | "signal" | "abi";
export type Marker = { step: number; kind: MarkerKind; label: string };
export type Timeline = { steps: number; depth: number[]; markers: Marker[] };
export type Reg = { name: string; value: string; rendered: string; changed: boolean };
export type Xmm = { name: string; value: string; changed: boolean };
export type Write = { addr: string; raw_addr: string; size: number; value: string; kind: string };
export type SyscallEvent = { kind: "syscall"; nr: number; name: string; args: string[]; ret: string | null };
export type SignalEvent = { kind: "signal"; signal: string; code: number; addr: string };
export type TraceEvent = SyscallEvent | SignalEvent;
export type Slot = {
  addr: string;
  raw_addr: string;
  rbp_off: string | null;
  label: string;
  value: string;
  raw_value: string;
  pointers: string[];
  writer: number | null;
};
export type Frame = { label: string; func: string; collapsed: boolean; size: number | null; slots: Slot[] };
export type StackView = { step: number; loc: string; asm: string; frames: Frame[] };
export type StepKind = "instr" | "fault" | "deliver" | "exit" | "end";
export type State = {
  step: number;
  kind: StepKind;
  loc: string;
  asm: string;
  bytes: string;
  source: { file: string; line: number } | null;
  regs: Reg[];
  flags: Record<string, number>;
  xmm: Xmm[];
  writes: Write[];
  event: TraceEvent | null;
  depth: number;
  stack: StackView;
};
export type Mem = { addr: string; bytes: (number | null)[]; writers: (number | null)[] };
export type DisasmLine = { addr: string; loc: string; bytes: string; asm: string; source_line: number | null };
export type Disasm = { func: string; lines: DisasmLine[] };
export type Source = { file: string; lines: string[] };
export type Finding = { rule: string; step: number; loc: string; message: string };
export type NavOp =
  | "over" | "back-over" | "out" | "back-out" | "next-line" | "prev-line"
  | "reg-prev" | "reg-next" | "write-prev" | "at-next";

async function get<T>(url: string, signal?: AbortSignal): Promise<T> {
  const r = await fetch(url, { signal });
  if (!r.ok) {
    let msg = `${r.status} ${r.statusText}`;
    try {
      const j = await r.json();
      if (j && typeof j.error === "string") msg = j.error;
    } catch {
      /* keep status text */
    }
    throw new Error(msg);
  }
  return (await r.json()) as T;
}

const q = (o: Record<string, string | number | undefined>) =>
  new URLSearchParams(
    Object.entries(o).filter(([, v]) => v !== undefined).map(([k, v]) => [k, String(v)]),
  ).toString();

export const api = {
  info: () => get<Info>("/api/info"),
  timeline: () => get<Timeline>("/api/timeline"),
  state: (s: number, signal?: AbortSignal) => get<State>(`/api/state/${s}`, signal),
  mem: (addr: string, len: number, step: number, signal?: AbortSignal) =>
    get<Mem>(`/api/mem?${q({ addr, len, step })}`, signal),
  disasm: (addr: string, step: number) => get<Disasm>(`/api/disasm?${q({ addr, step })}`),
  source: (file: string) => get<Source>(`/api/source?${q({ file })}`),
  nav: (from: number, op: NavOp, extra: { reg?: string; addr?: string } = {}) =>
    get<{ step: number | null }>(`/api/nav?${q({ from, op, ...extra })}`),
  locate: (loc: string) => get<{ step: number }>(`/api/locate?${q({ loc })}`),
  abi: () => get<{ findings: Finding[] }>("/api/abi"),
};
