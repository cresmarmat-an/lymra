/*
 * The Lymra runtime for edition 0.1.
 *
 * The compiler puts this text at the top of every C file it generates, so a
 * generated file builds on its own. It needs GCC or Clang, for the builtins
 * that detect integer overflow.
 */

#include <inttypes.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#if !defined(__GNUC__)
#error "Lymra programs are compiled with GCC or Clang"
#endif

/* The exit status of a program stopped by a runtime error. 70 is EX_SOFTWARE in sysexits.h. */
#define LYMRA_RUNTIME_ERROR_STATUS 70

/* The exit status of a program whose output could not be written. 74 is EX_IOERR in sysexits.h. */
#define LYMRA_OUTPUT_ERROR_STATUS 74

/* Marks the runtime functions and the program parts a given program may not use, so C compilers do not warn. */
#define LYMRA_UNUSED __attribute__((unused))

/* Immutable text: a pointer into static data and a length in bytes. */
typedef struct {
    const char *data;
    size_t length;
} lymra_string;

typedef enum {
    LYMRA_INTEGER_OVERFLOW,
    LYMRA_DIVISION_BY_ZERO,
    LYMRA_ASSERTION_FAILED,
    LYMRA_CONVERSION_OUT_OF_RANGE
} lymra_trap_kind;

/*
 * Stops the program with a runtime error. `site` is the place in the Lymra
 * source, already rendered by the compiler: the location line, then the
 * source line with the failing part underlined.
 */
LYMRA_UNUSED __attribute__((noreturn, cold, noinline)) static void lymra_trap(lymra_trap_kind kind,
                                                                                const char *site) {
    static const char *const messages[] = {
        "integer overflow",
        "division by zero",
        "assertion failed",
        "conversion out of range",
    };
    fflush(stdout);
    fprintf(stderr, "error: %s\n%s", messages[kind], site);
    exit(LYMRA_RUNTIME_ERROR_STATUS);
}

/* Checked arithmetic for a signed type. `minimum` is its lowest value. */
#define LYMRA_SIGNED_OPERATIONS(name, type, minimum)                                                     \
    LYMRA_UNUSED static inline type lymra_add_##name(type left, type right, const char *site) {          \
        type result;                                                                                     \
        if (__builtin_add_overflow(left, right, &result)) {                                              \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                    \
        }                                                                                                \
        return result;                                                                                   \
    }                                                                                                    \
    LYMRA_UNUSED static inline type lymra_subtract_##name(type left, type right, const char *site) {     \
        type result;                                                                                     \
        if (__builtin_sub_overflow(left, right, &result)) {                                              \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                    \
        }                                                                                                \
        return result;                                                                                   \
    }                                                                                                    \
    LYMRA_UNUSED static inline type lymra_multiply_##name(type left, type right, const char *site) {     \
        type result;                                                                                     \
        if (__builtin_mul_overflow(left, right, &result)) {                                              \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                    \
        }                                                                                                \
        return result;                                                                                   \
    }                                                                                                    \
    LYMRA_UNUSED static inline type lymra_divide_##name(type left, type right, const char *site) {       \
        if (right == 0) {                                                                                \
            lymra_trap(LYMRA_DIVISION_BY_ZERO, site);                                                    \
        }                                                                                                \
        if (left == (minimum) && right == -1) {                                                          \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                    \
        }                                                                                                \
        return (type)(left / right);                                                                     \
    }                                                                                                    \
    LYMRA_UNUSED static inline type lymra_remainder_##name(type left, type right, const char *site) {    \
        if (right == 0) {                                                                                \
            lymra_trap(LYMRA_DIVISION_BY_ZERO, site);                                                    \
        }                                                                                                \
        /* The remainder of anything divided by -1 is 0. In C, minimum % -1 is undefined. */             \
        if (right == -1) {                                                                               \
            return 0;                                                                                    \
        }                                                                                                \
        return (type)(left % right);                                                                     \
    }                                                                                                    \
    LYMRA_UNUSED static inline type lymra_negate_##name(type value, const char *site) {                  \
        if (value == (minimum)) {                                                                        \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                    \
        }                                                                                                \
        return (type)(-value);                                                                           \
    }

