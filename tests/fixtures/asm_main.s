# A hand-written main: no .type or .size, so the symbol is NOTYPE with size 0.
    .globl main
    .text
main:
    movl $42, %eax
    ret
