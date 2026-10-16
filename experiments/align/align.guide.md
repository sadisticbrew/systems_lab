# Stack alignment at calls

System V requires `%rsp` to be a multiple of 16 whenever a `call` instruction runs. `align.s` makes one call that follows the rule and one that breaks it. The Stack panel header shows `rsp mod 16` at every step.

## At process entry

@predict rsp mod 16 at _start

> The kernel starts every process with `%rsp` 16-byte aligned, so `_start` sees 0.

## Inside a well-aligned call

@predict rsp mod 16 at good

> `call good` ran with `%rsp mod 16 = 0` and pushed an 8-byte return address, so `good` starts at 8. This is what every function expects on entry.

@predict rsp mod 16 at good after

> `push %rbp` subtracts another 8, which makes `%rsp` a multiple of 16 again. That is why the standard prologue leaves the stack aligned for further calls.

## Breaking the rule

@go bad

`_start` subtracted 8 from `%rsp` before calling `bad`.

@predict rsp mod 16 at bad

> The call ran with `%rsp mod 16 = 8`, so `bad` starts at 0 instead of 8. Open the ABI tab: `check-abi` reports this call. A function that keeps 16-byte values on its stack with aligned instructions such as `movaps` would crash here.
