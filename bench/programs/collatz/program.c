#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

static int64_t chain_length(int64_t start) {
    int64_t value = start;
    int64_t steps = 1;
    while (value != 1) {
        if (value % 2 == 0) {
            value = value / 2;
        } else {
            value = value * 3 + 1;
        }
        steps++;
    }
    return steps;
}

int main(void) {
    int64_t longest = 0;
    int64_t best_start = 0;
    for (int64_t start = 1; start < 1000000; start++) {
        int64_t length = chain_length(start);
        if (length > longest) {
            longest = length;
            best_start = start;
        }
    }
    printf("%" PRId64 " %" PRId64 "\n", best_start, longest);
    return 0;
}
