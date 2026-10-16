// Why each System V rule that `check-abi` enforces exists, keyed by the rule name of a finding.

export const ABI_RULES: Record<string, string> = {
  "call-alignment":
    "System V requires %rsp to be a multiple of 16 when a call instruction runs, so every function starts with %rsp mod 16 = 8. Code that keeps 16-byte values on the stack with aligned instructions such as movaps crashes when a caller breaks this.",
  "callee-saved":
    "%rbx, %rbp and %r12–%r15 belong to the caller. A function that uses them must restore their values before it returns, usually with push and pop, and %rsp must also be back where it was before the call.",
  "direction-flag":
    "The direction flag must be clear at calls and returns so string instructions such as rep movsb copy forward. Code that sets it with std must clear it with cld before calling or returning.",
  "return-address":
    "ret went somewhere other than the address its call pushed. The return address on the stack was overwritten, or %rsp was not back at the return-address slot when ret ran.",
};
