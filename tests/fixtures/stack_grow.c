/* Touches stack memory far below the initial stack mapping, so the kernel grows the stack. */
static int deep(void) {
    volatile char big[512 * 1024];
    big[0] = 1;
    big[sizeof big - 1] = 2;
    return big[0] + big[sizeof big - 1];
}

int main(void) { return deep() == 3 ? 0 : 1; }
