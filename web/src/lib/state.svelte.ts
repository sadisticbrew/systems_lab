import { api, type Finding, type Info, type NavOp, type State, type Timeline } from "./api";

class Lab {
  step = $state(0);
  info = $state<Info | null>(null);
  timeline = $state<Timeline | null>(null);
  findings = $state<Finding[]>([]);
  cur = $state<State | null>(null);
  error = $state<string | null>(null);

  private ctl: AbortController | null = null;

  get last(): number {
    return this.info?.steps ?? 0;
  }

  fail(e: unknown) {
    if (e instanceof DOMException && e.name === "AbortError") return;
    this.error = e instanceof Error ? e.message : String(e);
  }

  async init() {
    try {
      const [info, tl, abi] = await Promise.all([api.info(), api.timeline(), api.abi()]);
      this.info = info;
      this.timeline = tl;
      this.findings = abi.findings;
      await this.goto(0);
    } catch (e) {
      this.fail(e);
    }
  }

  /** Fetch the state at `s`; a newer call aborts the previous request. */
  async goto(s: number) {
    const n = Math.max(0, Math.min(this.last, Math.round(s)));
    this.ctl?.abort();
    const ctl = new AbortController();
    this.ctl = ctl;
    try {
      const st = await api.state(n, ctl.signal);
      if (this.ctl !== ctl) return;
      this.cur = st;
      this.step = n;
    } catch (e) {
      this.fail(e);
    }
  }

  async nav(op: NavOp, extra: { reg?: string; addr?: string } = {}) {
    try {
      const r = await api.nav(this.step, op, extra);
      if (r.step !== null) await this.goto(r.step);
    } catch (e) {
      this.fail(e);
    }
  }

  async locate(loc: string) {
    try {
      const r = await api.locate(loc);
      await this.goto(r.step);
    } catch (e) {
      this.fail(e);
    }
  }
}

export const lab = new Lab();
