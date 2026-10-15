#define _GNU_SOURCE
#include <stdio.h>
#include <sys/mman.h>
#include <unistd.h>

static int file_with(const char *text) {
    int fd = memfd_create("slt", 0);
    if (fd < 0 || write(fd, text, 4) != 4) return -1;
    return fd;
}

int main(void) {
    int a = file_with("AAAA"), b = file_with("BBBB");
    volatile char *p = mmap((void *)0x10000000, 4096, PROT_READ, MAP_SHARED, a, 0);
    char first = p[0];
    mmap((void *)p, 4096, PROT_READ, MAP_SHARED | MAP_FIXED, b, 0);
    printf("%c%c\n", first, p[0]);
    return 0;
}
