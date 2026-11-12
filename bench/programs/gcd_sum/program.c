#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

static int64_t greatest_common_divisor(int64_t left, int64_t right) {
    while (right != 0) {
        int64_t remainder = left % right;
        left = right;
        right = remainder;
    }
    return left;
}

int main(void) {
    int64_t total = 0;
    for (int64_t first = 1; first < 2000; first++) {
        for (int64_t second = 1; second < 2000; second++) {
            total = total + greatest_common_divisor(first, second);
        }
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
