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

- **Proven safe runs as fast as C.** In `sum_formula` and `modulo_sum`, the optimizer proves that no operation in the loop can overflow and removes every check. `modulo_sum` then runs at C's speed. In `sum_formula`, GCC and Clang go further and replace the loop with a formula, so it finishes in a millisecond. With every check it takes 0.62 seconds, because a check has to stop the program at the exact operation that overflows, and that rules the formula out. Rust with overflow checks keeps its checks and takes 0.62 seconds as well.
- **Checks that stay cost up to 1.7 times C's time.** `collatz` multiplies inside a `while` loop, and the analysis gives up on the variables a `while` loop changes. `fibonacci` adds the results of two calls, and the analysis knows nothing about what a call returns. Those checks stay, so `collatz` takes 1.2 (GCC) to 1.6 (Clang) times as long as C, and `fibonacci` 1.7 times as long with Clang. Rust with overflow checks pays about the same.
- **Division hides the rest.** `gcd_sum` and `primes` spend their time dividing, and every version runs within noise of the others.
- **One number is a compiler accident.** Built with GCC, Lymra's `fibonacci` runs 3.4 times faster than the C version. The check on the `+` keeps GCC from rewriting the C version's recursion into an accumulating loop, and the code GCC makes instead happens to be faster on this machine. Clang shows the real cost of the check. The GCC number does not mean Lymra is faster than C.

Two things would remove most of the checks that stay: bounds for the variables a `while` loop changes, found by repeating the analysis until it settles, and facts about parameters and results carried across calls. Both are open question O11 in the [specification](../docs/spec.md).
