# Recursion, stack frames and return values

`fact.c` computes 5! recursively. Every call to `fact` gets its own stack frame, and those frames are how the CPU keeps five different values of `n` apart. Step with → and watch the Stack panel; press p to predict each instruction's effect before you see it.

## Where a frame starts

@go fact

This is the first instruction of `fact(5)`. The `call` in `main` has just pushed the return address, so `%rsp` points at it.

@predict rsp mod 16 at fact

> System V requires `%rsp` to be a multiple of 16 when `call` runs. The call then pushes an 8-byte return address, so every function starts with `%rsp mod 16 = 8`. The prologue's `push %rbp` makes it a multiple of 16 again.

## Five copies of n

@predict n at fact.c:4@3

> Each activation keeps its own `n` at the same offset from its own `%rbp`. The third time line 4 runs is inside `fact(3)`, so its `n` is 3, while the frames above it still hold 5 and 4. Look at the Stack panel to see all three.

## The return value travels in %rax

@predict rax at fact.c:6@3

> By the closing brace of `fact(3)` the multiplication has left 3 × fact(2) = 6 in `%rax`, where the System V calling convention puts an integer return value. The caller's `imul` reads it right after the `call` returns.
