#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

int main(void) {
    int64_t total = 0;
    for (int64_t round = 0; round < 20; round++) {
        for (int64_t index = 0; index < 50000000; index++) {
            total = total + index * 3 + round;
        }
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
