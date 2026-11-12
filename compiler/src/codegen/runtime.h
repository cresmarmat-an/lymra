/*
 * The Lymra runtime for edition 0.1.
 *
 * The compiler puts this text at the top of every C file it generates, so a
 * generated file builds on its own. It builds with GCC, Clang and MSVC, on
 * POSIX systems and on Windows.
 */

/*
 * Output goes out through the system's own write function, and a stack
 * overflow is caught by the system's own mechanism: a signal handler on POSIX
 * systems, an exception handler on Windows. What they need has to be asked
 * for before any header is included.
 */
#if defined(__unix__) || defined(__APPLE__)
#define LYMRA_POSIX 1
#define _XOPEN_SOURCE 700
#elif defined(_WIN32)
#define LYMRA_WINDOWS 1
#define WIN32_LEAN_AND_MEAN
#endif

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(LYMRA_POSIX)
#include <errno.h>
#include <signal.h>
#include <sys/resource.h>
#include <unistd.h>
#elif defined(LYMRA_WINDOWS)
#include <windows.h>
#endif

/* What the runtime needs from the C compiler, spelled the way each compiler spells it. */
#if defined(__GNUC__) || defined(__clang__)
/* Marks the runtime functions and the program parts a given program may not use, so C compilers do not warn. */
#define LYMRA_UNUSED __attribute__((unused))
#define LYMRA_NO_RETURN __attribute__((noreturn, cold, noinline))
#define LYMRA_NO_INLINE __attribute__((noinline))
#define LYMRA_UNREACHABLE() __builtin_unreachable()
/* Keeps the compiler from moving memory accesses across this point, so a signal handler sees them in order. */
#define LYMRA_SIGNAL_FENCE() __asm__ __volatile__("" ::: "memory")
#elif defined(_MSC_VER)
#include <intrin.h>
#define LYMRA_UNUSED
#define LYMRA_NO_RETURN __declspec(noreturn) __declspec(noinline)
#define LYMRA_NO_INLINE __declspec(noinline)
#define LYMRA_UNREACHABLE() __assume(0)
#define LYMRA_SIGNAL_FENCE() _ReadWriteBarrier()
/*
 * Two MSVC warnings do not apply to generated code: a program uses only part
 * of the runtime, and MSVC has no attribute for unused functions (C4505);
 * and once runtime checks are inlined, the optimizer finds trap paths that a
 * known value can never take (C4702). Lymra itself rejects unreachable code.
 */
#pragma warning(disable : 4505 4702)
#else
#error "Lymra programs are compiled with GCC, Clang or MSVC"
#endif

/* The exit status of a program stopped by a runtime error. 70 is EX_SOFTWARE in sysexits.h. */
#define LYMRA_RUNTIME_ERROR_STATUS 70

/* The exit status of a program whose output could not be written. 74 is EX_IOERR in sysexits.h. */
#define LYMRA_OUTPUT_ERROR_STATUS 74

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

/* Where text goes. The values are the POSIX file descriptors. */
typedef enum { LYMRA_STANDARD_OUTPUT = 1, LYMRA_STANDARD_ERROR = 2 } lymra_stream;

#if defined(LYMRA_WINDOWS)
/* Whether each stream, by its number, is a console. A console takes UTF-16 text; files and pipes take the bytes. */
LYMRA_UNUSED static bool lymra_console[3];
/* Text on its way to a console. UTF-8 never needs more UTF-16 units than it has bytes. */
LYMRA_UNUSED static wchar_t lymra_console_text[(size_t)1 << 16];
#endif

/*
 * The longest start of `data`, at most `length` bytes long, that does not end
 * inside a UTF-8 character. Text written in pieces then never splits one,
 * which a console would show as two broken characters.
 */
