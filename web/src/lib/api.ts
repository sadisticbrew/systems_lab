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
  trace_path: string;
  notes_path: string;
  mode: "view" | "run";
  source_path: string | null;
  has_guide: boolean;
};
export type Status = { generation: number; building: boolean; error: string | null };
export type MarkerKind = "call" | "ret" | "syscall" | "signal" | "abi";
export type Marker = { step: number; kind: MarkerKind; label: string };
export type Timeline = { steps: number; from: number; to: number; depth: number[]; markers: Marker[] };
export type Reg = { name: string; value: string; rendered: string; changed: boolean; prev: string | null };
export type Xmm = { name: string; value: string; changed: boolean };
export type Write = { addr: string; raw_addr: string; size: number; value: string; kind: string };
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
export type Frame = {
  label: string;
  func: string;
  collapsed: boolean;
  size: number | null;
  slots: Slot[];
  call_step: number | null;
  ret_step: number | null;
};
export type StackView = { step: number; loc: string; asm: string; frames: Frame[] };
export type StepKind = "instr" | "fault" | "deliver" | "exit" | "end";
export type SyscallArg = { reg: string; name: string; value: string; raw: string; note: string | null };
export type Buffer = { arg: string; dir: "in" | "out"; addr: string; len: number; text: string | null; truncated: boolean };
export type EventDetail =
  | {
      kind: "syscall";
      step: number;
      nr: number;
      name: string;
      args: SyscallArg[];
      ret: string | null;
      ret_raw: string | null;
      errno: string | null;
      errno_text: string | null;
      buffers: Buffer[];
    }
  | {
      kind: "signal";
      step: number;
      signal: string;
      code: number;
      code_name: string | null;
      code_text: string | null;
      addr: string;
      cpu_exception: string | null;
    };
export type Effects = {
  mnemonic: string;
  kind: "instr" | "fault" | "deliver" | "exit";
  next_loc: string;
  reg_reads: { name: string; value: string; rendered: string }[];
  reg_writes: {
    name: string;
    via: string | null;
    before: string;
    after: string;
    before_rendered: string;
    after_rendered: string;
    changed: boolean;
  }[];
  mem_reads: { addr: string; raw_addr: string; size: number; value: string }[];
  mem_writes: {
    addr: string;
    raw_addr: string;
    size: number;
    kind: "instr" | "kernel" | "snapshot" | "unattributed";
    before: string;
    after: string;
  }[];
  flags_read: { name: string; value: number }[];
  flags_written: { name: string; before: number; after: number; undefined: boolean }[];
  condition: { code: string; text: string; meaning: string; holds: boolean } | null;
  control:
    | { kind: "branch"; conditional: boolean; taken: boolean; target: string; fallthrough: string }
    | { kind: "call"; target: string; return_address: string; slot: string; rsp_before: string; rsp_after: string; rsp_mod16: number }
    | { kind: "ret"; to: string; slot: string; rsp_before: string; rsp_after: string; call_step: number | null }
    | null;
  event: EventDetail | null;
  delivered: { signal: string; handler: string | null } | null;
};
export type State = {
  step: number;
  kind: StepKind;
  loc: string;
  asm: string;
  bytes: string;
  locator: string;
  source: { file: string; line: number } | null;
  regs: Reg[];
  flags: Record<string, number>;
  xmm: Xmm[];
  /** Writes of the previous instruction (step - 1): what changed to produce this state. Empty at step 0. */
  prev_writes: Write[];
  /** What the instruction at this step is about to do; null at the final state. */
  effects: Effects | null;
  depth: number;
  stack: StackView;
};
export type Mem = { addr: string; bytes: (number | null)[]; writers: (number | null)[] };
export type DisasmLine = {
  addr: string;
  loc: string;
  bytes: string;
  asm: string;
  source_line: number | null;
  source_file: string | null;
  hits: number;
};
export type Disasm = { func: string; lines: DisasmLine[] };
export type Source = { file: string; lines: string[] };
export type Finding = { rule: string; step: number; loc: string; message: string };
export type WatchType =
  | "value" | "u8" | "i8" | "u16" | "i16" | "u32" | "i32" | "u64" | "i64" | "f32" | "f64" | "ptr" | "str" | "bytes";
