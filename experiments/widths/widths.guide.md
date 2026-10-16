# Register-width writes

`widths.s` fills `%rax` with ones and then writes smaller pieces of it: 32, 16 and 8 bits. x86-64 treats these writes differently. Predict each result, then check it.

## A 32-bit write

@go w32

`%rax` holds `0xffffffffffffffff` here. The next instruction writes only `%eax`, the low 32 bits.

@predict rax at w32 after

> Writing a 32-bit register zero-extends the result into the full 64-bit register, so the upper 32 bits become 0 and `%rax` is `0x11223344`. Compilers rely on this: `mov %edi, %eax` copies a 32-bit value and clears the top half in one instruction.

## 16-bit and 8-bit writes

@predict rax at w16 after

> 16-bit writes do not zero-extend. Only bits 0–15 change, so `%rax` is `0xffffffffffff5566`.

@predict rax at w8 after

> A write to `%al` changes only bits 0–7: `0xffffffffffff5577`.

@predict rax at wh after

> `%ah` names bits 8–15 of `%rax`, so `0x88` replaces the `0x55` byte: `0xffffffffffff8877`.

## Zero- versus sign-extension

`%bl` holds `0x80`. As an unsigned byte that is 128; as a signed byte it is -128.

@predict rcx at zext after

> `movzbq` copies the byte and fills the other 56 bits with zeros: `%rcx` is `0x80`, which is 128.

@predict rdx at sext after

> `movsbq` copies the byte's sign bit into the upper 56 bits: `%rdx` is `0xffffffffffffff80`, which is -128 as a 64-bit integer.
