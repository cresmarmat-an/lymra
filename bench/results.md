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
| `collatz` | 0.262 | 0.279 | 0.258 | 1.07 |
| `fibonacci` | 0.188 | 0.004 | 0.004 | 0.02 |
| `gcd_sum` | 0.145 | 0.150 | 0.153 | 1.04 |
| `modulo_sum` | 0.291 | 0.001 | 0.344 | < 0.01 |
| `primes` | 1.024 | 1.034 | 1.028 | 1.01 |
| `sieve` | 0.881 | 0.869 | 0.828 | 0.99 |
| `sum_formula` | 0.001 | 0.001 | 1.011 | n/a |

## Built with clang

| Program | C | Lymra | Lymra with every check | Lymra / C |
|---|---:|---:|---:|---:|
| `collatz` | 0.169 | 0.375 | 0.376 | 2.22 |
| `fibonacci` | 0.335 | 0.502 | 0.500 | 1.50 |
| `gcd_sum` | 0.122 | 0.122 | 0.126 | 1.00 |
| `modulo_sum` | 0.318 | 0.001 | 0.318 | < 0.01 |
| `primes` | 0.648 | 0.635 | 0.624 | 0.98 |
| `sieve` | 0.944 | 0.871 | 0.933 | 0.92 |
| `sum_formula` | 0.001 | 0.001 | 0.659 | n/a |

## Against Rust

Rust and Clang both generate code with LLVM. Rust with overflow checks stops on overflow, as Lymra does.

| Program | Lymra (clang) | Rust with overflow checks | Rust | Lymra / Rust with overflow checks |
|---|---:|---:|---:|---:|
| `collatz` | 0.375 | 0.275 | 0.172 | 1.36 |
| `fibonacci` | 0.502 | 0.621 | 0.304 | 0.81 |
| `gcd_sum` | 0.122 | 0.115 | 0.118 | 1.06 |
| `modulo_sum` | 0.001 | 0.317 | 0.279 | < 0.01 |
| `primes` | 0.635 | 0.620 | 0.626 | 1.02 |
| `sieve` | 0.871 | 0.946 | 0.896 | 0.92 |
| `sum_formula` | 0.001 | 0.690 | 0.002 | < 0.01 |

## Noise

Among versions whose median is at least 50 ms, the slowest timed run was 30% slower than that version's median.

## Machine

- Processor: Intel(R) Xeon(R) Processor @ 2.10GHz, 4 hardware threads
- System: linux x86_64
- `gcc`: gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0
- `clang`: Ubuntu clang version 18.1.3 (1ubuntu1)
- `rustc`: rustc 1.97.0 (2d8144b78 2026-07-07)
- C flags: `-std=c11 -O2`, the ones `lymra build` uses. Rust flags: `--edition 2024 -C opt-level=3`.
