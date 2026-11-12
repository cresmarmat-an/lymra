//! Running the built programs and timing them.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How one executable did over all its runs.
pub struct Timings {
    pub output: String,
    pub median: Duration,
    pub slowest: Duration,
}

/**
 * Runs every executable once to warm up, then `runs` more times each, taking
 * turns so that a slow moment on the machine hits all of them alike. Fails if
 * a run fails, or prints something different from the warm-up run.
 */
pub fn measure(executables: &[PathBuf], runs: usize) -> Result<Vec<Timings>, String> {
    if runs == 0 {
        return Err("at least one timed run is needed".to_owned());
    }
    let outputs = executables
        .iter()
        .map(|executable| run_once(executable).map(|(output, _)| output))
        .collect::<Result<Vec<_>, _>>()?;
    let mut times = vec![Vec::with_capacity(runs); executables.len()];
    for _ in 0..runs {
        for ((executable, expected), times) in executables.iter().zip(&outputs).zip(&mut times) {
            let (output, elapsed) = run_once(executable)?;
            if &output != expected {
                return Err(format!(
                    "{} printed something different from one run to the next",
                    executable.display()
                ));
            }
            times.push(elapsed);
        }
    }
    Ok(outputs
        .into_iter()
        .zip(times)
        .map(|(output, mut times)| {
            times.sort_unstable();
            Timings {
                output,
                median: times[times.len() / 2],
                slowest: times[times.len() - 1],
            }
        })
        .collect())
}

/// Runs an executable once. Returns what it printed and how long it took.
fn run_once(executable: &Path) -> Result<(String, Duration), String> {
    let started = Instant::now();
    let result = Command::new(executable)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("cannot run {}: {error}", executable.display()))?;
    let elapsed = started.elapsed();
    if !result.status.success() {
        return Err(format!(
            "{} failed: {}",
            executable.display(),
            result.status
        ));
    }
    let output = String::from_utf8(result.stdout)
        .map_err(|_| format!("{} printed text that is not UTF-8", executable.display()))?;
    Ok((output, elapsed))
}
