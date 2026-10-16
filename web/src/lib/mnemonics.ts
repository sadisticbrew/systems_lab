// Plain-English descriptions of the instructions the viewer shows in the Effects panel, keyed by iced's lowercase mnemonic.

const TEXT: Record<string, string> = {
  mov: "Copies the source into the destination. Writing a 32-bit register zeroes bits 32–63 of the full register; 8- and 16-bit writes leave the other bits unchanged.",
  movzx: "Copies a smaller source into a larger destination and fills the upper bits with zeros (zero-extension). AT&T spells it movzbl, movzwl, movzbq…",
  movsx: "Copies a smaller source into a larger destination and fills the upper bits with copies of the source's sign bit (sign-extension). AT&T: movsbl, movswl, movsbq…",
  movsxd: "Sign-extends a 32-bit source into a 64-bit register (AT&T movslq).",
  cdqe: "Sign-extends %eax into %rax (AT&T cltq).",
  cwde: "Sign-extends %ax into %eax (AT&T cwtl).",
  cqo: "Fills %rdx with copies of the sign bit of %rax, making the 128-bit value %rdx:%rax (AT&T cqto); compilers emit it before idivq.",
  cdq: "Fills %edx with copies of the sign bit of %eax (AT&T cltd); emitted before idivl.",
  lea: "Computes the address of the memory operand and stores that address in the destination. It reads no memory and changes no flags, so compilers also use it for arithmetic.",
  push: "Subtracts 8 from %rsp, then stores the operand at the new %rsp.",
  pop: "Loads the 8 bytes at %rsp into the operand, then adds 8 to %rsp.",
  call: "Pushes the address of the next instruction (the return address) and jumps to the target.",
  ret: "Pops the return address from the top of the stack into %rip, so execution continues after the matching call.",
  leave: "Copies %rbp into %rsp, then pops the saved %rbp, undoing the push %rbp; mov %rsp,%rbp prologue.",
  add: "Adds the source to the destination and sets CF, OF, SF, ZF, AF and PF from the result.",
  sub: "Subtracts the source from the destination (dst − src → dst) and sets CF, OF, SF, ZF, AF and PF.",
  adc: "Adds the source plus CF to the destination.",
  sbb: "Subtracts the source plus CF from the destination.",
  inc: "Adds 1. Sets OF, SF, ZF, AF and PF but leaves CF unchanged.",
  dec: "Subtracts 1. Sets OF, SF, ZF, AF and PF but leaves CF unchanged.",
  neg: "Replaces the operand with 0 − operand (two's complement). CF is set unless the operand was 0.",
  imul: "Signed multiply. The two- and three-operand forms keep the low half of the product in the destination and set CF and OF when the full product did not fit; the one-operand form puts the double-width product of %rax and the operand in %rdx:%rax. SF, ZF, AF and PF are undefined afterwards.",
  mul: "Unsigned multiply of %rax (or %eax, %ax, %al) by the operand into %rdx:%rax; CF and OF are set when the upper half is not zero.",
  idiv: "Signed divide of %rdx:%rax (or %edx:%eax, …) by the operand: quotient to %rax, remainder to %rdx. A zero divisor or a quotient that does not fit raises a divide error (#DE), which Linux delivers as SIGFPE.",
  div: "Unsigned divide of %rdx:%rax by the operand: quotient to %rax, remainder to %rdx. A zero divisor or an oversized quotient raises #DE (SIGFPE).",
  and: "Bitwise AND into the destination; clears CF and OF and sets SF, ZF and PF from the result.",
  or: "Bitwise OR into the destination; clears CF and OF and sets SF, ZF and PF from the result.",
  xor: "Bitwise exclusive OR into the destination; clears CF and OF. xor %eax,%eax is the usual way to zero a register.",
  not: "Inverts every bit of the operand; changes no flags.",
  shl: "Shifts left by the count, filling with zeros; CF gets the last bit shifted out.",
  sal: "Same as shl.",
  shr: "Logical right shift: fills the vacated high bits with zeros; CF gets the last bit shifted out.",
  sar: "Arithmetic right shift: fills the vacated high bits with copies of the sign bit, which divides a signed value by a power of two rounding toward −∞.",
  rol: "Rotates the bits left; bits shifted out re-enter on the right.",
  ror: "Rotates the bits right; bits shifted out re-enter on the left.",
  cmp: "Computes dst − src and sets the flags exactly as sub would, then discards the result. A following jcc, setcc or cmovcc tests those flags.",
  test: "Computes dst AND src, sets SF, ZF and PF from it and clears CF and OF, then discards the result. test %rax,%rax checks whether %rax is zero or negative.",
  jmp: "Jumps to the target unconditionally.",
  jcc: "Jumps to the target if the condition holds, otherwise continues with the next instruction. The condition only reads the flags.",
  jrcxz: "Jumps if %rcx is zero; it does not read the flags.",
  setcc: "Stores 1 in the byte operand if the condition holds, else 0.",
  cmovcc: "Copies the source into the destination only if the condition holds. With a 32-bit destination the upper half of the register is zeroed even when the condition is false.",
  xchg: "Swaps the two operands; with a memory operand the swap is atomic.",
  nop: "Does nothing except move to the next instruction.",
  endbr64: "Marks a valid target for indirect jumps and calls under Intel CET; otherwise behaves like nop. -fcf-protection=none removes it.",
  syscall: "Enters the kernel. %rax selects the system call, arguments go in %rdi, %rsi, %rdx, %r10, %r8 and %r9, and the result comes back in %rax. The instruction overwrites %rcx and %r11.",
  int3: "Breakpoint: raises #BP, which Linux delivers as SIGTRAP.",
  ud2: "Always raises an invalid-opcode exception (#UD), delivered as SIGILL. Compilers place it where execution must never arrive.",
  hlt: "Privileged: in a user program it raises a general-protection fault, delivered as SIGSEGV.",
  stosb: "Stores %al/%ax/%eax/%rax at (%rdi) and moves %rdi by the element size (forward while DF is clear). With rep it repeats %rcx times; in this recording each iteration is its own step.",
  stosw: "Stores %al/%ax/%eax/%rax at (%rdi) and moves %rdi by the element size (forward while DF is clear). With rep it repeats %rcx times; in this recording each iteration is its own step.",
  stosd: "Stores %al/%ax/%eax/%rax at (%rdi) and moves %rdi by the element size (forward while DF is clear). With rep it repeats %rcx times; in this recording each iteration is its own step.",
  stosq: "Stores %al/%ax/%eax/%rax at (%rdi) and moves %rdi by the element size (forward while DF is clear). With rep it repeats %rcx times; in this recording each iteration is its own step.",
  movsb: "Copies one element from (%rsi) to (%rdi) and advances both by the element size; with rep it repeats %rcx times, one step per iteration.",
  movsw: "Copies one element from (%rsi) to (%rdi) and advances both by the element size; with rep it repeats %rcx times, one step per iteration.",
  movsq: "Copies one element from (%rsi) to (%rdi) and advances both by the element size; with rep it repeats %rcx times, one step per iteration.",
  pushfq: "Pushes RFLAGS onto the stack.",
  popfq: "Pops the top of the stack into RFLAGS.",
  cld: "Clears the direction flag so string instructions move forward. System V requires DF clear at calls and returns.",
  std: "Sets the direction flag so string instructions move backward.",
  cvtsi2sd: "Converts a signed integer into a double in the low 64 bits of an xmm register.",
  cvttsd2si: "Converts a double into a signed integer, truncating toward zero.",
  addsd: "Double-precision arithmetic on the low 64 bits of xmm registers.",
  subsd: "Double-precision arithmetic on the low 64 bits of xmm registers.",
  mulsd: "Double-precision arithmetic on the low 64 bits of xmm registers.",
  divsd: "Double-precision arithmetic on the low 64 bits of xmm registers.",
  pxor: "Bitwise XOR of 128-bit xmm values; pxor %xmm0,%xmm0 zeroes the register.",
  movaps: "Copies 16 bytes between xmm registers or memory; a memory address that is not 16-byte aligned raises a general-protection fault (SIGSEGV).",
  movups: "Copies 16 bytes between xmm registers or memory without an alignment requirement.",
  movq: "Copies 64 bits between general-purpose registers, xmm registers and memory.",
  movd: "Copies 32 bits between general-purpose registers, xmm registers and memory.",
};

const JCC = TEXT.jcc;
const SETCC = TEXT.setcc;
const CMOVCC = TEXT.cmovcc;
const NOT_JCC = new Set(["jmp", "jrcxz", "jecxz"]);

/** What the instruction does, or undefined for a mnemonic the table does not cover. `j*` (except `jmp`, `jrcxz`, `jecxz`), `set*` and `cmov*` share one text per family. */
export function describe(mnemonic: string): string | undefined {
  const m = mnemonic.toLowerCase();
  if (Object.hasOwn(TEXT, m)) return TEXT[m];
  if (m.startsWith("j") && !NOT_JCC.has(m)) return JCC;
  if (m.startsWith("set")) return SETCC;
  if (m.startsWith("cmov")) return CMOVCC;
  return undefined;
}
