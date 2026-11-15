# Benchmark results

Written by `cargo run --release -p lymra-bench`. Each number is the median wall-clock time, in seconds, of 5 runs after one warm-up run. The versions of a program take turns, so a slow moment on the machine hits them alike. Every version of a program printed the same output. A time under 10 ms is mostly the cost of starting a process, so a ratio of two such times is shown as n/a.

## Programs

| Program | What it does | Prints |
|---|---|---|
| `collatz` | Finds the start below one million with the longest Collatz chain. | `837799 525` |
| `fibonacci` | Recursive Fibonacci: mostly the cost of calls. | `102334155` |
| `gcd_sum` | Sums the greatest common divisor of every pair of numbers below 2000. | `19430528` |
| `modulo_sum` | A loop whose remainder keeps the C compiler from finding a formula. | `2699999991` |
| `primes` | Counts the primes below three million by trial division. | `216816` |
| `sum_formula` | A sum the C compiler can turn into a formula, when it may assume the arithmetic never overflows. | `75000008000000000` |

## Built with gcc

| Program | C | Lymra | Lymra with every check | Lymra / C |
|---|---:|---:|---:|---:|
| `collatz` | 0.281 | 0.315 | 0.297 | 1.12 |
| `fibonacci` | 0.191 | 0.005 | 0.004 | 0.03 |
| `gcd_sum` | 0.163 | 0.162 | 0.167 | 0.99 |
| `modulo_sum` | 0.312 | 0.314 | 0.360 | 1.01 |
| `primes` | 1.118 | 1.116 | 1.120 | 1.00 |
| `sum_formula` | 0.002 | 0.001 | 1.077 | n/a |

## Built with clang

| Program | C | Lymra | Lymra with every check | Lymra / C |
|---|---:|---:|---:|---:|
| `collatz` | 0.183 | 0.434 | 0.420 | 2.37 |
| `fibonacci` | 0.348 | 0.633 | 0.626 | 1.82 |
| `gcd_sum` | 0.132 | 0.131 | 0.134 | 0.99 |
| `modulo_sum` | 0.308 | 0.370 | 0.344 | 1.20 |
| `primes` | 0.680 | 0.684 | 0.687 | 1.01 |
| `sum_formula` | 0.001 | 0.001 | 0.739 | n/a |

## Against Rust

Rust and Clang both generate code with LLVM. Rust with overflow checks stops on overflow, as Lymra does.

| Program | Lymra (clang) | Rust with overflow checks | Rust | Lymra / Rust with overflow checks |
|---|---:|---:|---:|---:|
| `collatz` | 0.434 | 0.299 | 0.194 | 1.45 |
| `fibonacci` | 0.633 | 0.697 | 0.337 | 0.91 |
| `gcd_sum` | 0.131 | 0.128 | 0.135 | 1.02 |
| `modulo_sum` | 0.370 | 0.327 | 0.345 | 1.13 |
| `primes` | 0.684 | 0.694 | 0.687 | 0.99 |
| `sum_formula` | 0.001 | 0.749 | 0.002 | < 0.01 |

## Noise

Among versions whose median is at least 50 ms, the slowest timed run was 47% slower than that version's median.

## Machine

- Processor: Intel(R) Xeon(R) Processor @ 2.10GHz, 4 hardware threads
- System: linux x86_64
- `gcc`: gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0
- `clang`: Ubuntu clang version 18.1.3 (1ubuntu1)
- `rustc`: rustc 1.97.0 (2d8144b78 2026-07-07)
- C flags: `-std=c11 -O2`, the ones `lymra build` uses. Rust flags: `--edition 2024 -C opt-level=3`.
