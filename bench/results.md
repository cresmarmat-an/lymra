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
| `collatz` | 0.254 | 0.289 | 0.281 | 1.14 |
| `fibonacci` | 0.166 | 0.004 | 0.003 | 0.02 |
| `gcd_sum` | 0.158 | 0.154 | 0.156 | 0.98 |
| `modulo_sum` | 0.315 | 0.001 | 0.370 | < 0.01 |
| `primes` | 1.038 | 1.027 | 1.026 | 0.99 |
| `sieve` | 0.885 | 0.965 | 0.930 | 1.09 |
| `sum_formula` | 0.001 | 0.001 | 1.113 | n/a |

## Built with clang

| Program | C | Lymra | Lymra with every check | Lymra / C |
|---|---:|---:|---:|---:|
| `collatz` | 0.162 | 0.274 | 0.289 | 1.69 |
| `fibonacci` | 0.311 | 0.477 | 0.484 | 1.54 |
| `gcd_sum` | 0.126 | 0.123 | 0.122 | 0.97 |
| `modulo_sum` | 0.289 | 0.001 | 0.323 | < 0.01 |
| `primes` | 0.628 | 0.626 | 0.624 | 1.00 |
| `sieve` | 0.978 | 0.894 | 0.834 | 0.91 |
| `sum_formula` | 0.001 | 0.001 | 0.703 | n/a |

## Against Rust

Rust and Clang both generate code with LLVM. Rust with overflow checks stops on overflow, as Lymra does.

| Program | Lymra (clang) | Rust with overflow checks | Rust | Lymra / Rust with overflow checks |
|---|---:|---:|---:|---:|
| `collatz` | 0.274 | 0.269 | 0.183 | 1.02 |
| `fibonacci` | 0.477 | 0.621 | 0.314 | 0.77 |
| `gcd_sum` | 0.123 | 0.123 | 0.126 | 0.99 |
| `modulo_sum` | 0.001 | 0.319 | 0.269 | < 0.01 |
| `primes` | 0.626 | 0.624 | 0.640 | 1.00 |
| `sieve` | 0.894 | 0.991 | 0.961 | 0.90 |
| `sum_formula` | 0.001 | 0.725 | 0.002 | < 0.01 |

## Noise

Among versions whose median is at least 50 ms, the slowest timed run was 36% slower than that version's median.

## Machine

- Processor: Intel(R) Xeon(R) Processor @ 2.10GHz, 4 hardware threads
- System: linux x86_64
- `gcc`: gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0
- `clang`: Ubuntu clang version 18.1.3 (1ubuntu1)
- `rustc`: rustc 1.97.0 (2d8144b78 2026-07-07)
- C flags: `-std=c11 -O2`, the ones `lymra build` uses. Rust flags: `--edition 2024 -C opt-level=3`.
