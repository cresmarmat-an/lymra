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
| `collatz` | 0.218 | 0.264 | 0.273 | 1.21 |
| `fibonacci` | 0.158 | 0.047 | 0.463 | 0.30 |
| `gcd_sum` | 0.145 | 0.149 | 0.149 | 1.03 |
| `modulo_sum` | 0.272 | 0.273 | 0.297 | 1.00 |
| `primes` | 0.948 | 1.008 | 1.001 | 1.06 |
| `sum_formula` | 0.001 | 0.001 | 0.626 | n/a |

## Built with clang

| Program | C | Lymra | Lymra with every check | Lymra / C |
|---|---:|---:|---:|---:|
| `collatz` | 0.158 | 0.256 | 0.249 | 1.62 |
| `fibonacci` | 0.262 | 0.443 | 0.486 | 1.69 |
| `gcd_sum` | 0.119 | 0.112 | 0.118 | 0.94 |
| `modulo_sum` | 0.285 | 0.286 | 0.322 | 1.01 |
| `primes` | 0.553 | 0.586 | 0.561 | 1.06 |
| `sum_formula` | 0.001 | 0.001 | 0.624 | n/a |

## Against Rust

Rust and Clang both generate code with LLVM. Rust with overflow checks stops on overflow, as Lymra does.

| Program | Lymra (clang) | Rust with overflow checks | Rust | Lymra / Rust with overflow checks |
|---|---:|---:|---:|---:|
| `collatz` | 0.256 | 0.268 | 0.174 | 0.95 |
| `fibonacci` | 0.443 | 0.547 | 0.284 | 0.81 |
| `gcd_sum` | 0.112 | 0.103 | 0.117 | 1.08 |
| `modulo_sum` | 0.286 | 0.260 | 0.218 | 1.10 |
| `primes` | 0.586 | 0.605 | 0.584 | 0.97 |
| `sum_formula` | 0.001 | 0.622 | 0.002 | < 0.01 |

## Noise

Among versions whose median is at least 50 ms, the slowest timed run was 23% slower than that version's median.

## Machine

- Processor: Intel(R) Xeon(R) Processor @ 2.10GHz, 4 hardware threads
- System: linux x86_64
- `gcc`: gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0
- `clang`: Ubuntu clang version 18.1.3 (1ubuntu1)
- `rustc`: rustc 1.97.0 (2d8144b78 2026-07-07)
- C flags: `-std=c11 -O2`, the ones `lymra build` uses. Rust flags: `--edition 2024 -C opt-level=3`.
