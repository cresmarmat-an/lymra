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

- **Proven safe runs as fast as C.** In `primes`, `gcd_sum`, `modulo_sum` and `sum_formula`, the optimizer proves that no operation can overflow, divide by zero or leave its array, and removes every check. With GCC and with Clang, `primes` and `gcd_sum` take 0.98 to 1.04 times C's time. `primes` needs the bound that `while divisor * divisor <= candidate` puts on the divisor, and the values its one call passes; `gcd_sum` needs its `while` loop worked out to a fixed point, and what the function returns, to bound the total. In `sum_formula`, GCC and Clang go further and replace the loop with a formula, so it finishes in a millisecond. With every check it takes 0.7 to 1.1 seconds, because a check has to stop the program at the exact operation that overflows, and that rules the formula out. Rust with overflow checks keeps its checks and takes as long.
- **A loop the C compilers leave slow, Lymra writes so they can make it fast.** `modulo_sum` adds up `index % 7 * 3` over three hundred million turns. GCC and Clang see a division on every turn and compile C's loop as written. Lymra runs the loop in blocks of seven turns, starting at a multiple of seven, where `index % 7` is the turn's place in its block (spec section 13). Both C compilers then unroll the block and replace the loop with a formula, so it finishes in a millisecond, against 0.29 seconds for C and for Rust. The same restatement written by hand in C would run as fast: what Lymra adds is that the compiler does it, for every loop of that shape, and proves it computes exactly what the loop does. With every check, the remainder stays checked and the loop runs as written, at C's speed.
- **A list fills at least as fast as a C array that doubles.** `sieve` pushes a hundred million booleans one at a time, then crosses out the multiples of each prime. The list grows in place on its data stack and is never copied; the C and Rust versions double their storage as they grow. The loop that fills the list pushes once on every turn, so Lymra reserves room for every element before the first turn and pushes with no check (spec section 13), and GCC and Clang turn the loop into one fill of memory. The optimizer proves both indexes inside the list, from `range(2, length(composite))` and `while multiple < length(composite)`, so only the overflow checks on `candidate * candidate` and `multiple + candidate` stay. Built with GCC it took 0.99 times C's time here, and with Clang 0.92 times; against Rust with overflow checks, 0.92 times. Timed alone, by processor time over eleven runs, it took 0.95 times C's time with GCC. This machine's noise reached 30% on this run, so call it a tie with C at worst.
- **Checks that can fail stay.** `collatz` grows its value by `value * 3 + 1` with no bound in sight, and `fibonacci` adds results that grow with every level of recursion. As far as the analysis can see, those operations can overflow, so their checks stay. Built with GCC, `collatz` takes 1.07 times as long as C here, and took 1.04 and 1.12 times on earlier runs: a check against a constant compares the other operand with a limit, which leaves the arithmetic plain for GCC to fold.
- **Clang's numbers here say more about the machine than about the checks.** On this cloud machine, a Clang build of `collatz` swings by up to 45% with changes outside its loop: registering the signal stack that catches stack overflow, which happens once before `main`, makes the same loop slower with one way of writing its checks and faster with another. With that call removed, both ways run alike. Measure on the machine you care about.
- **One number is a compiler accident.** Built with GCC, Lymra's `fibonacci` finishes in a few milliseconds. The work is still exponential: GCC inlines the recursion many levels deep, which it does not do for the C version, whose `+` it rewrites into an accumulating loop instead. Clang shows the real cost of the check on the `+`, about what Rust with overflow checks pays. The GCC number does not mean Lymra is faster than C.
