/*!
 * The runtime's overflow checks agree with exact arithmetic in both of their
 * forms: the builtins GCC and Clang provide, and the portable code that
 * other compilers, such as MSVC, use. A check that is wrong in either form
 * would let a program continue with a wrong number, so every pair of 8-bit
 * values is tried, and for wider types every pair of values near zero, the
 * limits, each power of two and the square root of the limit.
 */

mod common;

use std::fs;
use std::process::Command;

use lymra::driver::{TemporaryDirectory, Warnings};

/// Selects the portable checks even where the builtins exist.
const PORTABLE: &str = "#define LYMRA_PORTABLE_ARITHMETIC 1\n";

/// Checks every overflow predicate of the runtime that comes before it, and prints `ok` if all agree.
const HARNESS: &str = r#"
static unsigned long checks;
static unsigned long failures;

static void expect(const char *operation, bool overflowed, bool outside, bool same_value, long long left,
                   long long right) {
    checks++;
    if (overflowed != outside || (!overflowed && !same_value)) {
        if (failures < 10) {
            printf("wrong: %s of %lld and %lld\n", operation, left, right);
        }
        failures++;
    }
}

static size_t keep(long long *values, size_t count, long long value, long long minimum, long long maximum) {
    if (count < 2048 && value >= minimum && value <= maximum) {
        values[count++] = value;
    }
    return count;
}

/* The largest value whose square is at most `maximum`, found by halving the range rather than counting up to it. */
static long long square_root(long long maximum) {
    long long low = 1;
    long long high = maximum < 3037000499LL ? maximum : 3037000499LL;
    while (low < high) {
        long long middle = low + (high - low + 1) / 2;
        if (middle <= maximum / middle) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    return low;
}

/* Every value of a type up to 8 bits, and the values where arithmetic on a wider type changes behavior. */
static size_t interesting(long long minimum, long long maximum, long long *values) {
    size_t count = 0;
    if (maximum <= 255) {
        for (long long value = minimum; value <= maximum; value++) {
            count = keep(values, count, value, minimum, maximum);
        }
        return count;
    }
    long long root = square_root(maximum);
    for (long long step = 0; step <= 40; step++) {
        count = keep(values, count, step, minimum, maximum);
        count = keep(values, count, -step, minimum, maximum);
        count = keep(values, count, maximum - step, minimum, maximum);
        count = keep(values, count, minimum + step, minimum, maximum);
        count = keep(values, count, maximum / 2 - 20 + step, minimum, maximum);
        count = keep(values, count, root - 20 + step, minimum, maximum);
        count = keep(values, count, 20 - root - step, minimum, maximum);
    }
    for (int bit = 1; bit < 63; bit++) {
        long long power = 1LL << bit;
        if (power > maximum) {
            break;
        }
        for (long long step = -2; step <= 2; step++) {
            count = keep(values, count, power + step, minimum, maximum);
            count = keep(values, count, -power + step, minimum, maximum);
        }
    }
    return count;
}

/* Exact results of narrow types fit in `long long`; 64-bit types need 128 bits, where the compiler has them. */
#define CHECK_SIGNED(name, type, exact, minimum, maximum)                                                     \
    static void check_##name(void) {                                                                          \
        static long long values[2048];                                                                        \
        size_t count = interesting(minimum, maximum, values);                                                 \
        for (size_t i = 0; i < count; i++) {                                                                  \
            for (size_t j = 0; j < count; j++) {                                                              \
                type left = (type)values[i];                                                                  \
                type right = (type)values[j];                                                                 \
                type result = 0;                                                                              \
                exact sum = (exact)left + right;                                                              \
                exact difference = (exact)left - right;                                                       \
                exact product = (exact)left * right;                                                          \
                bool overflowed = lymra_add_overflows_##name(left, right, &result);                           \
                expect(#name " add", overflowed, sum < (minimum) || sum > (maximum), result == sum, values[i], \
                       values[j]);                                                                            \
                result = 0;                                                                                   \
                overflowed = lymra_subtract_overflows_##name(left, right, &result);                           \
                expect(#name " subtract", overflowed, difference < (minimum) || difference > (maximum),        \
                       result == difference, values[i], values[j]);                                           \
                result = 0;                                                                                   \
                overflowed = lymra_multiply_overflows_##name(left, right, &result);                           \
                expect(#name " multiply", overflowed, product < (minimum) || product > (maximum),              \
                       result == product, values[i], values[j]);                                              \
            }                                                                                                 \
        }                                                                                                     \
    }

