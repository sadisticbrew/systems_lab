#include <signal.h>
#include <unistd.h>

static void handler(int sig) { (void)sig; _exit(3); }

int main(void) {
    signal(SIGSEGV, handler);
    /* push from an unmapped address: faults before pushing anything */
    __asm__ volatile("push (%0)" : : "r"(0UL) : "memory");
    return 0;
}