LYMRA_UNUSED static size_t lymra_whole_characters(const char *data, size_t length) {
    size_t back = 0;
    while (back < 3 && back < length && ((unsigned char)data[length - 1 - back] & 0xC0) == 0x80) {
        back++;
    }
    if (back == length) {
        return length;
    }
    unsigned char lead = (unsigned char)data[length - 1 - back];
    size_t size = lead >= 0xF0 ? 4 : lead >= 0xE0 ? 3 : lead >= 0xC0 ? 2 : 1;
    return size > back + 1 ? length - 1 - back : length;
}

/*
 * Writes some of `data` with one write to the system and returns how many
 * bytes went out, which is 0 only when writing failed. On POSIX it only
 * calls `write`, so a signal handler may use it.
 */
LYMRA_UNUSED static size_t lymra_write_some(lymra_stream stream, const char *data, size_t length) {
#if defined(LYMRA_POSIX)
    for (;;) {
        ssize_t result = write((int)stream, data, length);
        if (result > 0) {
            return (size_t)result;
        }
        if (result < 0 && errno == EINTR) {
            continue;
        }
        return 0;
    }
#elif defined(LYMRA_WINDOWS)
    HANDLE handle = GetStdHandle(stream == LYMRA_STANDARD_OUTPUT ? STD_OUTPUT_HANDLE : STD_ERROR_HANDLE);
    size_t capacity = sizeof lymra_console_text / sizeof lymra_console_text[0];
    size_t part = length < capacity ? length : capacity;
    if (!lymra_console[stream]) {
        /* Files and pipes get the bytes as they are, so a line ends with `\n` as on every other system. */
        DWORD written = 0;
        return WriteFile(handle, data, (DWORD)part, &written, NULL) ? written : 0;
    }
    size_t whole = lymra_whole_characters(data, part);
    if (whole > 0) {
        part = whole;
    }
    int units = MultiByteToWideChar(CP_UTF8, 0, data, (int)part, lymra_console_text, (int)capacity);
    if (units <= 0) {
        return 0;
    }
    for (int start = 0; start < units;) {
        DWORD done = 0;
        if (!WriteConsoleW(handle, lymra_console_text + start, (DWORD)(units - start), &done, NULL) || done == 0) {
            return 0;
        }
        start += (int)done;
    }
    return part;
#else
    FILE *file = stream == LYMRA_STANDARD_OUTPUT ? stdout : stderr;
    return fwrite(data, 1, length, file) == length && fflush(file) == 0 ? length : 0;
#endif
}

/* Writes an error message to standard error, as much of it as the system takes. */
LYMRA_UNUSED static void lymra_write_error(const char *text) {
    size_t length = strlen(text);
    while (length > 0) {
        size_t taken = lymra_write_some(LYMRA_STANDARD_ERROR, text, length);
        if (taken == 0) {
            return;
        }
        text += taken;
        length -= taken;
    }
}

/*
 * Output waits in a buffer and goes out in large writes, which is much faster
 * than writing each piece. A line becomes part of the output only when its
 * line break is added, so a program that stops in the middle of a `print`
 * leaves no half line behind. Everything that ends the program writes the
 * finished lines first, the stack overflow handler included.
 */
#define LYMRA_OUTPUT_CAPACITY ((size_t)1 << 16)
LYMRA_UNUSED static char lymra_output[LYMRA_OUTPUT_CAPACITY];
/* The end of the finished lines, and how much of them is written already. Volatile, so the handler sees each update. */
LYMRA_UNUSED static volatile size_t lymra_output_length;
LYMRA_UNUSED static volatile size_t lymra_output_written;
/* The end of the line being printed, which follows the finished lines. */
LYMRA_UNUSED static size_t lymra_line_end;
/* Set when output could not be written. The program reports it when it ends. */
LYMRA_UNUSED static bool lymra_output_failed;

