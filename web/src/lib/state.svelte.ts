import {
  api,
  type EventDetail,
  type Finding,
  type Info,
  type MarkerKind,
  type NavExtra,
  type NavOp,
  type NavResult,
  type State,
  type Status,
  type Timeline,
  type WatchType,
} from "./api";

export type Selection = { kind: "reg"; name: string } | { kind: "mem"; addr: string; size: number };
export type Bookmark = { id: string; locator: string; step: number; label: string; note: string; created: string };
export type NotesDoc = {
  version: 1;
  trace?: string;
  last?: string;
  notes?: string;
  bookmarks?: Bookmark[];
  watches?: { expr: string; type: WatchType }[];
  guide?: Record<string, { answer: string; correct: boolean }>;
};
export type Prefs = {
  version: 1;
  cols?: [number, number, number];
  bottom?: number;
  bottomCollapsed?: boolean;
  leftSplit?: number;
  stackAddrs?: boolean;
  memWidth?: 1 | 2 | 4 | 8;
  markerFilter?: Partial<Record<MarkerKind | "bookmark", boolean>>;
  helpSeen?: boolean;
  predict?: boolean;
  narrowTab?: "code" | "regs" | "stack";
};
export type BottomTab = "Memory" | "Events" | "Output" | "ABI" | "Notes" | "Guide";

/** `lab.prefs` after loading: every field except `helpSeen` has a value (a missing marker filter entry means on). */
export type ResolvedPrefs = Prefs & Required<Omit<Prefs, "helpSeen">>;

export const PREF_DEFAULTS: Omit<ResolvedPrefs, "helpSeen"> = {
  version: 1,
  cols: [0.4, 0.27, 0.33],
  bottom: 240,
  bottomCollapsed: false,
  leftSplit: 0.55,
  stackAddrs: false,
  memWidth: 1,
  markerFilter: {},
  predict: false,
  narrowTab: "code",
};

/** What the status line says when a navigation did not move. */
export const NAV_FAIL: Record<NavOp, string> = {
  "over": "No later step at this call depth.",
  "back-over": "No earlier step at this call depth.",
  "next-line": "No later source line at this call depth.",
  "prev-line": "No earlier source line at this call depth.",
  "out": "This function does not return within the recording.",
  "back-out": "Already in the outermost recorded function.",
  "reg-prev": "This register has not changed before this step.",
  "reg-next": "This register does not change again.",
  "write-prev": "No recorded write to this address before this step; it still holds its initial contents.",
  "write-next": "This address is not written again.",
  "at-next": "No later execution of this instruction.",
  "line-next": "This line does not run again after this step.",
};

/** Ops whose result is a jump somewhere else, so the place you came from goes into the history. */
const RECORDED_OPS: Partial<Record<NavOp, true>> = {
  "reg-prev": true,
  "reg-next": true,
  "write-prev": true,
  "write-next": true,
  "at-next": true,
  "line-next": true,
};

/** Keeps `raw`'s value when it has the right type and range, else the default. */
function resolvePrefs(raw: Record<string, unknown>): ResolvedPrefs {
  const d = PREF_DEFAULTS;
  const within = (v: unknown, lo: number, hi: number) => typeof v === "number" && v >= lo && v <= hi;
  const flag = (v: unknown, dflt: boolean) => (typeof v === "boolean" ? v : dflt);
  const c = raw.cols;
  return {
    ...raw,
    version: 1,
    cols: Array.isArray(c) && c.length === 3 && c.every((x) => within(x, 0.05, 20)) ? [c[0], c[1], c[2]] : [...d.cols],
    bottom: within(raw.bottom, 40, 10000) ? (raw.bottom as number) : d.bottom,
    bottomCollapsed: flag(raw.bottomCollapsed, d.bottomCollapsed),
    leftSplit: within(raw.leftSplit, 0.05, 0.95) ? (raw.leftSplit as number) : d.leftSplit,
    stackAddrs: flag(raw.stackAddrs, d.stackAddrs),
    memWidth: [1, 2, 4, 8].includes(raw.memWidth as number) ? (raw.memWidth as 1 | 2 | 4 | 8) : d.memWidth,
    markerFilter: raw.markerFilter && typeof raw.markerFilter === "object" ? (raw.markerFilter as ResolvedPrefs["markerFilter"]) : {},
    predict: flag(raw.predict, d.predict),
    narrowTab: raw.narrowTab === "regs" || raw.narrowTab === "stack" ? raw.narrowTab : "code",
    helpSeen: typeof raw.helpSeen === "boolean" ? raw.helpSeen : undefined,
  };
}