/* Checked arithmetic for an unsigned type. Lymra has no negation of unsigned values. */
#define LYMRA_UNSIGNED_OPERATIONS(name, type)                                                            \
    LYMRA_UNUSED static inline type lymra_add_##name(type left, type right, const char *site) {          \
        type result;                                                                                     \
        if (__builtin_add_overflow(left, right, &result)) {                                              \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                    \
        }                                                                                                \
        return result;                                                                                   \
    }                                                                                                    \
    LYMRA_UNUSED static inline type lymra_subtract_##name(type left, type right, const char *site) {     \
        type result;                                                                                     \
        if (__builtin_sub_overflow(left, right, &result)) {                                              \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                    \
        }                                                                                                \
        return result;                                                                                   \
    }                                                                                                    \
    LYMRA_UNUSED static inline type lymra_multiply_##name(type left, type right, const char *site) {     \
        type result;                                                                                     \
        if (__builtin_mul_overflow(left, right, &result)) {                                              \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                    \
        }                                                                                                \
        return result;                                                                                   \
    }                                                                                                    \
    LYMRA_UNUSED static inline type lymra_divide_##name(type left, type right, const char *site) {       \
        if (right == 0) {                                                                                \
            lymra_trap(LYMRA_DIVISION_BY_ZERO, site);                                                    \
        }                                                                                                \
        return (type)(left / right);                                                                     \
    }                                                                                                    \
    LYMRA_UNUSED static inline type lymra_remainder_##name(type left, type right, const char *site) {    \
        if (right == 0) {                                                                                \
            lymra_trap(LYMRA_DIVISION_BY_ZERO, site);                                                    \
        }                                                                                                \
        return (type)(left % right);                                                                     \
    }

LYMRA_SIGNED_OPERATIONS(integer8, int8_t, INT8_MIN)
LYMRA_SIGNED_OPERATIONS(integer16, int16_t, INT16_MIN)
LYMRA_SIGNED_OPERATIONS(integer32, int32_t, INT32_MIN)
LYMRA_SIGNED_OPERATIONS(integer64, int64_t, INT64_MIN)
LYMRA_UNSIGNED_OPERATIONS(unsigned8, uint8_t)
LYMRA_UNSIGNED_OPERATIONS(unsigned16, uint16_t)
LYMRA_UNSIGNED_OPERATIONS(unsigned32, uint32_t)
LYMRA_UNSIGNED_OPERATIONS(unsigned64, uint64_t)

/* Checks a conversion from a signed type: the value has to lie within [minimum, maximum]. */
LYMRA_UNUSED static inline int64_t lymra_check_signed(int64_t value, int64_t minimum, int64_t maximum,
                                                       const char *site) {
    if (value < minimum || value > maximum) {
        lymra_trap(LYMRA_CONVERSION_OUT_OF_RANGE, site);
    }
    return value;
}

/* Checks a conversion from an unsigned type: the value has to be at most `maximum`. */
LYMRA_UNUSED static inline uint64_t lymra_check_unsigned(uint64_t value, uint64_t maximum, const char *site) {
    if (value > maximum) {
        lymra_trap(LYMRA_CONVERSION_OUT_OF_RANGE, site);
    }
    return value;
}

LYMRA_UNUSED static inline void lymra_write_text(lymra_string text) {
    fwrite(text.data, 1, text.length, stdout);
}

LYMRA_UNUSED static inline void lymra_write_signed(int64_t value) {
    printf("%" PRId64, value);
}

LYMRA_UNUSED static inline void lymra_write_unsigned(uint64_t value) {
    printf("%" PRIu64, value);
}

LYMRA_UNUSED static inline void lymra_write_boolean(bool value) {
    fputs(value ? "true" : "false", stdout);
}

LYMRA_UNUSED static inline void lymra_end_line(void) {
    putchar('\n');
}

/* Flushes the output when `main` returns, and reports if any of it could not be written. */
LYMRA_UNUSED static inline int lymra_finish(void) {
    if (fflush(stdout) != 0 || ferror(stdout)) {
        fputs("error: the output could not be written\n", stderr);
        return LYMRA_OUTPUT_ERROR_STATUS;
    }
    return 0;
}