/* Writes the finished lines not written yet. It counts what each write took, so a write cut short is never repeated. */
LYMRA_UNUSED static void lymra_write_finished(void) {
    while (lymra_output_written < lymra_output_length) {
        size_t written = lymra_output_written;
        size_t taken = lymra_write_some(LYMRA_STANDARD_OUTPUT, lymra_output + written, lymra_output_length - written);
        if (taken == 0) {
            lymra_output_failed = true;
            lymra_output_written = lymra_output_length;
            return;
        }
        lymra_output_written = written + taken;
    }
}

/* Writes the finished lines, then moves the line being printed to the front of the buffer. */
LYMRA_UNUSED static void lymra_flush(void) {
    lymra_write_finished();
    size_t finished = lymra_output_length;
    size_t unfinished = lymra_line_end - finished;
    memmove(lymra_output, lymra_output + finished, unfinished);
    lymra_line_end = unfinished;
    lymra_output_written = 0;
    lymra_output_length = 0;
}

/* Makes the bytes before `end` part of the output. */
LYMRA_UNUSED static inline void lymra_commit(size_t end) {
    /* Keeps the compiler from moving the end of the output past bytes not stored yet. */
    LYMRA_SIGNAL_FENCE();
    lymra_output_length = end;
}

/* Adds bytes to the line being printed. */
LYMRA_UNUSED static void lymra_append(const char *data, size_t length) {
    while (length > 0) {
        if (lymra_line_end == LYMRA_OUTPUT_CAPACITY) {
            if (lymra_output_length == 0) {
                /* One line fills the whole buffer. It goes out as it is, up to its last whole character. */
                size_t whole = lymra_whole_characters(lymra_output, lymra_line_end);
                lymra_commit(whole > 0 ? whole : lymra_line_end);
            }
            lymra_flush();
        }
        size_t end = lymra_line_end;
        size_t part = length < LYMRA_OUTPUT_CAPACITY - end ? length : LYMRA_OUTPUT_CAPACITY - end;
        memcpy(lymra_output + end, data, part);
        lymra_line_end = end + part;
        data += part;
        length -= part;
    }
}

/*
 * Stops the program with a runtime error. `site` is the place in the Lymra
 * source, already rendered by the compiler: the location line, then the
 * source line with the failing part underlined.
 */
LYMRA_UNUSED LYMRA_NO_RETURN static void lymra_trap(lymra_trap_kind kind, const char *site) {
    static const char *const messages[] = {
        "integer overflow",
        "division by zero",
        "assertion failed",
        "conversion out of range",
    };
    lymra_write_finished();
    lymra_write_error("error: ");
    lymra_write_error(messages[kind]);
    lymra_write_error("\n");
    lymra_write_error(site);
    exit(LYMRA_RUNTIME_ERROR_STATUS);
}

/*
 * Whether `left op right` falls outside the type, and when it does not, its
 * value in `*result`. GCC and Clang compute this with one instruction and its
 * overflow flag. Other compilers get code that never overflows itself: a sum
 * or a difference is compared with the type's limits before it is computed,
 * and a product of types up to 32 bits is exact in 64 bits, so one multiply
 * and one comparison decide it. A 64-bit product comes from the 128-bit
 * multiply of MSVC on x86-64 and ARM64, and elsewhere from comparing with a
 * limit divided by one operand. Defining LYMRA_PORTABLE_ARITHMETIC selects
 * the code without builtins or intrinsics everywhere, which the tests use to
 * check it.
 */
#if (defined(__GNUC__) || defined(__clang__)) && !defined(LYMRA_PORTABLE_ARITHMETIC)
#define LYMRA_OVERFLOW_CHECKS(name, type)                                                                      \
    LYMRA_UNUSED static inline bool lymra_add_overflows_##name(type left, type right, type *result) {          \
        return __builtin_add_overflow(left, right, result);                                                    \
    }                                                                                                          \
    LYMRA_UNUSED static inline bool lymra_subtract_overflows_##name(type left, type right, type *result) {     \
        return __builtin_sub_overflow(left, right, result);                                                    \
    }                                                                                                          \
    LYMRA_UNUSED static inline bool lymra_multiply_overflows_##name(type left, type right, type *result) {     \
        return __builtin_mul_overflow(left, right, result);                                                    \
    }
