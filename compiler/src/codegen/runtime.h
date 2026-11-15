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
/* Anonymous memory for data stacks: every system has it, under a name each shows only when asked. */
#if defined(__APPLE__)
#define _DARWIN_C_SOURCE 1
#elif defined(__NetBSD__)
#define _NETBSD_SOURCE 1
#elif defined(__OpenBSD__)
#define _BSD_SOURCE 1
#else
#define _DEFAULT_SOURCE 1
#endif
#elif defined(_WIN32)
#define LYMRA_WINDOWS 1
#define WIN32_LEAN_AND_MEAN
#endif

#include <float.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/*
 * Floats give the same results on every system only when each operation is
 * rounded to its own type: not kept wider, as the x87 unit of 32-bit x86
 * does, and not fused with the next one, as a multiply and an add can be on
 * processors with fused multiply-add. The compiler driver asks GCC and Clang
 * for that; MSVC is told here, and a build that would still keep floats
 * wider stops. A program without floats is not affected.
 */
#if defined(LYMRA_USES_FLOATS)
/* Clang on Windows defines `_MSC_VER` too, but takes `-ffp-contract=off` like GCC and rejects this pragma. */
#if defined(_MSC_VER) && !defined(__clang__)
#pragma fp_contract(off)
#endif
#if defined(FLT_EVAL_METHOD) && FLT_EVAL_METHOD != 0 && FLT_EVAL_METHOD != -1
#error "Lymra programs need floats computed in their own type: build with SSE2 math, as -msse2 -mfpmath=sse"
#endif
#endif

#if defined(LYMRA_POSIX)
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <sys/mman.h>
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
 * Four MSVC warnings do not apply to generated code. A program uses only part
 * of the runtime, and MSVC has no attribute for unused functions (C4505).
 * Once runtime checks are inlined, the optimizer finds trap paths that a
 * known value can never take (C4702), and Lymra itself rejects unreachable
 * code. Inlining can also make an operand known: a divisor of zero (C4723),
 * where Lymra stops the program before any integer division by zero and a
 * float division by zero gives infinity or NaN, as IEEE 754 defines; or a
 * float too large for `float32` (C4756), on a path the conversion's check
 * stops first.
 */
#pragma warning(disable : 4505 4702 4723 4756)
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
    LYMRA_CONVERSION_OUT_OF_RANGE,
    LYMRA_DATA_STACK_FULL,
    LYMRA_NOT_ON_TOP,
    LYMRA_OUT_OF_MEMORY
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
 * source line with the failing part underlined. An error whose message and
 * place do not say what to do adds a line of help after them.
 */
LYMRA_UNUSED LYMRA_NO_RETURN static void lymra_trap(lymra_trap_kind kind, const char *site) {
    static const char *const messages[] = {
        "integer overflow",
        "division by zero",
        "assertion failed",
        "conversion out of range",
        "data stack full",
        "a list changed while a newer list sat above it on its data stack",
        "out of memory",
    };
    static const char *const helps[] = {
        NULL,
        NULL,
        NULL,
        NULL,
        "help: a list outgrew the address space its data stack reserves, up to 64 GiB on a 64-bit system "
        "and less on a 32-bit one; look for a loop that keeps adding to a list\n",
        "help: this is a bug in the Lymra compiler, which should have rejected the program with error "
        "L0704; please report it with the source file\n",
        "help: the system has no memory left for a list to grow into\n",
    };
    lymra_write_finished();
    lymra_write_error("error: ");
    lymra_write_error(messages[kind]);
    lymra_write_error("\n");
    lymra_write_error(site);
    if (helps[kind] != NULL) {
        lymra_write_error(helps[kind]);
    }
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

/*
 * Overflow tests for an operation whose second operand is a constant of the
 * program, such as the 3 and the 1 of `value * 3 + 1`. They compare the other
 * operand with limits the C compiler works out while compiling, so the
 * operation itself stays plain arithmetic the compiler can fold: on x86 both
 * of those become one `lea`. An overflow flag would tie each operation to an
 * instruction that sets it. Each test is exact for any second operand,
 * constant or not.
 */
#define LYMRA_SIGNED_CONSTANT_CHECKS(name, type, minimum, maximum)                                             \
    LYMRA_UNUSED static inline bool lymra_add_constant_overflows_##name(type left, type constant,              \
                                                                       type *result) {                         \
        if (constant > 0 ? left > (maximum) - constant : left < (minimum) - constant) {                        \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)(left + constant);                                                                     \
        return false;                                                                                          \
    }                                                                                                          \
    LYMRA_UNUSED static inline bool lymra_subtract_constant_overflows_##name(type left, type constant,         \
                                                                            type *result) {                    \
        if (constant > 0 ? left < (minimum) + constant : left > (maximum) + constant) {                        \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)(left - constant);                                                                     \
        return false;                                                                                          \
    }                                                                                                          \
    /* A negative constant swaps which limit bounds which side, and -1 overflows the lowest value alone. */    \
    LYMRA_UNUSED static inline bool lymra_multiply_constant_overflows_##name(type left, type constant,         \
                                                                            type *result) {                    \
        if (constant > 0    ? left > (maximum) / constant || left < (minimum) / constant                       \
            : constant < -1 ? left < (maximum) / constant || left > (minimum) / constant                       \
                            : constant == -1 && left == (minimum)) {                                           \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)(left * constant);                                                                     \
        return false;                                                                                          \
    }
