# Benchmarks

Each directory in `programs/` holds one program written three times: `program.lym`, `program.c` and `program.rs`. The three versions do the same work in the same way, so a difference in time comes from the language and its compiler, not from the algorithm.

```bash
cargo run --release -p lymra-bench
```

```bash
cargo run --release -p lymra-bench -- --runs 9 primes collatz
```

The runner builds every program in these ways, and skips a compiler that is not installed:

| Version | How it is built |
|---|---|
| C | `gcc` or `clang` with `-std=c11 -O2`, the flags `lymra build` uses |
| Lymra | As `lymra build` does it: the optimizer removes every overflow check it proves can never fail, then `gcc` or `clang` compiles the C |
| Lymra with every check | The same without the optimizer, so every operation is checked |
| Rust | `rustc --edition 2024 -C opt-level=3`. Overflow wraps around silently |
| Rust with overflow checks | The same with `-C overflow-checks=on`. Overflow stops the program, as it does in Lymra |

It checks that every version prints the same output, runs each one once to warm up and then five more times, taking turns, and writes the medians to [results.md](results.md).

## What the results show

The numbers below come from one run on a shared cloud machine, recorded in [results.md](results.md). Treat differences under about 10% as noise.

- **Proven safe runs as fast as C.** In `primes`, `gcd_sum`, `modulo_sum` and `sum_formula`, the optimizer proves that no operation can overflow, divide by zero or leave its array, and removes every check. With GCC each runs within 1% of C's time, and with Clang so do `primes` and `gcd_sum`; Clang's `modulo_sum` moves with the machine, as the third point explains. `primes` needs the bound that `while divisor * divisor <= candidate` puts on the divisor, and the values its one call passes; `gcd_sum` needs its `while` loop worked out to a fixed point, and what the function returns, to bound the total. In `sum_formula`, GCC and Clang go further and replace the loop with a formula, so it finishes in a millisecond. With every check it takes 0.7 to 1.1 seconds, because a check has to stop the program at the exact operation that overflows, and that rules the formula out. Rust with overflow checks keeps its checks and takes as long.
- **Checks that can fail stay.** `collatz` grows its value by `value * 3 + 1` with no bound in sight, and `fibonacci` adds results that grow with every level of recursion. As far as the analysis can see, those operations can overflow, so their checks stay. Built with GCC, `collatz` takes 1.12 times as long as C: a check against a constant compares the other operand with a limit, which leaves the arithmetic plain for GCC to fold.
- **Clang's numbers here say more about the machine than about the checks.** On this cloud machine, a Clang build of `collatz` swings by up to 45% with changes outside its loop: registering the signal stack that catches stack overflow, which happens once before `main`, makes the same loop slower with one way of writing its checks and faster with another. With that call removed, both ways run alike. Even `modulo_sum`, which keeps no check, moved between 0.98 and 1.20 times C's time across runs. Measure on the machine you care about.
- **One number is a compiler accident.** Built with GCC, Lymra's `fibonacci` finishes in a few milliseconds. The work is still exponential: GCC inlines the recursion many levels deep, which it does not do for the C version, whose `+` it rewrites into an accumulating loop instead. Clang shows the real cost of the check on the `+`, about what Rust with overflow checks pays. The GCC number does not mean Lymra is faster than C.
