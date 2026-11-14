/*!
 * The runtime writes a float as the shortest decimal that reads back as the
 * same float, laid out the same way on every system. Each C compiler builds
 * a harness that writes floats through the runtime's own output: every
 * exponent with the significands where rounding changes, the smallest
 * subnormals, and a long run of random bit patterns, NaN and infinities among
 * them. Rust's own shortest
 * digits must agree, except where two decimals are equally close, which the
 * runtime settles toward the even last digit and Rust does not.
 */

mod common;

use std::fs;
use std::process::Command;

use lymra::driver::{TemporaryDirectory, Warnings};

/// Random values per type. Enough to reach every exponent many times, few enough for an emulated processor.
const RANDOM_VALUES: u32 = 100_000;

/// Writes `d <bits> <text>` for `float64` values and `f <bits> <text>` for `float32` ones.
const HARNESS: &str = r#"
static uint64_t random_state = UINT64_C(0x9E3779B97F4A7C15);

static uint64_t next_random(void) {
    random_state ^= random_state << 13;
    random_state ^= random_state >> 7;
    random_state ^= random_state << 17;
    return random_state;
}

static void line_64(uint64_t bits) {
    double value;
    memcpy(&value, &bits, sizeof value);
    lymra_append("d ", 2);
    lymra_write_unsigned(bits);
    lymra_append(" ", 1);
    lymra_write_float64(value);
    lymra_end_line();
}

static void line_32(uint32_t bits) {
    float value;
    memcpy(&value, &bits, sizeof value);
    lymra_append("f ", 2);
    lymra_write_unsigned(bits);
    lymra_append(" ", 1);
    lymra_write_float32(value);
    lymra_end_line();
}

int main(void) {
    lymra_start();
    for (uint64_t exponent = 0; exponent < 0x800; exponent++) {
        const uint64_t fractions[] = {0, 1, 2, 3, 9, UINT64_C(1) << 51, (UINT64_C(1) << 52) - 1};
        for (size_t index = 0; index < sizeof fractions / sizeof fractions[0]; index++) {
            line_64((exponent << 52) | fractions[index]);
            line_64((UINT64_C(1) << 63) | (exponent << 52) | fractions[index]);
        }
    }
    for (uint32_t exponent = 0; exponent < 0x100; exponent++) {
        const uint32_t fractions[] = {0, 1, 2, 3, 9, UINT32_C(1) << 22, (UINT32_C(1) << 23) - 1};
        for (size_t index = 0; index < sizeof fractions / sizeof fractions[0]; index++) {
            line_32((exponent << 23) | fractions[index]);
            line_32(UINT32_C(0x80000000) | (exponent << 23) | fractions[index]);
        }
    }
    /* The smallest subnormals have the fewest digits, where the shortest decimal is hardest to find. */
    for (uint32_t fraction = 0; fraction < 1000; fraction++) {
        line_64(fraction);
        line_32(fraction);
    }
    for (unsigned long count = 0; count < RANDOM_VALUES; count++) {
        line_64(next_random());
        line_32((uint32_t)next_random());
    }
    return lymra_finish();
}
"#;

/// A decimal as `digits` times ten to the `exponent`, with no trailing zeros in `digits`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Decimal {
    digits: u128,
    exponent: i32,
}

impl Decimal {
    fn trimmed(mut self) -> Self {
        while self.digits != 0 && self.digits % 10 == 0 {
            self.digits /= 10;
            self.exponent += 1;
        }
        self
    }

    /// Rust's shortest digits, from the `d.ddde-x` form of `{:e}`.
    fn from_scientific(text: &str) -> Self {
        let (mantissa, exponent) = text.split_once('e').expect("an exponent");
        let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
        let exponent: i32 = exponent.parse().expect("an exponent number");
        Decimal {
            digits: digits.parse().expect("digits"),
            exponent: exponent - (digits.len() as i32 - 1),
        }
        .trimmed()
    }

    /// The decimal a Lymra float literal stands for, as `2.5`, `100.0` or `1.0e-7`.
    fn from_lymra(text: &str) -> Self {
        let (mantissa, exponent) = text.split_once('e').unwrap_or((text, "0"));
        let (whole, fraction) = mantissa.split_once('.').expect("a point");
        let exponent: i32 = exponent.parse().expect("an exponent number");
        Decimal {
            digits: format!("{whole}{fraction}").parse().expect("digits"),
            exponent: exponent - fraction.len() as i32,
        }
        .trimmed()
    }

    /// Both at the smaller exponent, so their digits line up.
    fn aligned(&self, other: &Decimal) -> (u128, u128) {
        let exponent = self.exponent.min(other.exponent);
        let scale =
            |decimal: &Decimal| decimal.digits * 10_u128.pow((decimal.exponent - exponent) as u32);
        (scale(self), scale(other))
    }
}