#define LYMRA_UNSIGNED_CONSTANT_CHECKS(name, type, maximum)                                                    \
    LYMRA_UNUSED static inline bool lymra_add_constant_overflows_##name(type left, type constant,              \
                                                                       type *result) {                         \
        if (left > (maximum) - constant) {                                                                     \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)(left + constant);                                                                     \
        return false;                                                                                          \
    }                                                                                                          \
    LYMRA_UNUSED static inline bool lymra_subtract_constant_overflows_##name(type left, type constant,         \
                                                                            type *result) {                    \
        if (left < constant) {                                                                                 \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)(left - constant);                                                                     \
        return false;                                                                                          \
    }                                                                                                          \
    LYMRA_UNUSED static inline bool lymra_multiply_constant_overflows_##name(type left, type constant,         \
                                                                            type *result) {                    \
        if (constant != 0 && left > (maximum) / constant) {                                                    \
            return true;                                                                                       \
        }                                                                                                      \
        *result = (type)(left * constant);                                                                     \
        return false;                                                                                          \
    }

/* The trapping form of each check, which the generated code calls. */
#define LYMRA_TRAPPING(operation, name, type)                                                                  \
    LYMRA_UNUSED static inline type lymra_##operation##_##name(type left, type right, const char *site) {      \
        type result = 0;                                                                                       \
        if (lymra_##operation##_overflows_##name(left, right, &result)) {                                      \
            lymra_trap(LYMRA_INTEGER_OVERFLOW, site);                                                          \
        }                                                                                                      \
        return result;                                                                                         \
    }
#define LYMRA_CHECKED_ARITHMETIC(name, type)                                                                   \
    LYMRA_TRAPPING(add, name, type)                                                                            \
    LYMRA_TRAPPING(subtract, name, type)                                                                       \
    LYMRA_TRAPPING(multiply, name, type)                                                                       \
    LYMRA_TRAPPING(add_constant, name, type)                                                                   \
    LYMRA_TRAPPING(subtract_constant, name, type)                                                              \
    LYMRA_TRAPPING(multiply_constant, name, type)

/* Checked arithmetic for a signed type, with division and negation. */
#define LYMRA_SIGNED_OPERATIONS(name, type, minimum, maximum, product)                                         \
    LYMRA_SIGNED_OVERFLOW_CHECKS(name, type, minimum, maximum, product)                                        \
    LYMRA_SIGNED_CONSTANT_CHECKS(name, type, minimum, maximum)                                                 \
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
    LYMRA_UNSIGNED_CONSTANT_CHECKS(name, type, maximum)                                                        \
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

/* Writes `magnitude` in decimal to standard error, after a minus sign when `negative`. */
LYMRA_UNUSED static void lymra_write_error_number(uint64_t magnitude, bool negative) {
    /* 20 digits hold the largest `unsigned64`; one more holds the sign, and one ends the text. */
    char digits[22];
    size_t start = sizeof digits - 1;
    digits[start] = '\0';
    do {
        digits[--start] = (char)('0' + magnitude % 10);
        magnitude /= 10;
    } while (magnitude != 0);
    if (negative) {
        digits[--start] = '-';
    }
    lymra_write_error(digits + start);
}

