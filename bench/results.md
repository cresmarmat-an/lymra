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
| `sieve` | Counts the primes below a hundred million with a sieve kept in a list that grows one element at a time. | `5761455` |
| `sum_formula` | A sum the C compiler can turn into a formula, when it may assume the arithmetic never overflows. | `75000008000000000` |

## Built with gcc

| Program | C | Lymra | Lymra with every check | Lymra / C |
|---|---:|---:|---:|---:|
| `collatz` | 0.239 | 0.256 | 0.262 | 1.07 |
| `fibonacci` | 0.187 | 0.004 | 0.003 | 0.02 |
| `gcd_sum` | 0.156 | 0.150 | 0.155 | 0.96 |
| `modulo_sum` | 0.287 | 0.001 | 0.345 | < 0.01 |
| `primes` | 1.005 | 1.016 | 0.990 | 1.01 |
| `sieve` | 0.660 | 0.664 | 0.668 | 1.01 |
| `sum_formula` | 0.001 | 0.001 | 0.938 | n/a |

## Built with clang

| Program | C | Lymra | Lymra with every check | Lymra / C |
|---|---:|---:|---:|---:|
| `collatz` | 0.161 | 0.386 | 0.383 | 2.41 |
| `fibonacci` | 0.337 | 0.490 | 0.494 | 1.46 |
| `gcd_sum` | 0.124 | 0.122 | 0.124 | 0.99 |
| `modulo_sum` | 0.288 | 0.001 | 0.333 | < 0.01 |
| `primes` | 0.612 | 0.610 | 0.617 | 1.00 |
| `sieve` | 0.712 | 0.716 | 0.703 | 1.01 |
| `sum_formula` | 0.001 | 0.001 | 0.645 | n/a |

## Against Rust

Rust and Clang both generate code with LLVM. Rust with overflow checks stops on overflow, as Lymra does.

| Program | Lymra (clang) | Rust with overflow checks | Rust | Lymra / Rust with overflow checks |
|---|---:|---:|---:|---:|
| `collatz` | 0.386 | 0.266 | 0.173 | 1.45 |
| `fibonacci` | 0.490 | 0.617 | 0.297 | 0.79 |
| `gcd_sum` | 0.122 | 0.133 | 0.129 | 0.92 |
| `modulo_sum` | 0.001 | 0.293 | 0.297 | < 0.01 |
| `primes` | 0.610 | 0.612 | 0.609 | 1.00 |
| `sieve` | 0.716 | 0.728 | 0.702 | 0.98 |
| `sum_formula` | 0.001 | 0.644 | 0.001 | < 0.01 |

## Noise

Among versions whose median is at least 50 ms, the slowest timed run was 31% slower than that version's median.

## Machine

- Processor: Intel(R) Xeon(R) Processor @ 2.10GHz, 4 hardware threads
- System: linux x86_64
- `gcc`: gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0
- `clang`: Ubuntu clang version 18.1.3 (1ubuntu1)
- `rustc`: rustc 1.97.0 (2d8144b78 2026-07-07)
- C flags: `-std=c11 -O2`, the ones `lymra build` uses. Rust flags: `--edition 2024 -C opt-level=3`.
