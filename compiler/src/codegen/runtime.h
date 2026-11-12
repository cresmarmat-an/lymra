/*
 * The Lymra runtime for edition 0.1.
 *
 * The compiler puts this text at the top of every C file it generates, so a
 * generated file builds on its own. It needs GCC or Clang, for the builtins
 * that detect integer overflow.
 */

/*
 * On POSIX systems, output goes out through `write` and a stack overflow is
 * caught by a signal handler. The functions they need have to be asked for
 * before any header is included.
 */
#if defined(__unix__) || defined(__APPLE__)
#define LYMRA_POSIX 1
#define _XOPEN_SOURCE 700
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
#endif

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
 * Output waits in a buffer and goes out in large writes, which is much faster
 * than writing each piece. A line becomes part of the output only when its
 * line break is added, so a program that stops in the middle of a `print`
 * leaves no half line behind. Everything that ends the program writes the
 * finished lines first, the stack overflow handler included, so they are only
 * ever written with `write`, which a signal handler may call.
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
        size_t waiting = lymra_output_length - written;
#if defined(LYMRA_POSIX)
        ssize_t result = write(STDOUT_FILENO, lymra_output + written, waiting);
        if (result < 0 && errno == EINTR) {
            continue;
        }
        if (result <= 0) {
            lymra_output_failed = true;
            lymra_output_written = lymra_output_length;
            break;
        }
        lymra_output_written = written + (size_t)result;
#else
        if (fwrite(lymra_output + written, 1, waiting, stdout) != waiting || fflush(stdout) != 0) {
            lymra_output_failed = true;
        }
        lymra_output_written = lymra_output_length;
#endif
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
    __asm__ __volatile__("" ::: "memory");
    lymra_output_length = end;
}

/* Adds bytes to the line being printed. */
LYMRA_UNUSED static void lymra_append(const char *data, size_t length) {
    while (length > 0) {
        if (lymra_line_end == LYMRA_OUTPUT_CAPACITY) {
            if (lymra_output_length == 0) {
                /* One line fills the whole buffer. It goes out as it is. */
                lymra_commit(lymra_line_end);
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
LYMRA_UNUSED __attribute__((noreturn, cold, noinline)) static void lymra_trap(lymra_trap_kind kind,
                                                                                const char *site) {
    static const char *const messages[] = {
        "integer overflow",
        "division by zero",
        "assertion failed",
        "conversion out of range",
    };
    lymra_write_finished();
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
        static const char message[] =
            "error: stack overflow: calls nested too deeply\n"
            "help: a function kept calling itself, or functions kept calling one another; "
            "make sure every chain of calls reaches a case that returns\n";
        ssize_t written = write(STDERR_FILENO, message, sizeof message - 1);
        (void)written;
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
    size_t largest = SIZE_MAX - LYMRA_STACK_SLACK;
    lymra_stack_reach = (limit.rlim_cur < largest ? (size_t)limit.rlim_cur : largest) + LYMRA_STACK_SLACK;

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
#endif

/* Prepares the program to run. The C `main` calls it before anything else. */
LYMRA_UNUSED __attribute__((noinline)) static void lymra_start(void) {
#if defined(LYMRA_POSIX)
    lymra_catch_stack_overflow();
#endif
}

/* Writes the waiting output when `main` returns, and reports if any of it could not be written. */
LYMRA_UNUSED static inline int lymra_finish(void) {
    lymra_flush();
    if (lymra_output_failed) {
        fputs("error: the output could not be written\n", stderr);
        return LYMRA_OUTPUT_ERROR_STATUS;
    }
    return 0;
}