/* The builtins take every width, so they need no way of checking a product. */
#define LYMRA_SIGNED_OVERFLOW_CHECKS(name, type, minimum, maximum, product) LYMRA_OVERFLOW_CHECKS(name, type)
#define LYMRA_UNSIGNED_OVERFLOW_CHECKS(name, type, maximum, product) LYMRA_OVERFLOW_CHECKS(name, type)
#else
#define LYMRA_SIGNED_OVERFLOW_CHECKS(name, type, minimum, maximum, product)                                    \
    LYMRA_UNUSED static inline bool lymra_add_overflows_##name(type left, type right, type *result) {          \
        if ((right > 0 && left > (maximum) - right) || (right < 0 && left < (minimum) - right)) {              \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)(left + right);                                                                        \
        return false;                                                                                          \
    }                                                                                                          \
    LYMRA_UNUSED static inline bool lymra_subtract_overflows_##name(type left, type right, type *result) {     \
        if ((right < 0 && left > (maximum) + right) || (right > 0 && left < (minimum) + right)) {              \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)(left - right);                                                                        \
        return false;                                                                                          \
    }                                                                                                          \
    product(name, type, minimum, maximum)
#define LYMRA_UNSIGNED_OVERFLOW_CHECKS(name, type, maximum, product)                                           \
    LYMRA_UNUSED static inline bool lymra_add_overflows_##name(type left, type right, type *result) {          \
        if (left > (maximum) - right) {                                                                        \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)(left + right);                                                                        \
        return false;                                                                                          \
    }                                                                                                          \
    LYMRA_UNUSED static inline bool lymra_subtract_overflows_##name(type left, type right, type *result) {     \
        if (left < right) {                                                                                    \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)(left - right);                                                                        \
        return false;                                                                                          \
    }                                                                                                          \
    product(name, type, 0, maximum)
/* A product of types up to 32 bits is exact in 64 bits. */
#define LYMRA_SIGNED_WIDENED_PRODUCT(name, type, minimum, maximum)                                             \
    LYMRA_UNUSED static inline bool lymra_multiply_overflows_##name(type left, type right, type *result) {     \
        int64_t product = (int64_t)left * right;                                                               \
        if (product < (minimum) || product > (maximum)) {                                                      \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)product;                                                                               \
        return false;                                                                                          \
    }
#define LYMRA_UNSIGNED_WIDENED_PRODUCT(name, type, minimum, maximum)                                           \
    LYMRA_UNUSED static inline bool lymra_multiply_overflows_##name(type left, type right, type *result) {     \
        uint64_t product = (uint64_t)left * right;                                                             \
        if (product > (maximum)) {                                                                             \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)product;                                                                               \
        return false;                                                                                          \
    }
#if defined(_MSC_VER) && !defined(LYMRA_PORTABLE_ARITHMETIC) && defined(_M_X64) && !defined(_M_ARM64EC)
/* The high 64 bits of the full product, with the low 64 bits in `*low`, from one 128-bit multiply. */
LYMRA_UNUSED static inline int64_t lymra_signed_product(int64_t left, int64_t right, int64_t *low) {
    int64_t high = 0;
    *low = _mul128(left, right, &high);
    return high;
}
LYMRA_UNUSED static inline uint64_t lymra_unsigned_product(uint64_t left, uint64_t right, uint64_t *low) {
    uint64_t high = 0;
    *low = _umul128(left, right, &high);
    return high;
}
#define LYMRA_FULL_PRODUCT 1
#elif defined(_MSC_VER) && !defined(LYMRA_PORTABLE_ARITHMETIC) && defined(_M_ARM64)
LYMRA_UNUSED static inline int64_t lymra_signed_product(int64_t left, int64_t right, int64_t *low) {
    *low = (int64_t)((uint64_t)left * (uint64_t)right);
    return __mulh(left, right);
}
LYMRA_UNUSED static inline uint64_t lymra_unsigned_product(uint64_t left, uint64_t right, uint64_t *low) {
    *low = left * right;
    return __umulh(left, right);
}
#define LYMRA_FULL_PRODUCT 1
#endif
#if defined(LYMRA_FULL_PRODUCT)
/*
 * A 64-bit product fits when its high half only repeats the sign of its low
 * half. The high half is read in a statement of its own, after the multiply
 * has written the low half.
 */