export type EvalResult = { value: string; rendered: string; default_type: WatchType };
export type Inspect = {
  addr: string;
  rendered: string;
  region: string;
  bytes: (number | null)[];
  /** u8 i8 u16 i16 u32 i32 u64 i64 f32 f64 ptr, in that order. */
  values: { type: WatchType; value: string | null }[];
  cstring: string | null;
  writer: number | null;
  writer_asm: string | null;
  next_writer: number | null;
};
export type Watch = {
  expr: string;
  type: WatchType;
  addr: string | null;
  value: string | null;
  raw: string | null;
  prev: string | null;
  changed: boolean;
  error: string | null;
};
export type Guide = { path: string; markdown: string };
export type NavOp =
  | "over" | "back-over" | "out" | "back-out" | "next-line" | "prev-line"
  | "reg-prev" | "reg-next" | "write-prev" | "write-next" | "at-next" | "line-next";
export type NavResult = { step: number | null; loc: string | null; asm: string | null };
export type NavExtra = { reg?: string; addr?: string; file?: string; line?: number };

async function errorOf(r: Response): Promise<Error> {
  let msg = `${r.status} ${r.statusText}`;
  try {
    const j = await r.json();
    if (j && typeof j.error === "string") msg = j.error;
  } catch {
    /* keep status text */
  }
  return new Error(msg);
}

async function get<T>(url: string, signal?: AbortSignal): Promise<T> {
  const r = await fetch(url, { signal });
  if (!r.ok) throw await errorOf(r);
  return (await r.json()) as T;
}

/** PUT a JSON body; the custom header is what the server requires for writes. `keepalive` lets it finish while the page unloads. */
async function put(url: string, body: unknown, keepalive = false): Promise<void> {
  const r = await fetch(url, {
    method: "PUT",
    headers: { "Content-Type": "application/json", "X-SystemsLab": "1" },
    body: JSON.stringify(body),
    keepalive,
  });
  if (!r.ok) throw await errorOf(r);
}

const q = (o: Record<string, string | number | undefined>) =>
  new URLSearchParams(
    Object.entries(o).filter(([, v]) => v !== undefined).map(([k, v]) => [k, String(v)]),
  ).toString();

export const api = {
  info: () => get<Info>("/api/info"),
  status: () => get<Status>("/api/status"),
  timeline: (from?: number, to?: number) => get<Timeline>(`/api/timeline?${q({ from, to })}`),
  state: (s: number, signal?: AbortSignal) => get<State>(`/api/state/${s}`, signal),
  mem: (addr: string, len: number, step: number, signal?: AbortSignal) =>
    get<Mem>(`/api/mem?${q({ addr, len, step })}`, signal),
  disasm: (addr: string, step: number) => get<Disasm>(`/api/disasm?${q({ addr, step })}`),
  source: (file: string) => get<Source>(`/api/source?${q({ file })}`),
  nav: (from: number, op: NavOp, extra: NavExtra = {}) => get<NavResult>(`/api/nav?${q({ from, op, ...extra })}`),
  locate: (loc: string) => get<{ step: number }>(`/api/locate?${q({ loc })}`),
  abi: () => get<{ findings: Finding[] }>("/api/abi"),
  events: () => get<{ events: EventDetail[] }>("/api/events"),
  eval: (expr: string, step: number) => get<EvalResult>(`/api/eval?${q({ expr, step })}`),
  inspect: (addr: string, step: number, signal?: AbortSignal) => get<Inspect>(`/api/inspect?${q({ addr, step })}`, signal),
  /** One `w=EXPR` (or `w=EXPR|TYPE`) parameter per entry. */
  watch: (step: number, watches: { expr: string; type?: WatchType }[], signal?: AbortSignal) => {
    const p = new URLSearchParams({ step: String(step) });
    for (const w of watches) p.append("w", w.type ? `${w.expr}|${w.type}` : w.expr);
    return get<{ watches: Watch[] }>(`/api/watch?${p}`, signal);
  },
  guide: () => get<Guide>("/api/guide"),
  notes: () => get<Record<string, unknown>>("/api/notes"),
  prefs: () => get<Record<string, unknown>>("/api/prefs"),
  putNotes: (doc: object, keepalive = false) => put("/api/notes", doc, keepalive),
  putPrefs: (doc: object, keepalive = false) => put("/api/prefs", doc, keepalive),
};
