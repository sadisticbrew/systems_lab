#include <sys/wait.h>
#include <unistd.h>

/* Forks: a recording cannot follow the child, so it ends with "unsupported". */
int main(void) {
    pid_t p = fork();
    if (p == 0) {
        for (;;) pause();
    }
    waitpid(p, 0, 0);
    return 0;
}
