//! The results page: what was measured, on what, and the numbers.

use std::fmt::Write as _;
use std::fs;
use std::process::Command;
use std::time::Duration;

use crate::timing::Timings;
use crate::variant::{C_FLAGS, RUST_FLAGS, Variant};

/// Below this, a time is mostly the cost of starting a process, so ratios of it mean little.
const START_UP: Duration = Duration::from_millis(10);
/// Versions faster than this are left out of the noise figure, where start-up would dominate.
const NOISE_FLOOR: Duration = Duration::from_millis(50);

/// One program: what it does, what it printed, and the timings of each variant, in the order of the variants.
pub struct ProgramResult {
    pub name: String,
    pub description: String,
    pub output: String,
    pub timings: Vec<Timings>,
}

impl ProgramResult {
    fn median(&self, variants: &[Variant], wanted: Variant) -> Option<Duration> {
        let index = variants.iter().position(|&variant| variant == wanted)?;
        self.timings.get(index).map(|timings| timings.median)
    }
}

pub fn render(results: &[ProgramResult], variants: &[Variant], runs: usize) -> String {
    let mut page = String::from("# Benchmark results\n\n");
    let _ = writeln!(
        page,
        "Written by `cargo run --release -p lymra-bench`. Each number is the median wall-clock time, in seconds, \
         of {runs} {} after one warm-up run. The versions of a program take turns, so a slow moment on the \
         machine hits them alike. Every version of a program printed the same output. A time under {} ms is \
         mostly the cost of starting a process, so a ratio of two such times is shown as n/a.\n",
        if runs == 1 { "run" } else { "runs" },
        START_UP.as_millis()
    );
    programs(&mut page, results);
    for variant in variants {
        if let Variant::C { compiler } = *variant {
            compiler_table(&mut page, results, variants, compiler);
        }
    }
    if variants.contains(&Variant::Rust) {
        rust_table(&mut page, results, variants);
    }
    noise(&mut page, results);
    machine(&mut page, variants);
    page
}

fn seconds(duration: Option<Duration>) -> String {
    duration.map_or_else(
        || "n/a".to_owned(),
        |duration| format!("{:.3}", duration.as_secs_f64()),
    )
}

/// How many times longer `numerator` took. Two times that are both mostly start-up are not compared.
fn ratio(numerator: Option<Duration>, denominator: Option<Duration>) -> String {
    let (Some(numerator), Some(denominator)) = (numerator, denominator) else {
        return "n/a".to_owned();
    };
    if numerator < START_UP && denominator < START_UP {
        return "n/a".to_owned();
    }
    let ratio = numerator.as_secs_f64() / denominator.as_secs_f64().max(f64::MIN_POSITIVE);
    if ratio < 0.01 {
        "< 0.01".to_owned()
    } else if ratio > 100.0 {
        "> 100".to_owned()
    } else {
        format!("{ratio:.2}")
    }
}

fn programs(page: &mut String, results: &[ProgramResult]) {
    page.push_str("## Programs\n\n| Program | What it does | Prints |\n|---|---|---|\n");
    for result in results {
        let _ = writeln!(
            page,
            "| `{}` | {} | `{}` |",
            result.name, result.description, result.output
        );
    }
    page.push('\n');
}

/// C, Lymra and Lymra with every check, all built by one C compiler.
fn compiler_table(
    page: &mut String,
    results: &[ProgramResult],
    variants: &[Variant],
    compiler: &'static str,
) {
    let _ = writeln!(
        page,
        "## Built with {compiler}\n\n\
         | Program | C | Lymra | Lymra with every check | Lymra / C |\n\
         |---|---:|---:|---:|---:|"
    );
    for result in results {
        let c = result.median(variants, Variant::C { compiler });
        let lymra = result.median(variants, Variant::Lymra { compiler });
        let every_check = result.median(variants, Variant::LymraEveryCheck { compiler });
        let _ = writeln!(
            page,
            "| `{}` | {} | {} | {} | {} |",
            result.name,
            seconds(c),
            seconds(lymra),
            seconds(every_check),
            ratio(lymra, c)
        );
    }
    page.push('\n');
}

/// Rust with and without overflow checks, next to Lymra built by Clang, which uses the same code generator as Rust.
fn rust_table(page: &mut String, results: &[ProgramResult], variants: &[Variant]) {
    let lymra = Variant::Lymra { compiler: "clang" };
    page.push_str(
        "## Against Rust\n\n\
         Rust and Clang both generate code with LLVM. Rust with overflow checks stops on overflow, as Lymra does.\n\n\
         | Program | Lymra (clang) | Rust with overflow checks | Rust | Lymra / Rust with overflow checks |\n\
         |---|---:|---:|---:|---:|\n",
    );
    for result in results {
        let lymra = result.median(variants, lymra);
        let checked = result.median(variants, Variant::RustChecked);
        let _ = writeln!(
            page,
            "| `{}` | {} | {} | {} | {} |",
            result.name,
            seconds(lymra),
            seconds(checked),
            seconds(result.median(variants, Variant::Rust)),
            ratio(lymra, checked)
        );
    }
    page.push('\n');
}

/// How far the slowest run strayed from its median, as a measure of how noisy the machine was.
fn noise(page: &mut String, results: &[ProgramResult]) {
    let worst = results
        .iter()
        .flat_map(|result| &result.timings)
        .filter(|timings| timings.median >= NOISE_FLOOR)
        .map(|timings| timings.slowest.as_secs_f64() / timings.median.as_secs_f64() - 1.0)
        .fold(0.0, f64::max);
    let _ = writeln!(
        page,
        "## Noise\n\nAmong versions whose median is at least {} ms, the slowest timed run was {:.0}% slower \
         than that version's median.\n",
        NOISE_FLOOR.as_millis(),
        worst * 100.0
    );
}

fn machine(page: &mut String, variants: &[Variant]) {
    let processor = fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|info| {
            info.lines()
                .find_map(|line| line.strip_prefix("model name"))
                .and_then(|line| line.split_once(':'))
                .map(|(_, name)| name.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let threads = std::thread::available_parallelism().map_or(0, usize::from);
    let _ = writeln!(
        page,
        "## Machine\n\n\
         - Processor: {processor}, {threads} hardware threads\n\
         - System: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let mut tools: Vec<&str> = variants
        .iter()
        .filter_map(|variant| match variant {
            Variant::C { compiler } => Some(*compiler),
            _ => None,
        })
        .collect();
    if variants.contains(&Variant::Rust) {
        tools.push("rustc");
    }
    for tool in tools {
        let _ = writeln!(page, "- `{tool}`: {}", version(tool));
    }
    let _ = writeln!(
        page,
        "- C flags: `{}`, the ones `lymra build` uses. Rust flags: `{}`.",
        C_FLAGS.join(" "),
        RUST_FLAGS.join(" ")
    );
}

/// The first line a tool prints for `--version`.
fn version(tool: &str) -> String {
    Command::new(tool)
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_owned())
}
