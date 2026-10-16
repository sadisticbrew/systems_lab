# Stack alignment at calls.
    .globl _start
    .text
_start:
    call   good             # %rsp mod 16 is 0 here, as System V requires
    sub    $8, %rsp
    call   bad              # %rsp mod 16 is 8 here: this call breaks the rule
    add    $8, %rsp
    mov    $60, %eax        # exit(0)
    xor    %edi, %edi
    syscall
good:
    push   %rbp
    mov    %rsp, %rbp
    pop    %rbp
    ret
bad:
    ret
