#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

int main(void) {
    int64_t total = 0;
    for (int64_t index = 0; index < 300000000; index++) {
        total = total + index % 7 * 3;
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