#define LYMRA_SIGNED_64_PRODUCT(name, type, minimum, maximum)                                                  \
    LYMRA_UNUSED static inline bool lymra_multiply_overflows_##name(type left, type right, type *result) {     \
        type low = 0;                                                                                          \
        type high = lymra_signed_product(left, right, &low);                                                   \
        if (high != (low < 0 ? -1 : 0)) {                                                                      \
            return true;                                                                                       \
        }                                                                                                      \
        *result = low;                                                                                         \
        return false;                                                                                          \
    }
#define LYMRA_UNSIGNED_64_PRODUCT(name, type, minimum, maximum)                                                \
    LYMRA_UNUSED static inline bool lymra_multiply_overflows_##name(type left, type right, type *result) {     \
        type low = 0;                                                                                          \
        type high = lymra_unsigned_product(left, right, &low);                                                 \
        if (high != 0) {                                                                                       \
            return true;                                                                                       \
        }                                                                                                      \
        *result = low;                                                                                         \
        return false;                                                                                          \
    }
#else
/* Without a 128-bit multiply, each case divides a limit by a value that cannot make the division overflow. */
#define LYMRA_SIGNED_64_PRODUCT(name, type, minimum, maximum)                                                  \
    LYMRA_UNUSED static inline bool lymra_multiply_overflows_##name(type left, type right, type *result) {     \
        if (left > 0 ? (right > 0 ? left > (maximum) / right : right < (minimum) / left)                       \
                     : (right > 0 ? left < (minimum) / right : left != 0 && right < (maximum) / left)) {       \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)(left * right);                                                                        \
        return false;                                                                                          \
    }
#define LYMRA_UNSIGNED_64_PRODUCT(name, type, minimum, maximum)                                                \
    LYMRA_UNUSED static inline bool lymra_multiply_overflows_##name(type left, type right, type *result) {     \
        if (left != 0 && right > (maximum) / left) {                                                           \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)(left * right);                                                                        \
        return false;                                                                                          \
    }
#endif
#endif

/* The trapping form of each check, which the generated code calls. */
#define LYMRA_CHECKED_ARITHMETIC(name, type)                                                                   \
    LYMRA_UNUSED static inline type lymra_add_##name(type left, type right, const char *site) {                \
        type result = 0;                                                                                       \
        if (lymra_add_overflows_##name(left, right, &result)) {                                                \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                          \
        }                                                                                                      \
        return result;                                                                                         \
    }                                                                                                          \
    LYMRA_UNUSED static inline type lymra_subtract_##name(type left, type right, const char *site) {           \
        type result = 0;                                                                                       \
        if (lymra_subtract_overflows_##name(left, right, &result)) {                                           \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                          \
        }                                                                                                      \
        return result;                                                                                         \
    }                                                                                                          \
    LYMRA_UNUSED static inline type lymra_multiply_##name(type left, type right, const char *site) {           \
        type result = 0;                                                                                       \
        if (lymra_multiply_overflows_##name(left, right, &result)) {                                           \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                          \
        }                                                                                                      \
        return result;                                                                                         \
    }

