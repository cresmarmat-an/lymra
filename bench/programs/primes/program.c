#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

static bool is_prime(int64_t candidate) {
    if (candidate < 2) {
        return false;
    }
    for (int64_t divisor = 2; divisor * divisor <= candidate; divisor++) {
        if (candidate % divisor == 0) {
            return false;
        }
    }
    return true;
}

int main(void) {
    int64_t count = 0;
    for (int64_t candidate = 0; candidate < 3000000; candidate++) {
        if (is_prime(candidate)) {
            count++;
        }
    }
    printf("%" PRId64 "\n", count);
    return 0;
}