/*
 * Stops the program because an index of `magnitude`, negative or not, lies
 * outside `indexed`, "an array", "a list" or "a view", of `length` elements.
 */
LYMRA_UNUSED LYMRA_NO_RETURN static void lymra_trap_index(uint64_t magnitude, bool negative, uint64_t length,
                                                          const char *indexed, const char *site) {
    lymra_write_finished();
    lymra_write_error("error: index ");
    lymra_write_error_number(magnitude, negative);
    lymra_write_error(" is out of range for ");
    lymra_write_error(indexed);
    lymra_write_error(" of length ");
    lymra_write_error_number(length, false);
    lymra_write_error("\n");
    lymra_write_error(site);
    exit(LYMRA_RUNTIME_ERROR_STATUS);
}

/* Checks a signed index into `indexed`, of `length` elements, and returns it as a position. */
LYMRA_UNUSED static inline size_t lymra_index_signed(int64_t index, uint64_t length, const char *indexed,
                                                     const char *site) {
    if (index < 0) {
        lymra_trap_index((uint64_t)0 - (uint64_t)index, true, length, indexed, site);
    }
    if ((uint64_t)index >= length) {
        lymra_trap_index((uint64_t)index, false, length, indexed, site);
    }
    return (size_t)index;
}

/* Checks an unsigned index into `indexed`, of `length` elements, and returns it as a position. */
LYMRA_UNUSED static inline size_t lymra_index_unsigned(uint64_t index, uint64_t length, const char *indexed,
                                                       const char *site) {
    if (index >= length) {
        lymra_trap_index(index, false, length, indexed, site);
    }
    return (size_t)index;
}

/*
 * Stops the program because a slice cannot start, or end, at `magnitude`,
 * negative or not: `part` says which. `indexed` is what the slice is taken
 * from, as "a list" or "a text", which has `length` elements or bytes.
 */
LYMRA_UNUSED LYMRA_NO_RETURN static void lymra_trap_slice(const char *part, uint64_t magnitude, bool negative,
                                                          uint64_t length, const char *indexed,
                                                          const char *site) {
    lymra_write_finished();
    lymra_write_error("error: slice ");
    lymra_write_error(part);
    lymra_write_error(" ");
    lymra_write_error_number(magnitude, negative);
    lymra_write_error(" is out of range for ");
    lymra_write_error(indexed);
    lymra_write_error(" of length ");
    lymra_write_error_number(length, false);
    lymra_write_error("\n");
    lymra_write_error(site);
    exit(LYMRA_RUNTIME_ERROR_STATUS);
}

/* Stops the program because a slice would end at `end`, before its `start`. */
LYMRA_UNUSED LYMRA_NO_RETURN static void lymra_trap_slice_order(int64_t start, int64_t end, const char *site) {
    lymra_write_finished();
    lymra_write_error("error: slice end ");
    lymra_write_error_number((uint64_t)end, false);
    lymra_write_error(" is before its start ");
    lymra_write_error_number((uint64_t)start, false);
    lymra_write_error("\n");
    lymra_write_error(site);
    exit(LYMRA_RUNTIME_ERROR_STATUS);
}

/* Checks a signed start of a slice of `indexed`, of `length` elements: from 0 to `length`. */
LYMRA_UNUSED static inline int64_t lymra_slice_start_signed(int64_t start, uint64_t length, const char *indexed,
                                                            const char *site) {
    if (start < 0) {
        lymra_trap_slice("start", (uint64_t)0 - (uint64_t)start, true, length, indexed, site);
    }
    if ((uint64_t)start > length) {
        lymra_trap_slice("start", (uint64_t)start, false, length, indexed, site);
    }
    return start;
}

/* Checks an unsigned start of a slice of `indexed`, of `length` elements: at most `length`. */
LYMRA_UNUSED static inline int64_t lymra_slice_start_unsigned(uint64_t start, uint64_t length,
                                                              const char *indexed, const char *site) {
    if (start > length) {
        lymra_trap_slice("start", start, false, length, indexed, site);
    }
    return (int64_t)start;
}