const STEP_HASH = /^#step=(\d+)$/;

class Lab {
  info = $state.raw<Info | null>(null);
  timeline = $state.raw<Timeline | null>(null);
  findings = $state.raw<Finding[]>([]);
  events = $state.raw<EventDetail[]>([]);
  /** The displayed state; `step` is its step number. */
  cur = $state.raw<State | null>(null);
  step = $state(0);
  error = $state<string | null>(null);
  notice = $state<string | null>(null);
  selection = $state<Selection | null>(null);
  /** Expression the memory panel is showing; empty follows `rsp & ~15`. */
  memAddr = $state("");
  notes = $state<NotesDoc>({ version: 1 });
  prefs = $state<ResolvedPrefs>(resolvePrefs({}));
  /** Panels that cache server data refetch when `status.generation` changes (the store has reloaded by then). */
  status = $state<Status>({ generation: 0, building: false, error: null });
  sources = $state<Record<string, string[]>>({});
  bottomTab = $state<BottomTab>("Memory");
  predictScore = $state({ right: 0, total: 0 });
  /** Position in this tab's jump history and the highest position seen: the browser entries `{step, idx}` we created. */
  histIdx = $state(0);
  histMax = $state(0);
  helpOpen = $state(false);

  /** Latest requested step. Not reactive: only `pump` and relative moves read it. */
  private want = 0;
  private busy = false;
  private pumping: Promise<void> = Promise.resolve();
  private navQ: Promise<void> = Promise.resolve();
  /** Bumped when the recording is replaced; a displayed state from an older epoch is refetched. */
  private epoch = 0;
  private shownEpoch = 0;
  private pendingSources = new Set<string>();
  /** Set when notes or prefs could not be read: saving would overwrite a file we failed to understand. */
  private unreadable = { notes: false, prefs: false };
  private saveTimers: Record<"notes" | "prefs", number | undefined> = { notes: undefined, prefs: undefined };
  private noticeTimer: number | undefined;
  private lastTimer: number | undefined;
  private urlTimer: number | undefined;
  private urlAt = 0;
  private pollFailures = 0;

  get last(): number {
    return this.info?.steps ?? 0;
  }

  get canBack(): boolean {
    return this.histIdx > 0;
  }

  get canForward(): boolean {
    return this.histIdx < this.histMax;
  }

  notify(msg: string) {
    this.notice = msg;
    clearTimeout(this.noticeTimer);
    this.noticeTimer = window.setTimeout(() => (this.notice = null), 4000);
  }

  fail(e: unknown) {
    if (e instanceof DOMException && e.name === "AbortError") return;
    this.error = e instanceof Error ? e.message : String(e);
  }

  async init() {
    try {
      const [info, tl, abi, ev, status, notes, prefs] = await Promise.all([
        api.info(),
        api.timeline(),
        api.abi(),
        api.events(),
        api.status(),
        api.notes().catch((e) => {
          this.unreadable.notes = true;
          this.fail(e);
          return {};
        }),
        api.prefs().catch((e) => {
          this.unreadable.prefs = true;
          this.fail(e);
          return {};
        }),
      ]);
      this.info = info;
      // A guide is why this recording was opened (`systemslab try`): show it first.
      if (info.has_guide) this.bottomTab = "Guide";
      this.timeline = tl;
      this.findings = abi.findings;
      this.events = ev.events;
      Object.assign(this.status, status);
      this.notes = { ...notes, version: 1 };
      this.prefs = resolvePrefs(prefs);
      if (!this.prefs.helpSeen) this.helpOpen = true;

      const past = history.state as { idx?: unknown } | null;
      this.histIdx = typeof past?.idx === "number" ? past.idx : 0;
      this.histMax = this.histIdx;
      window.addEventListener("popstate", (e) => this.onPopState(e));
      window.addEventListener("pagehide", () => this.flushPending());

      let start = 0;
      const hash = STEP_HASH.exec(location.hash);
      if (hash) {
        start = Number(hash[1]);
      } else if (this.notes.last) {
        try {
          start = (await api.locate(this.notes.last)).step;
          this.notify(`Restored your last position (${this.notes.last}).`);
        } catch {
          /* the saved locator no longer exists in this recording; start at the beginning */
        }
      }
      await this.goto(start);
      window.setTimeout(() => void this.poll(), 1000);
    } catch (e) {
      this.fail(e);
    }
  }

