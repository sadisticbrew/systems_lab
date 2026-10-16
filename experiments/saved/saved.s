# Callee-saved registers.
    .globl _start
    .text
_start:
    mov    $7, %rbx
    call   polite           # keeps %rbx
    mov    %rbx, %r12       # %r12 = 7
    call   rude             # clobbers %rbx
after_rude:
    mov    %rbx, %r13       # %r13 = 99
    mov    $60, %eax        # exit(0)
    xor    %edi, %edi
    syscall
polite:
    push   %rbx             # save the caller's %rbx
polite_use:
    mov    $42, %rbx        # use %rbx for our own value
polite_done:
    pop    %rbx             # restore it
    ret
rude:
    mov    $99, %rbx        # overwrites the caller's %rbx and never restores it
    ret
