import { api, type Finding, type Info, type NavOp, type State, type Timeline } from "./api";

class Lab {
  step = $state(0);
  info = $state<Info | null>(null);
  timeline = $state<Timeline | null>(null);
  findings = $state<Finding[]>([]);
  cur = $state<State | null>(null);
  error = $state<string | null>(null);

  /** Latest requested step. Not reactive: only `pump` and relative moves read it. */
  private want = 0;
  private busy = false;
  private pumping: Promise<void> = Promise.resolve();
  private navQ: Promise<void> = Promise.resolve();

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

  /**
   * Request the state at `s`. Requests collapse into `want`; a single pump fetches until the
   * displayed step equals `want`, so rapid or held keys are never dropped. The returned promise
   * settles when the pump goes idle.
   */
  goto(s: number): Promise<void> {
    if (Number.isFinite(s)) this.want = Math.max(0, Math.min(this.last, Math.round(s)));
    if (!this.busy) {
      this.busy = true;
      this.pumping = this.pump();
    }
    return this.pumping;
  }

  /** Move relative to the latest requested step, not the displayed one. */
  stepBy(d: number) {
    this.goto(this.want + d);
  }

  private async pump() {
    try {
      while (this.want !== this.step || this.cur === null) {
        const n = this.want;
        try {
          const st = await api.state(n);
          this.cur = st;
          this.step = n;
        } catch (e) {
          this.fail(e);
          this.want = this.step;
          return;
        }
      }
    } finally {
      this.busy = false;
    }
  }

  /** Navigation ops run one at a time, so each one starts from the previous op's target. */
  nav(op: NavOp, extra: { reg?: string; addr?: string } = {}): Promise<void> {
    const run = async () => {
      try {
        const r = await api.nav(this.want, op, extra);
        if (r.step !== null) this.goto(r.step);
      } catch (e) {
        this.fail(e);
      }
    };
    this.navQ = this.navQ.then(run);
    return this.navQ.then(() => this.pumping);
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