/// How Lymra writes a float whose magnitude is the decimal: plain from 0.0001 up to below 1e16, with an exponent otherwise.
fn lymra_text(decimal: &Decimal, negative: bool) -> String {
    let digits = decimal.digits.to_string();
    let count = digits.len() as i32;
    let point = count + decimal.exponent;
    let sign = if negative { "-" } else { "" };
    if (-4..16).contains(&(point - 1)) {
        if point <= 0 {
            return format!("{sign}0.{}{digits}", "0".repeat((-point) as usize));
        }
        if point >= count {
            return format!("{sign}{digits}{}.0", "0".repeat((point - count) as usize));
        }
        let (whole, fraction) = digits.split_at(point as usize);
        return format!("{sign}{whole}.{fraction}");
    }
    let (first, rest) = digits.split_at(1);
    let rest = if rest.is_empty() { "0" } else { rest };
    format!("{sign}{first}.{rest}e{}", point - 1)
}

/// What Lymra writes for a float that is NaN, infinite or zero, or `None` for any other.
fn special_text(value: f64) -> Option<String> {
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value.is_nan() {
        Some("NaN".to_owned())
    } else if value.is_infinite() {
        Some(format!("{sign}infinity"))
    } else if value == 0.0 {
        Some(format!("{sign}0.0"))
    } else {
        None
    }
}

/**
 * Checks one line of the harness. A different decimal is accepted only as
 * the other of two equally close ones: as long as Rust's, next to it, and
 * ending in an even digit.
 */
fn check_line(line: &str) -> Result<(), String> {
    let mut parts = line.splitn(3, ' ');
    let (Some(kind), Some(bits), Some(written)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!("a malformed line: {line}"));
    };
    let bits: u64 = bits
        .parse()
        .map_err(|_| format!("malformed bits: {line}"))?;
    let (value, scientific) = match kind {
        "d" => {
            let value = f64::from_bits(bits);
            (value, format!("{:e}", value.abs()))
        }
        "f" => {
            let value = f32::from_bits(bits as u32);
            (f64::from(value), format!("{:e}", value.abs()))
        }
        _ => return Err(format!("an unknown kind: {line}")),
    };
    if let Some(expected) = special_text(value) {
        return if written == expected {
            Ok(())
        } else {
            Err(format!("{line}: expected {expected}"))
        };
    }
    let theirs = Decimal::from_scientific(&scientific);
    let expected = lymra_text(&theirs, value.is_sign_negative());
    if written == expected {
        return Ok(());
    }
    let ours = Decimal::from_lymra(written.trim_start_matches('-'));
    let (ours_aligned, theirs_aligned) = ours.aligned(&theirs);
    let is_even_tie = ours_aligned.abs_diff(theirs_aligned) == 1
        && ours_aligned % 2 == 0
        && ours_aligned.to_string().len() == theirs_aligned.to_string().len()
        && lymra_text(&ours, value.is_sign_negative()) == written;
    if is_even_tie {
        Ok(())
    } else {
        Err(format!("{line}: expected {expected}"))
    }
}

#[test]
fn floats_are_written_as_their_shortest_decimal() {
    let root = common::repository_root().join("compiler/src/codegen");
    let runtime = common::read(&root.join("runtime.h"));
    let floats = common::read(&root.join("runtime_floats.h"));
    let harness = HARNESS.replace("RANDOM_VALUES", &RANDOM_VALUES.to_string());
    let c_source = format!("#define LYMRA_USES_FLOATS 1\n{runtime}\n{floats}\n{harness}");
    let mut runs = 0;
    for compiler in common::c_compilers() {
        let workspace = TemporaryDirectory::new().expect("a temporary directory can be made");
        let executable = workspace
            .path()
            .join(format!("floats{}", std::env::consts::EXE_SUFFIX));
        compiler
            .compile(&c_source, &executable, None, Warnings::Strict)
            .unwrap_or_else(|error| panic!("{} rejects the harness:\n{error}", compiler.name()));
        let output = Command::new(&executable)
            .output()
            .expect("the harness runs");
        assert!(
            output.status.success(),
            "{} harness failed",
            compiler.name()
        );
        let text = String::from_utf8_lossy(&output.stdout);
        let wrong: Vec<String> = text
            .lines()
            .filter_map(|line| check_line(line).err())
            .take(20)
            .collect();
        assert!(
            wrong.is_empty(),
            "{} writes floats wrong:\n{}",
            compiler.name(),
            wrong.join("\n")
        );
        let lines = text.lines().count();
        assert!(
            lines > 2 * RANDOM_VALUES as usize,
            "{} wrote only {lines} lines",
            compiler.name()
        );
        fs::remove_file(&executable).ok();
        runs += 1;
    }
    assert!(runs > 0, "no C compiler built the harness");
}

#[test]
fn the_layout_matches_the_spec() {
    let text = |value: f64| {
        lymra_text(
            &Decimal::from_scientific(&format!("{:e}", value.abs())),
            value < 0.0,
        )
    };
    assert_eq!(text(0.1), "0.1");
    assert_eq!(text(100.0), "100.0");
    assert_eq!(text(-2.5), "-2.5");
    assert_eq!(text(0.0001), "0.0001");
    assert_eq!(text(0.00001), "1.0e-5");
    assert_eq!(text(1.0e15), "1000000000000000.0");
    assert_eq!(text(1.0e16), "1.0e16");
    assert_eq!(text(1.5e-7), "1.5e-7");
    assert_eq!(text(5e-324), "5.0e-324");
    assert_eq!(text(f64::MAX), "1.7976931348623157e308");
}
