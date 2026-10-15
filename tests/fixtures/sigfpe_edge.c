#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static void handler(int sig) { (void)sig; _exit(3); }

/* Put %rsp just above the lower edge of the stack mapping, then divide by
   zero: the kernel grows the stack while it builds the signal frame. */
int main(void) {
    unsigned long lo = 0, hi;
    char line[256];
    FILE *f = fopen("/proc/self/maps", "r");
    while (f && fgets(line, sizeof line, f))
        if (sscanf(line, "%lx-%lx", &lo, &hi) == 2 && line[strlen(line) - 2] == ']' && strstr(line, "[stack]"))
            break;
    if (f) fclose(f);
    signal(SIGFPE, handler);
    unsigned long top = (lo + 64) & ~15UL;
    __asm__ volatile("mov %%rax, %%rsp\n\txor %%edx, %%edx\n\txor %%ecx, %%ecx\n\tdiv %%rcx"
                     : : "a"(top) : "rdx", "rcx", "memory");
    return 0;
}