/* Checked arithmetic for a signed type, with division and negation. */
#define LYMRA_SIGNED_OPERATIONS(name, type, minimum, maximum, product)                                         \
    LYMRA_SIGNED_OVERFLOW_CHECKS(name, type, minimum, maximum, product)                                        \
    LYMRA_CHECKED_ARITHMETIC(name, type)                                                                       \
    LYMRA_UNUSED static inline type lymra_divide_##name(type left, type right, const char *site) {             \
        if (right == 0) {                                                                                      \
            lymra_trap(LYMRA_DIVISION_BY_ZERO, site);                                                          \
        }                                                                                                      \
        if (left == (minimum) && right == -1) {                                                                \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                          \
        }                                                                                                      \
        return (type)(left / right);                                                                           \
    }                                                                                                          \
    LYMRA_UNUSED static inline type lymra_remainder_##name(type left, type right, const char *site) {          \
        if (right == 0) {                                                                                      \
            lymra_trap(LYMRA_DIVISION_BY_ZERO, site);                                                          \
        }                                                                                                      \
        /* The remainder of anything divided by -1 is 0. In C, minimum % -1 is undefined. */                   \
        if (right == -1) {                                                                                     \
            return 0;                                                                                          \
        }                                                                                                      \
        return (type)(left % right);                                                                           \
    }                                                                                                          \
    LYMRA_UNUSED static inline type lymra_negate_##name(type value, const char *site) {                        \
        if (value == (minimum)) {                                                                              \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                          \
        }                                                                                                      \
        return (type)(-value);                                                                                 \
    }

/* Checked arithmetic for an unsigned type, with division. Lymra has no negation of unsigned values. */
#define LYMRA_UNSIGNED_OPERATIONS(name, type, maximum, product)                                                \
    LYMRA_UNSIGNED_OVERFLOW_CHECKS(name, type, maximum, product)                                               \
    LYMRA_CHECKED_ARITHMETIC(name, type)                                                                       \
    LYMRA_UNUSED static inline type lymra_divide_##name(type left, type right, const char *site) {             \
        if (right == 0) {                                                                                      \
            lymra_trap(LYMRA_DIVISION_BY_ZERO, site);                                                          \
        }                                                                                                      \
        return (type)(left / right);                                                                           \
    }                                                                                                          \
    LYMRA_UNUSED static inline type lymra_remainder_##name(type left, type right, const char *site) {          \
        if (right == 0) {                                                                                      \
            lymra_trap(LYMRA_DIVISION_BY_ZERO, site);                                                          \
        }                                                                                                      \
        return (type)(left % right);                                                                           \
    }

LYMRA_SIGNED_OPERATIONS(integer8, int8_t, INT8_MIN, INT8_MAX, LYMRA_SIGNED_WIDENED_PRODUCT)
LYMRA_SIGNED_OPERATIONS(integer16, int16_t, INT16_MIN, INT16_MAX, LYMRA_SIGNED_WIDENED_PRODUCT)
LYMRA_SIGNED_OPERATIONS(integer32, int32_t, INT32_MIN, INT32_MAX, LYMRA_SIGNED_WIDENED_PRODUCT)
LYMRA_SIGNED_OPERATIONS(integer64, int64_t, INT64_MIN, INT64_MAX, LYMRA_SIGNED_64_PRODUCT)
LYMRA_UNSIGNED_OPERATIONS(unsigned8, uint8_t, UINT8_MAX, LYMRA_UNSIGNED_WIDENED_PRODUCT)
LYMRA_UNSIGNED_OPERATIONS(unsigned16, uint16_t, UINT16_MAX, LYMRA_UNSIGNED_WIDENED_PRODUCT)
LYMRA_UNSIGNED_OPERATIONS(unsigned32, uint32_t, UINT32_MAX, LYMRA_UNSIGNED_WIDENED_PRODUCT)
LYMRA_UNSIGNED_OPERATIONS(unsigned64, uint64_t, UINT64_MAX, LYMRA_UNSIGNED_64_PRODUCT)

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
    lymra_append(text.data, text.length);
}

