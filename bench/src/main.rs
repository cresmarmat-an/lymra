/*!
 * Times Lymra programs against the same programs in C and Rust, and measures
 * what Lymra's runtime checks cost with and without the optimizer.
 *
 *     cargo run --release -p lymra-bench -- [--runs N] [program ...]
 *
 * Each directory in `bench/programs` holds one program three times:
 * `program.lym`, `program.c` and `program.rs`. Every version is built, run,
 * and timed, and every version has to print the same output. The results go
 * into `bench/results.md`.
 */

mod report;
mod timing;
mod variant;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

use lymra::driver::TemporaryDirectory;

use report::ProgramResult;
use variant::Variant;

const DEFAULT_RUNS: usize = 5;

/// What the command line asked for.
struct Options {
    runs: usize,
    /// Programs to run, by directory name. Empty means every program.
    programs: Vec<String>,
}

fn parse_options(mut arguments: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        runs: DEFAULT_RUNS,
        programs: Vec::new(),
    };
    while let Some(argument) = arguments.next() {
        if argument == "--runs" {
            let value = arguments.next().ok_or("`--runs` needs a number")?;
            options.runs = value.parse().ok().filter(|&runs| runs > 0).ok_or_else(|| {
                format!("`--runs` needs a whole number above zero, not `{value}`")
            })?;
        } else if argument.starts_with('-') {
            return Err(format!(
                "unknown option `{argument}`; the options are `--runs N` and program names"
            ));
        } else {
            options.programs.push(argument);
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let options = parse_options(env::args().skip(1))?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directories = program_directories(&root.join("programs"), &options.programs)?;
    let variants = variant::available();
    let workspace = TemporaryDirectory::new().map_err(|error| error.to_string())?;
    let results = directories
        .iter()
        .map(|directory| measure_program(directory, &variants, options.runs, workspace.path()))
        .collect::<Result<Vec<_>, _>>()?;
    let path = root.join("results.md");
    fs::write(&path, report::render(&results, &variants, options.runs))
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn directory_name(directory: &Path) -> String {
    directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The program directories to run, sorted by name: every one, or the ones named.
fn program_directories(programs: &Path, names: &[String]) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(programs)
        .map_err(|error| format!("cannot read {}: {error}", programs.display()))?;
    let mut directories = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("cannot read {}: {error}", programs.display()))?
            .path();
        if path.is_dir() {
            directories.push(path);
        }
    }
    directories.sort();
    if let Some(unknown) = names.iter().find(|name| {
        !directories
            .iter()
            .any(|directory| directory_name(directory) == **name)
    }) {
        return Err(format!(
            "no program named `{unknown}` in {}",
            programs.display()
        ));
    }
    if !names.is_empty() {
        directories.retain(|directory| names.contains(&directory_name(directory)));
    }
    Ok(directories)
}

/// The text of the block comment at the top of a Lymra file, on one line.
fn description(lymra_source: &str) -> String {
    lymra_source
        .split_once("#[")
        .and_then(|(_, rest)| rest.split_once("]#"))
        .map(|(comment, _)| comment.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

/// Builds every variant of one program, times them, and makes sure they all print the same.
fn measure_program(
    directory: &Path,
    variants: &[Variant],
    runs: usize,
    workspace: &Path,
) -> Result<ProgramResult, String> {
    let name = directory_name(directory);
    eprintln!("{name}: building {} versions", variants.len());
    let mut executables = Vec::with_capacity(variants.len());
    for (index, variant) in variants.iter().enumerate() {
        let executable = workspace.join(format!("{name}-{index}"));
        variant
            .build(directory, &executable)
            .map_err(|error| format!("{name}, {variant}: {error}"))?;
        executables.push(executable);
    }
    eprintln!("{name}: timing {runs} runs of each");
    let timings =
        timing::measure(&executables, runs).map_err(|error| format!("{name}: {error}"))?;
    let (Some(first), Some(first_variant)) = (timings.first(), variants.first()) else {
        return Err("no compiler is installed, so nothing can be built".to_owned());
    };
    for (variant, timing) in variants.iter().zip(&timings) {
        if timing.output != first.output {
            return Err(format!(
                "{name}: {variant} printed {:?}, but {first_variant} printed {:?}",
                timing.output, first.output
            ));
        }
    }
    let lymra_path = directory.join("program.lym");
    let lymra_source = fs::read_to_string(&lymra_path)
        .map_err(|error| format!("cannot read {}: {error}", lymra_path.display()))?;
    Ok(ProgramResult {
        name,
        description: description(&lymra_source),
        output: first.output.trim().to_owned(),
        timings,
    })
}
