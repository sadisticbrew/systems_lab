#include <signal.h>
#include <stdio.h>

static volatile int hits;
static void handler(int sig) { (void)sig; hits++; }

int main(void) {
    /* SA_NODEFER: a single-stepping tracer makes the kernel raise SIGTRAP inside
       the handler, and a blocked SIGTRAP would reset the handler to SIG_DFL. */
    struct sigaction sa = {0};
    sa.sa_handler = handler;
    sa.sa_flags = SA_NODEFER;
    sigaction(SIGTRAP, &sa, 0);
    raise(SIGTRAP);
    __asm__ volatile("int3");
    printf("handler ran %d times\n", hits);
    return 0;
}