/* Writes the decimal digits of `value`, after a minus sign when `negative`. */
LYMRA_UNUSED static void lymra_write_digits(uint64_t value, bool negative) {
    /* 20 digits hold the largest `unsigned64`; one more holds the sign. */
    char digits[21];
    size_t start = sizeof digits;
    do {
        digits[--start] = (char)('0' + value % 10);
        value /= 10;
    } while (value != 0);
    if (negative) {
        digits[--start] = '-';
    }
    lymra_append(digits + start, sizeof digits - start);
}

LYMRA_UNUSED static inline void lymra_write_signed(int64_t value) {
    /* Unsigned arithmetic has room for the magnitude of the lowest value, which `int64_t` does not. */
    uint64_t magnitude = value < 0 ? (uint64_t)0 - (uint64_t)value : (uint64_t)value;
    lymra_write_digits(magnitude, value < 0);
}

LYMRA_UNUSED static inline void lymra_write_unsigned(uint64_t value) {
    lymra_write_digits(value, false);
}

LYMRA_UNUSED static inline void lymra_write_boolean(bool value) {
    if (value) {
        lymra_append("true", 4);
    } else {
        lymra_append("false", 5);
    }
}

/* Ends the line being printed, which makes it part of the output. */
LYMRA_UNUSED static inline void lymra_end_line(void) {
    lymra_append("\n", 1);
    lymra_commit(lymra_line_end);
}

/* What a stack overflow writes, on every system that catches one. */
LYMRA_UNUSED static const char lymra_stack_overflow_message[] =
    "error: stack overflow: calls nested too deeply\n"
    "help: a function kept calling itself, or functions kept calling one another; "
    "make sure every chain of calls reaches a case that returns\n";

#if defined(LYMRA_POSIX)
/*
 * Calls that nest too deeply run the stack into its guard page, and the
 * system sends SIGSEGV, or SIGBUS on some systems. The handler runs on a
 * stack of its own, writes the waiting output and a runtime error, and ends
 * the program. Calls cost nothing extra for this.
 */

/* The stack limit the program sets when the system sets none, so deep calls overflow instead of using up memory. */
#define LYMRA_UNLIMITED_STACK_SIZE ((size_t)1 << 28)
/*
 * How far past the stack's limit a fault still counts as an overflow: the
 * largest frame a function can have. Phase 1 frames hold only numbers, far
 * below this. Once arrays can live on the stack, large frames will need stack
 * probing, so that they cannot step over the guard page.
 */
#define LYMRA_STACK_SLACK ((size_t)1 << 20)

LYMRA_UNUSED static char lymra_signal_stack[(size_t)1 << 16];
/* The stack position where the program starts, and how far below it the stack can reach. */
LYMRA_UNUSED static uintptr_t lymra_stack_top;
LYMRA_UNUSED static size_t lymra_stack_reach;

LYMRA_UNUSED static void lymra_on_fault(int signal_number, siginfo_t *details, void *context) {
    (void)context;
    /* The finished lines go out first, whatever the signal means. */
    lymra_write_finished();
    uintptr_t address = (uintptr_t)details->si_addr;
    /* A positive code means the program's own memory access caused the signal; another process can send one too. */
    bool caused_by_access = details->si_code > 0;
    if (caused_by_access && address < lymra_stack_top && lymra_stack_top - address <= lymra_stack_reach) {
        lymra_write_error(lymra_stack_overflow_message);
        _exit(LYMRA_RUNTIME_ERROR_STATUS);
    }
    /* Any other signal ends the program the way it would without this handler. */
    static struct sigaction default_action;
    sigaction(signal_number, &default_action, NULL);
    raise(signal_number);
}

