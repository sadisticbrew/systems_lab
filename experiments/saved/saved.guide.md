# Callee-saved registers

System V splits the general-purpose registers into two groups. A function may overwrite the caller-saved ones (`%rax`, `%rcx`, `%rdx`, `%rsi`, `%rdi`, `%r8`–`%r11`) freely, but it must hand `%rbx`, `%rbp` and `%r12`–`%r15` back unchanged. `saved.s` calls one function that keeps that promise and one that breaks it.

## A function that keeps the promise

@go polite

`_start` put 7 in `%rbx` before this call. `polite` wants to use `%rbx` itself.

@predict rbx at polite_use after

> `polite` saved the caller's 7 on the stack with `push %rbx`, so it can now use the register for its own value, 42.

@predict rbx at polite_done after

> `pop %rbx` restores the saved 7 before `ret`, so the caller gets its value back.

## A function that breaks it

@predict rbx at after_rude

> `rude` overwrote `%rbx` with 99 and returned without restoring it. The caller's 7 is gone, which is exactly the bug the rule prevents. The ABI tab shows the `callee-saved` finding for this call.
