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

- **Proven safe runs as fast as C.** In `primes`, `gcd_sum`, `modulo_sum` and `sum_formula`, the optimizer proves that no operation can overflow, divide by zero or leave its array, and removes every check. With GCC each takes 0.95 to 1.00 times C's time, and with Clang 1.01 to 1.03 times; Clang's `modulo_sum` also moves with the machine, as the fourth point explains. `primes` needs the bound that `while divisor * divisor <= candidate` puts on the divisor, and the values its one call passes; `gcd_sum` needs its `while` loop worked out to a fixed point, and what the function returns, to bound the total. In `sum_formula`, GCC and Clang go further and replace the loop with a formula, so it finishes in a millisecond. With every check it takes 0.7 to 1.1 seconds, because a check has to stop the program at the exact operation that overflows, and that rules the formula out. Rust with overflow checks keeps its checks and takes as long.
- **A list grows as fast as a C array that doubles.** `sieve` pushes a hundred million booleans one at a time, then crosses out the multiples of each prime. The list grows in place on its data stack and is never copied; the C and Rust versions double their storage as they grow. The optimizer proves both indexes inside the list, from `range(2, length(composite))` and `while multiple < length(composite)`, so only the overflow checks on `candidate * candidate` and `multiple + candidate` stay. Built with GCC it took 0.98 times C's time here, and 1.05 times in a run of the sieve alone; built with Clang, 1.10 and 0.97 times. Against Rust with overflow checks it took 1.23 and 0.99 times: within this machine's noise, which reached 39% on this run.
- **Checks that can fail stay.** `collatz` grows its value by `value * 3 + 1` with no bound in sight, and `fibonacci` adds results that grow with every level of recursion. As far as the analysis can see, those operations can overflow, so their checks stay. Built with GCC, `collatz` takes 1.04 times as long as C here, and took 1.12 times on the run before: a check against a constant compares the other operand with a limit, which leaves the arithmetic plain for GCC to fold.
- **Clang's numbers here say more about the machine than about the checks.** On this cloud machine, a Clang build of `collatz` swings by up to 45% with changes outside its loop: registering the signal stack that catches stack overflow, which happens once before `main`, makes the same loop slower with one way of writing its checks and faster with another. With that call removed, both ways run alike. Even `modulo_sum`, which keeps no check, moved between 0.98 and 1.20 times C's time across runs. Measure on the machine you care about.
- **One number is a compiler accident.** Built with GCC, Lymra's `fibonacci` finishes in a few milliseconds. The work is still exponential: GCC inlines the recursion many levels deep, which it does not do for the C version, whose `+` it rewrites into an accumulating loop instead. Clang shows the real cost of the check on the `+`, about what Rust with overflow checks pays. The GCC number does not mean Lymra is faster than C.