/* Checks a signed end of a slice of `indexed`, of `length` elements, starting at `start`: up to `length`. */
LYMRA_UNUSED static inline int64_t lymra_slice_end_signed(int64_t end, int64_t start, uint64_t length,
                                                          const char *indexed, const char *site) {
    if (end < 0) {
        lymra_trap_slice("end", (uint64_t)0 - (uint64_t)end, true, length, indexed, site);
    }
    if ((uint64_t)end > length) {
        lymra_trap_slice("end", (uint64_t)end, false, length, indexed, site);
    }
    if (end < start) {
        lymra_trap_slice_order(start, end, site);
    }
    return end;
}

/* Checks an unsigned end of a slice of `indexed`, of `length` elements, starting at `start`: up to `length`. */
LYMRA_UNUSED static inline int64_t lymra_slice_end_unsigned(uint64_t end, int64_t start, uint64_t length,
                                                            const char *indexed, const char *site) {
    if (end > length) {
        lymra_trap_slice("end", end, false, length, indexed, site);
    }
    if ((int64_t)end < start) {
        lymra_trap_slice_order(start, (int64_t)end, site);
    }
    return (int64_t)end;
}

/*
 * Checks that a slice of `text` starts, or ends, between two characters at
 * `position`, which lies within the text: `part` says which. A UTF-8
 * character goes on with bytes of the form 10xxxxxx, so a position at one
 * of them falls inside a character.
 */
LYMRA_UNUSED static inline void lymra_text_boundary(lymra_string text, int64_t position, const char *part,
                                                    const char *site) {
    if ((size_t)position < text.length && ((unsigned char)text.data[position] & 0xC0) == 0x80) {
        lymra_write_finished();
        lymra_write_error("error: slice ");
        lymra_write_error(part);
        lymra_write_error(" ");
        lymra_write_error_number((uint64_t)position, false);
        lymra_write_error(" falls inside a character of the text\n");
        lymra_write_error(site);
        exit(LYMRA_RUNTIME_ERROR_STATUS);
    }
}

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

#if defined(LYMRA_USES_FLOATS)
/*
 * Checks a conversion from a float to an integer: the value has to lie
 * strictly between `below` and `above`, the nearest floats outside the range
 * whose values keep their whole part inside the target type. NaN lies
 * nowhere and fails too.
 */
LYMRA_UNUSED static inline double lymra_check_float(double value, double below, double above, const char *site) {
    if (!(value > below && value < above)) {
        lymra_trap(LYMRA_CONVERSION_OUT_OF_RANGE, site);
    }
    return value;
}

/*
 * Converts a `float64` to the nearest `float32`. A finite value at or above
 * the halfway point between the largest `float32` and 2^128 would become
 * infinity, so it stops the program; infinity and NaN stay what they are.
 */
LYMRA_UNUSED static inline float lymra_narrow_float(double value, const char *site) {
    /* A value minus itself is zero exactly when the value is finite; the math library is not needed. */
    bool finite = value - value == 0.0;
    if (finite && (value >= 0x1.ffffffp+127 || value <= -0x1.ffffffp+127)) {
        lymra_trap(LYMRA_CONVERSION_OUT_OF_RANGE, site);
    }
    return (float)value;
}
#endif

/*
 * Orders two texts by their UTF-8 bytes, which is the order of their
 * characters: negative when `left` comes first, zero when they are equal,
 * positive otherwise. A text that starts another comes first.
 */