  /**
   * Request the state at `s`. Requests collapse into `want`; a single pump fetches until the
   * displayed step equals `want`, so rapid or held keys are never dropped. With `record`, the
   * browser history gets an entry first so Back returns to where you were. The returned promise
   * settles when the pump goes idle.
   */
  goto(s: number, opts: { record?: boolean } = {}): Promise<void> {
    const target = Number.isFinite(s) ? Math.max(0, Math.min(this.last, Math.round(s))) : this.want;
    if (opts.record && target !== this.want) {
      history.replaceState({ step: this.want, idx: this.histIdx }, "", `#step=${this.want}`);
      this.histIdx += 1;
      this.histMax = this.histIdx;
      history.pushState({ step: target, idx: this.histIdx }, "", `#step=${target}`);
    }
    this.want = target;
    if (!this.busy) {
      this.busy = true;
      this.pumping = this.pump();
    }
    return this.pumping;
  }

  /** Move relative to the latest requested step, not the displayed one. */
  stepBy(d: number) {
    void this.goto(this.want + d);
  }

  private async pump() {
    try {
      while (this.want !== this.step || this.cur === null || this.shownEpoch !== this.epoch) {
        const n = this.want;
        const epoch = this.epoch;
        let st: State;
        try {
          st = await api.state(n);
        } catch (e) {
          if (epoch !== this.epoch) continue; // the recording was replaced while this request was out
          this.fail(e);
          this.want = this.step;
          this.syncUrl();
          return;
        }
        if (epoch !== this.epoch) continue;
        this.cur = st;
        this.step = n;
        this.shownEpoch = epoch;
        this.syncUrl();
        clearTimeout(this.lastTimer);
        this.lastTimer = window.setTimeout(() => this.rememberPlace(), 1000);
      }
    } finally {
      this.busy = false;
    }
  }

  /** Mirror the displayed step into the current history entry; at most every 100 ms, since browsers throttle history calls. */
  private syncUrl() {
    clearTimeout(this.urlTimer);
    const wait = this.urlAt + 100 - Date.now();
    if (wait > 0) {
      this.urlTimer = window.setTimeout(() => this.syncUrl(), wait);
      return;
    }
    this.urlAt = Date.now();
    history.replaceState({ step: this.step, idx: this.histIdx }, "", `#step=${this.step}`);
  }

  private onPopState(e: PopStateEvent) {
    const st = e.state as { step?: unknown; idx?: unknown } | null;
    if (st && typeof st.step === "number" && typeof st.idx === "number") {
      this.histIdx = st.idx;
      this.histMax = Math.max(this.histMax, st.idx);
      void this.goto(st.step);
      return;
    }
    // The address bar was edited: the browser added an entry of its own, so claim it.
    const hash = STEP_HASH.exec(location.hash);
    const target = hash ? Math.min(this.last, Number(hash[1])) : this.want;
    this.histIdx += 1;
    this.histMax = this.histIdx;
    history.replaceState({ step: target, idx: this.histIdx }, "", `#step=${target}`);
    void this.goto(target);
  }

  back() {
    if (this.canBack) history.back();
  }

  forward() {
    if (this.canForward) history.forward();
  }

  /** Navigation ops run one at a time, so each one starts from the previous op's target. */
  nav(op: NavOp, extra: NavExtra = {}, opts: { record?: boolean } = {}): Promise<number | null> {
    const run = async (): Promise<number | null> => {
      try {
        const r = await api.nav(this.want, op, extra);
        if (r.step === null) {
          this.notify(NAV_FAIL[op]);
          return null;
        }
        void this.goto(r.step, { record: opts.record ?? RECORDED_OPS[op] === true });
        return r.step;
      } catch (e) {
        this.fail(e);
        return null;
      }
    };
    const result = this.navQ.then(run);
    this.navQ = result.then(() => undefined);
    return result.then(async (step) => {
      await this.pumping;
      return step;
    });
  }

  /** Where `op` would go from the displayed step, without moving. */
  async probe(op: NavOp, extra: NavExtra = {}): Promise<NavResult> {
    try {
      return await api.nav(this.step, op, extra);
    } catch (e) {
      this.fail(e);
      return { step: null, loc: null, asm: null };
    }
  }

