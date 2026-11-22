#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    if (argv[1][0] == 'u') {
        volatile unsigned shift = 32;
        volatile unsigned value = 1u << shift;
        (void)value;
    } else {
        char *bytes = (char *)malloc(1);
        if (!bytes) return 3;
        volatile int index = argc;
        volatile char value = bytes[index];
        (void)value;
        free(bytes);
    }
    puts("RESULT 0 0 0 1 -");
    return 0;
}
