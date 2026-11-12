#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

static int64_t fibonacci(int64_t number) {
    if (number < 2) {
        return number;
    }
    return fibonacci(number - 1) + fibonacci(number - 2);
}

int main(void) {
    printf("%" PRId64 "\n", fibonacci(40));
    return 0;
}