/* Records where the stack starts and installs the handler. If the system refuses, an overflow crashes as it would in C. */
LYMRA_UNUSED static void lymra_catch_stack_overflow(void) {
    struct rlimit limit;
    if (getrlimit(RLIMIT_STACK, &limit) != 0) {
        return;
    }
    if (limit.rlim_cur == RLIM_INFINITY) {
        limit.rlim_cur = LYMRA_UNLIMITED_STACK_SIZE;
        if (setrlimit(RLIMIT_STACK, &limit) != 0) {
            return;
        }
    }
    char top;
    lymra_stack_top = (uintptr_t)&top;
    /* `rlim_t` is unsigned on most systems and signed on some, such as FreeBSD; the limit itself is never negative. */
    uintmax_t size = (uintmax_t)limit.rlim_cur;
    size_t largest = SIZE_MAX - LYMRA_STACK_SLACK;
    lymra_stack_reach = (size < largest ? (size_t)size : largest) + LYMRA_STACK_SLACK;

    stack_t alternate;
    alternate.ss_sp = lymra_signal_stack;
    alternate.ss_size = sizeof lymra_signal_stack;
    alternate.ss_flags = 0;
    if (sigaltstack(&alternate, NULL) != 0) {
        return;
    }
    struct sigaction action;
    memset(&action, 0, sizeof action);
    action.sa_sigaction = lymra_on_fault;
    action.sa_flags = SA_SIGINFO | SA_ONSTACK;
    sigemptyset(&action.sa_mask);
    sigaction(SIGSEGV, &action, NULL);
    sigaction(SIGBUS, &action, NULL);
}
#elif defined(LYMRA_WINDOWS)
/*
 * Calls that nest too deeply run the stack into its guard page, and Windows
 * raises a stack overflow exception on the same thread. The handler writes
 * the waiting output and a runtime error with stack the system keeps back
 * for it, and ends the program. Calls cost nothing extra for this.
 */

/* Stack kept back for the handler, which runs after the rest is used up. */
#define LYMRA_HANDLER_STACK ((ULONG)64 << 10)

LYMRA_UNUSED static LONG WINAPI lymra_on_exception(EXCEPTION_POINTERS *exception) {
    if (exception->ExceptionRecord->ExceptionCode != EXCEPTION_STACK_OVERFLOW) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    lymra_write_finished();
    lymra_write_error(lymra_stack_overflow_message);
    /* Ends the process at once: an orderly exit would run code that needs more stack. */
    TerminateProcess(GetCurrentProcess(), LYMRA_RUNTIME_ERROR_STATUS);
    return EXCEPTION_CONTINUE_SEARCH;
}

/* Keeps stack back for the handler and installs it. If Windows refuses, an overflow crashes as it would in C. */
LYMRA_UNUSED static void lymra_catch_stack_overflow(void) {
    ULONG reserve = LYMRA_HANDLER_STACK;
    if (SetThreadStackGuarantee(&reserve)) {
        AddVectoredExceptionHandler(1, lymra_on_exception);
    }
}
#endif

/* Prepares the program to run. The C `main` calls it before anything else. */
LYMRA_UNUSED LYMRA_NO_INLINE static void lymra_start(void) {
#if defined(LYMRA_POSIX)
    lymra_catch_stack_overflow();
#elif defined(LYMRA_WINDOWS)
    DWORD mode = 0;
    lymra_console[LYMRA_STANDARD_OUTPUT] = GetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), &mode) != 0;
    lymra_console[LYMRA_STANDARD_ERROR] = GetConsoleMode(GetStdHandle(STD_ERROR_HANDLE), &mode) != 0;
    lymra_catch_stack_overflow();
#endif
}

/* Writes the waiting output when `main` returns, and reports if any of it could not be written. */
LYMRA_UNUSED static inline int lymra_finish(void) {
    lymra_flush();
    if (lymra_output_failed) {
        lymra_write_error("error: the output could not be written\n");
        return LYMRA_OUTPUT_ERROR_STATUS;
    }
    return 0;
}