LYMRA_UNUSED static inline int lymra_compare_text(lymra_string left, lymra_string right) {
    size_t shorter = left.length < right.length ? left.length : right.length;
    int order = shorter == 0 ? 0 : memcmp(left.data, right.data, shorter);
    if (order != 0) {
        return order;
    }
    return (left.length > right.length) - (left.length < right.length);
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
    "make sure every chain of calls reaches a case that returns. Every call in progress "
    "keeps its arrays on the stack too, so keep large arrays out of deep chains of calls\n";

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
 * The least stack the handler counts on, whatever RLIMIT_STACK says. QEMU's
 * user-mode emulation runs programs on at least 8 MiB of stack even under a
 * smaller limit, so an overflow there faults further down than the limit
 * suggests. Nothing else is mapped that close below the stack.
 */
#define LYMRA_LEAST_STACK_SIZE ((uintmax_t)8 << 20)
/*
 * The largest stack frame a function of the program can have. The code the
 * Lymra compiler generates defines it before this file, from the arrays its
 * functions hold.
 */
#ifndef LYMRA_LARGEST_FRAME
#define LYMRA_LARGEST_FRAME ((size_t)0)
#endif
/*
 * How far past the stack's limit a fault still counts as an overflow. Where
 * the C compiler does not probe a large frame a page at a time, a function
 * moves past the limit by up to its whole frame before it touches the stack,
 * so the window holds the largest frame and a mebibyte more for the frames of
 * the runtime and the C library. Nothing else is mapped that close below the
 * stack, and Lymra code never reads or writes memory it does not own, so a
 * fault there is the stack running out.
 */
#define LYMRA_STACK_SLACK (((size_t)1 << 20) + LYMRA_LARGEST_FRAME)

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
#if defined(__GNUC__) || defined(__clang__)
    /* The frame itself: AddressSanitizer can move a local such as `top` below off the stack, to catch use after return. */
    lymra_stack_top = (uintptr_t)__builtin_frame_address(0);
#else
    char top;
    lymra_stack_top = (uintptr_t)&top;
#endif
    /* `rlim_t` is unsigned on most systems and signed on some, such as FreeBSD; the limit itself is never negative. */
    uintmax_t size = (uintmax_t)limit.rlim_cur;
    if (size < LYMRA_LEAST_STACK_SIZE) {
        size = LYMRA_LEAST_STACK_SIZE;
    }
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

#if defined(LYMRA_DATA_STACKS) && LYMRA_DATA_STACKS > 0
/*
 * Data stacks (spec 7.2). Each is a range of address space reserved when the
 * program starts, with a top that moves up as a value grows and back down
 * when values are freed. Pages get memory behind them when the top first
 * reaches them, in steps that double, so reserved space costs nothing until
 * it is used. A value that would pass the end of its range stops the program
 * instead of touching anything beyond it.
 */
typedef struct {
    char *top;       /* The first free byte. */
    char *committed; /* The end of the part with memory behind it. */
    char *limit;     /* The end of the range. */
    char *base;      /* The start of the range. */
} lymra_data_stack;

/* The generated code defines how many data stacks the program uses: one for each list it builds. */
LYMRA_UNUSED static lymra_data_stack lymra_data_stacks[LYMRA_DATA_STACKS];

/* Memory is committed in steps of at least this many bytes, and each range is a multiple of it. */
#define LYMRA_COMMIT_STEP ((size_t)1 << 16)

/*
 * A build with AddressSanitizer marks the bytes above each top as unusable,
 * so a read of a freed value stops the program. GCC and MSVC say the
 * sanitizer is on with __SANITIZE_ADDRESS__, Clang with __has_feature. The
 * two functions are declared here rather than through their header, which
 * some installations leave out.
 */
#if defined(__SANITIZE_ADDRESS__)
#define LYMRA_ADDRESS_SANITIZER 1
#elif defined(__has_feature)
#if __has_feature(address_sanitizer)
#define LYMRA_ADDRESS_SANITIZER 1
#endif
#endif

#if defined(LYMRA_ADDRESS_SANITIZER)
void __asan_poison_memory_region(void const volatile *start, size_t bytes);
void __asan_unpoison_memory_region(void const volatile *start, size_t bytes);
#define LYMRA_MARK_FREE(start, bytes) __asan_poison_memory_region((start), (bytes))
#define LYMRA_MARK_USED(start, bytes) __asan_unpoison_memory_region((start), (bytes))
#else
#define LYMRA_MARK_FREE(start, bytes) ((void)(start), (void)(bytes))
#define LYMRA_MARK_USED(start, bytes) ((void)(start), (void)(bytes))
#endif

#if defined(LYMRA_WINDOWS)
LYMRA_UNUSED static char *lymra_reserve_range(size_t bytes) {
    return (char *)VirtualAlloc(NULL, bytes, MEM_RESERVE, PAGE_NOACCESS);
}

LYMRA_UNUSED static bool lymra_commit_range(char *start, size_t bytes) {
    return VirtualAlloc(start, bytes, MEM_COMMIT, PAGE_READWRITE) != NULL;
}
#elif !defined(LYMRA_POSIX)
#error "Lymra's lists need a system that can reserve address space: a POSIX system or Windows"
#else
#if defined(PROT_MPROTECT)
/*
 * NetBSD lets `mprotect` grant no more than `mmap` allowed, so there the
 * reservation allows the reading and writing that come later.
 */
#define LYMRA_RESERVED_PROTECTION (PROT_NONE | PROT_MPROTECT(PROT_READ | PROT_WRITE))
#else
#define LYMRA_RESERVED_PROTECTION PROT_NONE
#endif

LYMRA_UNUSED static char *lymra_reserve_range(size_t bytes) {
#if defined(MAP_ANONYMOUS)
    void *range = mmap(NULL, bytes, LYMRA_RESERVED_PROTECTION, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
#elif defined(MAP_ANON)
    void *range = mmap(NULL, bytes, LYMRA_RESERVED_PROTECTION, MAP_PRIVATE | MAP_ANON, -1, 0);
#else
    /* A system that shows neither name still maps zeroed pages from /dev/zero. */
    int zero = open("/dev/zero", O_RDWR);
    if (zero < 0) {
        return NULL;
    }
    void *range = mmap(NULL, bytes, LYMRA_RESERVED_PROTECTION, MAP_PRIVATE, zero, 0);
    close(zero);
#endif
    return range == MAP_FAILED ? NULL : (char *)range;
}

LYMRA_UNUSED static bool lymra_commit_range(char *start, size_t bytes) {
    return mprotect(start, bytes, PROT_READ | PROT_WRITE) == 0;
}
#endif

/*
 * Reserves one range for all the data stacks and splits it evenly. Each asks
 * for 64 GiB on a 64-bit system; a 32-bit one has 2 to 3 GiB in all, so
 * there they share 1 GiB. A system that refuses is asked for half as much,
 * down to one commit step each.
 */
LYMRA_UNUSED static void lymra_reserve_data_stacks(void) {
#if SIZE_MAX > 0xFFFFFFFFu
    size_t each = (size_t)1 << 36;
#else
    size_t each = ((size_t)1 << 30) / LYMRA_DATA_STACKS;
#endif
    if (each > SIZE_MAX / LYMRA_DATA_STACKS) {
        each = SIZE_MAX / LYMRA_DATA_STACKS;
    }
    each -= each % LYMRA_COMMIT_STEP;
    while (each >= LYMRA_COMMIT_STEP) {
        char *range = lymra_reserve_range(each * LYMRA_DATA_STACKS);
        if (range != NULL) {
            for (size_t index = 0; index < LYMRA_DATA_STACKS; index++) {
                lymra_data_stack *stack = &lymra_data_stacks[index];
                stack->base = range + index * each;
                stack->top = stack->base;
                stack->committed = stack->base;
                stack->limit = stack->base + each;
            }
            return;
        }
        each /= 2;
        each -= each % LYMRA_COMMIT_STEP;
    }
    lymra_write_error("error: the system has no address space for the program's data stacks\n");
    exit(LYMRA_RUNTIME_ERROR_STATUS);
}

/* Marks the bytes above the top of `stack` as free, in a build with AddressSanitizer. */
LYMRA_UNUSED static inline void lymra_data_stack_settle(lymra_data_stack *stack) {
    LYMRA_MARK_FREE(stack->top, (size_t)(stack->committed - stack->top));
}

/*
 * Commits memory for `needed` bytes above the top of `stack`: as much as the
 * stack has committed so far, so the steps double, or more if it needs more.
 * Stops the program when the range has no room left.
 */
LYMRA_UNUSED LYMRA_NO_INLINE static void lymra_data_stack_commit(lymra_data_stack *stack, size_t needed,
                                                                  const char *site) {
    if (needed > (size_t)(stack->limit - stack->top)) {
        lymra_trap(LYMRA_DATA_STACK_FULL, site);
    }
    size_t shortfall = needed - (size_t)(stack->committed - stack->top);
    size_t step = (size_t)(stack->committed - stack->base);
    if (step < shortfall) {
        step = shortfall;
    }
    if (step < LYMRA_COMMIT_STEP) {
        step = LYMRA_COMMIT_STEP;
    }
    step += (LYMRA_COMMIT_STEP - step % LYMRA_COMMIT_STEP) % LYMRA_COMMIT_STEP;
    if (step > (size_t)(stack->limit - stack->committed)) {
        step = (size_t)(stack->limit - stack->committed);
    }
    if (!lymra_commit_range(stack->committed, step)) {
        lymra_trap(LYMRA_OUT_OF_MEMORY, site);
    }
    LYMRA_MARK_FREE(stack->committed, step);
    stack->committed += step;
}

/*
 * Moves the top of `stack` up to the next multiple of `align`, where a new
 * value starts, and returns it. The committed part ends at a multiple of
 * every alignment, so the new top never passes it.
 */
LYMRA_UNUSED static inline char *lymra_data_stack_align(lymra_data_stack *stack, size_t align) {
    size_t misalignment = (size_t)((uintptr_t)stack->top % align);
    if (misalignment != 0) {
        stack->top += align - misalignment;
    }
    return stack->top;
}

/* Stops the program unless `end`, where a value ends, is the top of `stack`: only the newest value can grow. */
LYMRA_UNUSED static inline void lymra_data_stack_require_top(const lymra_data_stack *stack, const char *end,
                                                             const char *site) {
    if (end != stack->top) {
        lymra_trap(LYMRA_NOT_ON_TOP, site);
    }
}

/* Room for `size` more bytes at `end`, the end of the newest value on `stack`. */
LYMRA_UNUSED static inline char *lymra_data_stack_extend(lymra_data_stack *stack, char *end, size_t size,
                                                         const char *site) {
    lymra_data_stack_require_top(stack, end, site);
    if ((size_t)(stack->committed - end) < size) {
        lymra_data_stack_commit(stack, size, site);
    }
    LYMRA_MARK_USED(end, size);
    stack->top = end + size;
    return end;
}

/*
 * Copies `bytes` bytes from `source` to the aligned top of `stack`, which
 * they then end, and returns where they start. The source can lie anywhere,
 * on this stack too, below the copy or overlapping it.
 */
LYMRA_UNUSED static inline char *lymra_data_stack_place(lymra_data_stack *stack, const void *source,
                                                        size_t bytes, size_t align, const char *site) {
    char *start = lymra_data_stack_align(stack, align);
    if ((size_t)(stack->committed - start) < bytes) {
        lymra_data_stack_commit(stack, bytes, site);
    }
    LYMRA_MARK_USED(start, bytes);
    if (bytes != 0) {
        memmove(start, source, bytes);
    }
    stack->top = start + bytes;
    lymra_data_stack_settle(stack);
    return start;
}

/* Frees everything on `stack` from `start` up: the newest value there, which starts at `start`. */
LYMRA_UNUSED static inline void lymra_data_stack_release(lymra_data_stack *stack, char *start) {
    stack->top = start;
    lymra_data_stack_settle(stack);
}

/*
 * Frees as `lymra_data_stack_release` does, but a build with AddressSanitizer
 * keeps the freed bytes readable until the next `lymra_data_stack_settle`,
 * so a returning function can move its result out of what it frees.
 */
LYMRA_UNUSED static inline void lymra_data_stack_drop(lymra_data_stack *stack, char *start) {
    stack->top = start;
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
#if defined(LYMRA_DATA_STACKS) && LYMRA_DATA_STACKS > 0
    lymra_reserve_data_stacks();
#endif
}

/* Writes the waiting output when `main` returns, and reports if any of it could not be written. */
LYMRA_UNUSED static inline int lymra_finish(void) {
    lymra_flush();
    if (lymra_output_failed) {
        lymra_write_error("error: the output could not be written\n");
        return LYMRA_OUTPUT_ERROR_STATUS;
    }
#if defined(LYMRA_ADDRESS_SANITIZER) && defined(LYMRA_DATA_STACKS) && LYMRA_DATA_STACKS > 0
    /* Every list is freed by the time `main` returns. A build with AddressSanitizer checks that the code did so. */
    for (size_t index = 0; index < LYMRA_DATA_STACKS; index++) {
        if (lymra_data_stacks[index].top != lymra_data_stacks[index].base) {
            lymra_write_error("error: a list was never freed; this is a bug in the Lymra compiler\n");
            return LYMRA_RUNTIME_ERROR_STATUS;
        }
    }
#endif
    return 0;
}
