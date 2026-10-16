// The nine user-visible RFLAGS bits, in display order, with what each one means.

export const FLAGS = [
  { name: "CF", bit: 0, meaning: "carry/borrow out of the top bit (unsigned overflow)" },
  { name: "PF", bit: 2, meaning: "the low byte of the result has an even number of 1 bits" },
  { name: "AF", bit: 4, meaning: "carry out of bit 3 (BCD arithmetic)" },
  { name: "ZF", bit: 6, meaning: "the result was zero" },
  { name: "SF", bit: 7, meaning: "copy of the result's sign bit" },
  { name: "TF", bit: 8, meaning: "single-step trap (set by debuggers)" },
  { name: "IF", bit: 9, meaning: "interrupts enabled (always 1 in user programs)" },
  { name: "DF", bit: 10, meaning: "string instructions move backward when set" },
  { name: "OF", bit: 11, meaning: "signed overflow" },
] as const;

/** Whether bit `bit` of `rflags` differs from `prev` (the previous value, or null when `rflags` did not change). */
export function flagChanged(rflags: bigint, prev: bigint | null, bit: number): boolean {
  return prev !== null && (((rflags ^ prev) >> BigInt(bit)) & 1n) === 1n;
}