/* Unsigned values are drawn from the signed range of the same size, then shifted up into the unsigned one. */
#define CHECK_UNSIGNED(name, type, exact, maximum, half)                                                      \
    static void check_##name(void) {                                                                          \
        static long long values[2048];                                                                        \
        size_t count = interesting(-(half) - 1, half, values);                                                \
        for (size_t i = 0; i < count; i++) {                                                                  \
            for (size_t j = 0; j < count; j++) {                                                              \
                type left = (type)((type)values[i] + (type)(half) + 1);                                       \
                type right = (type)((type)values[j] + (type)(half) + 1);                                      \
                type result = 0;                                                                              \
                exact sum = (exact)left + right;                                                              \
                exact product = (exact)left * right;                                                          \
                bool overflowed = lymra_add_overflows_##name(left, right, &result);                           \
                expect(#name " add", overflowed, sum > (exact)(maximum), result == sum, (long long)left,       \
                       (long long)right);                                                                     \
                result = 0;                                                                                   \
                overflowed = lymra_subtract_overflows_##name(left, right, &result);                           \
                expect(#name " subtract", overflowed, left < right, result == (type)(left - right),            \
                       (long long)left, (long long)right);                                                    \
                result = 0;                                                                                   \
                overflowed = lymra_multiply_overflows_##name(left, right, &result);                           \
                expect(#name " multiply", overflowed, product > (exact)(maximum), result == product,           \
                       (long long)left, (long long)right);                                                    \
            }                                                                                                 \
        }                                                                                                     \
    }

CHECK_SIGNED(integer8, int8_t, long long, INT8_MIN, INT8_MAX)
CHECK_SIGNED(integer16, int16_t, long long, INT16_MIN, INT16_MAX)
CHECK_SIGNED(integer32, int32_t, long long, INT32_MIN, INT32_MAX)
CHECK_UNSIGNED(unsigned8, uint8_t, unsigned long long, UINT8_MAX, INT8_MAX)
CHECK_UNSIGNED(unsigned16, uint16_t, unsigned long long, UINT16_MAX, INT16_MAX)
CHECK_UNSIGNED(unsigned32, uint32_t, unsigned long long, UINT32_MAX, INT32_MAX)
#if defined(__SIZEOF_INT128__)
/* `__extension__` tells strict ISO C mode that the 128-bit types are meant. */
__extension__ typedef __int128 exact_signed_128;
__extension__ typedef unsigned __int128 exact_unsigned_128;
CHECK_SIGNED(integer64, int64_t, exact_signed_128, INT64_MIN, INT64_MAX)
CHECK_UNSIGNED(unsigned64, uint64_t, exact_unsigned_128, UINT64_MAX, INT64_MAX)
#endif

int main(void) {
    check_integer8();
    check_integer16();
    check_integer32();
    check_unsigned8();
    check_unsigned16();
    check_unsigned32();
#if defined(__SIZEOF_INT128__)
    check_integer64();
    check_unsigned64();
    printf("64-bit types checked\n");
#endif
    if (failures > 0) {
        printf("%lu of %lu checks failed\n", failures, checks);
        return 1;
    }
    printf("ok: %lu checks\n", checks);
    return 0;
}
"#;

#[test]
fn overflow_checks_agree_with_exact_arithmetic() {
    let runtime = common::read(&common::repository_root().join("compiler/src/codegen/runtime.h"));
    let mut runs = 0;
    for compiler in common::c_compilers() {
        for (form, prefix) in [("builtin", ""), ("portable", PORTABLE)] {
            let c_source = format!("{prefix}{runtime}{HARNESS}");
            let workspace = TemporaryDirectory::new().expect("a temporary directory can be made");
            let executable = workspace
                .path()
                .join(format!("arithmetic{}", std::env::consts::EXE_SUFFIX));
            compiler
                .compile(&c_source, &executable, None, Warnings::Strict)
                .unwrap_or_else(|error| {
                    panic!(
                        "{} ({form} checks) rejects the harness:\n{error}",
                        compiler.name()
                    )
                });
            let output = Command::new(&executable)
                .output()
                .expect("the harness runs");
            let report = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && report.contains("ok: "),
                "{} ({form} checks):\n{report}",
                compiler.name()
            );
            fs::remove_file(&executable).ok();
            runs += 1;
        }
    }
    assert!(runs > 0, "no C compiler built the harness");
}