  async locate(loc: string, opts: { record?: boolean } = {}): Promise<boolean> {
    try {
      const r = await api.locate(loc);
      await this.goto(r.step, { record: opts.record ?? true });
      return true;
    } catch (e) {
      this.fail(e);
      return false;
    }
  }

  select(sel: Selection | null) {
    this.selection = sel;
  }

  showInMemory(addr: string) {
    this.memAddr = addr;
    this.bottomTab = "Memory";
    if (this.prefs.bottomCollapsed) {
      this.prefs.bottomCollapsed = false;
      this.savePrefs();
    }
  }

  addBookmark() {
    const c = this.cur;
    if (!c) return;
    const id = `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
    this.notes.bookmarks = [
      ...(this.notes.bookmarks ?? []),
      { id, locator: c.locator, step: c.step, label: `${c.loc} · ${c.asm}`, note: "", created: new Date().toISOString() },
    ];
    this.saveNotes();
    this.notify(`Bookmarked step ${c.step}`);
  }

  ensureSource(file: string) {
    if (file in this.sources || this.pendingSources.has(file)) return;
    this.pendingSources.add(file);
    const epoch = this.epoch;
    api
      .source(file)
      .then(
        (s) => s.lines,
        (e) => [`(source unavailable: ${e instanceof Error ? e.message : e})`],
      )
      .then((lines) => {
        if (epoch === this.epoch) this.sources[file] = lines;
      })
      .finally(() => this.pendingSources.delete(file));
  }

  saveNotes() {
    this.scheduleSave("notes", 800);
  }

  savePrefs() {
    this.scheduleSave("prefs", 500);
  }

  private scheduleSave(kind: "notes" | "prefs", delay: number) {
    if (this.unreadable[kind]) return;
    clearTimeout(this.saveTimers[kind]);
    this.saveTimers[kind] = window.setTimeout(() => this.writeOut(kind, false), delay);
  }

  private writeOut(kind: "notes" | "prefs", keepalive: boolean) {
    clearTimeout(this.saveTimers[kind]);
    this.saveTimers[kind] = undefined;
    const doc = $state.snapshot(kind === "notes" ? this.notes : this.prefs);
    (kind === "notes" ? api.putNotes(doc, keepalive) : api.putPrefs(doc, keepalive)).catch((e) =>
      this.fail(`could not save ${kind}: ${e instanceof Error ? e.message : e}`),
    );
  }

  /** `notes.last` follows the displayed state, 1 s after the last change. */
  private rememberPlace() {
    this.lastTimer = undefined;
    const loc = this.cur?.locator;
    if (loc && this.notes.last !== loc) {
      this.notes.last = loc;
      this.saveNotes();
    }
  }

  /** The page is going away: send what is still waiting for its timer. */
  private flushPending() {
    if (this.lastTimer !== undefined) {
      clearTimeout(this.lastTimer);
      this.rememberPlace();
    }
    for (const kind of ["notes", "prefs"] as const) {
      if (this.saveTimers[kind] !== undefined) this.writeOut(kind, true);
    }
  }

  /** Polls the server once a second: build progress for the banner, and a reload when the recording was replaced. */
  private async poll() {
    try {
      const s = await api.status();
      this.pollFailures = 0;
      this.status.building = s.building;
      this.status.error = s.error;
      if (s.generation !== this.status.generation) {
        await this.reload();
        this.status.generation = s.generation;
      }
    } catch {
      if (++this.pollFailures >= 3) {
        this.fail("The viewer server stopped.");
        return;
      }
    }
    window.setTimeout(() => void this.poll(), 1000);
  }

  /** The recording was replaced: refetch everything derived from it and stay on the same moment if it still exists. */
  private async reload() {
    const loc = this.cur?.locator;
    this.epoch += 1;
    this.sources = {};
    this.pendingSources.clear();
    try {
      const [info, tl, abi, ev] = await Promise.all([api.info(), api.timeline(), api.abi(), api.events()]);
      this.info = info;
      this.timeline = tl;
      this.findings = abi.findings;
      this.events = ev.events;
    } catch (e) {
      this.fail(e);
      return;
    }
    let target = Math.min(this.step, this.last);
    if (loc) {
      try {
        target = (await api.locate(loc)).step;
        this.notify(`Recording reloaded; kept your place at ${loc}.`);
      } catch {
        this.notify(`Recording reloaded; ${loc} no longer exists, staying at step ${target}.`);
      }
    }
    await this.goto(target);
  }
}

export const lab = new Lab();
