#include <signal.h>

/* Stops itself with SIGSTOP: before main when given an argument, inside main otherwise. */
__attribute__((constructor)) static void early(int argc, char **argv) {
    (void)argv;
    if (argc > 1) raise(SIGSTOP);
}

int main(int argc, char **argv) {
    (void)argv;
    if (argc == 1) raise(SIGSTOP);
    return 0;
}
