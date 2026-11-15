#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

/* A list of booleans that grows one element at a time, doubling its storage when full. */
typedef struct {
    bool *elements;
    int64_t length;
    int64_t capacity;
} booleans;

static void push(booleans *list, bool value) {
    if (list->length == list->capacity) {
        int64_t capacity = list->capacity == 0 ? 16 : list->capacity * 2;
        bool *grown = realloc(list->elements, (size_t)capacity * sizeof *grown);
        if (grown == NULL) {
            abort();
        }
        list->elements = grown;
        list->capacity = capacity;
    }
    list->elements[list->length++] = value;
}

int main(void) {
    booleans composite = {NULL, 0, 0};
    for (int64_t index = 0; index < 100000000; index++) {
        push(&composite, false);
    }
    int64_t count = 0;
    for (int64_t candidate = 2; candidate < composite.length; candidate++) {
        if (!composite.elements[candidate]) {
            count++;
            for (int64_t multiple = candidate * candidate; multiple < composite.length; multiple += candidate) {
                composite.elements[multiple] = true;
            }
        }
    }
    printf("%" PRId64 "\n", count);
    free(composite.elements);
    return 0;
}
