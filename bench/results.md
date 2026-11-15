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
| `collatz` | 0.269 | 0.280 | 0.293 | 1.04 |
| `fibonacci` | 0.257 | 0.004 | 0.004 | 0.02 |
| `gcd_sum` | 0.153 | 0.151 | 0.154 | 0.99 |
| `modulo_sum` | 0.352 | 0.335 | 0.365 | 0.95 |
| `primes` | 1.043 | 1.042 | 1.031 | 1.00 |
| `sieve` | 0.734 | 0.722 | 0.757 | 0.98 |
| `sum_formula` | 0.001 | 0.001 | 1.011 | n/a |

## Built with clang

| Program | C | Lymra | Lymra with every check | Lymra / C |
|---|---:|---:|---:|---:|
| `collatz` | 0.168 | 0.412 | 0.403 | 2.46 |
| `fibonacci` | 0.348 | 0.690 | 0.663 | 1.98 |
| `gcd_sum` | 0.121 | 0.124 | 0.122 | 1.03 |
| `modulo_sum` | 0.304 | 0.308 | 0.329 | 1.01 |
| `primes` | 0.629 | 0.632 | 0.644 | 1.01 |
| `sieve` | 0.809 | 0.888 | 0.788 | 1.10 |
| `sum_formula` | 0.001 | 0.001 | 0.673 | n/a |

## Against Rust

Rust and Clang both generate code with LLVM. Rust with overflow checks stops on overflow, as Lymra does.

| Program | Lymra (clang) | Rust with overflow checks | Rust | Lymra / Rust with overflow checks |
|---|---:|---:|---:|---:|
| `collatz` | 0.412 | 0.303 | 0.181 | 1.36 |
| `fibonacci` | 0.690 | 0.660 | 0.333 | 1.05 |
| `gcd_sum` | 0.124 | 0.122 | 0.130 | 1.01 |
| `modulo_sum` | 0.308 | 0.343 | 0.265 | 0.90 |
| `primes` | 0.632 | 0.652 | 0.637 | 0.97 |
| `sieve` | 0.888 | 0.720 | 0.782 | 1.23 |
| `sum_formula` | 0.001 | 0.648 | 0.001 | < 0.01 |

## Noise

Among versions whose median is at least 50 ms, the slowest timed run was 39% slower than that version's median.

## Machine

- Processor: Intel(R) Xeon(R) Processor @ 2.10GHz, 4 hardware threads
- System: linux x86_64
- `gcc`: gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0
- `clang`: Ubuntu clang version 18.1.3 (1ubuntu1)
- `rustc`: rustc 1.97.0 (2d8144b78 2026-07-07)
- C flags: `-std=c11 -O2`, the ones `lymra build` uses. Rust flags: `--edition 2024 -C opt-level=3`.
