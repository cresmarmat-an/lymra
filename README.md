<p align="center">
  <img alt="Lymra logo: an L whose corner fills with a stack of blocks, which then clears" src="branding/animated/lymra-icon-animated.svg" width="160">
</p>

<h1 align="center">Lymra</h1>

A programming language for building safe, fast, and efficient software. Lymra keeps memory safe without a garbage collector, reference counting, or lifetime annotations, and its compiler explains every error it rejects with a concrete fix.

```lymra
constant string author = "Cresmar Mat-an"

function main()
    print("Hello, {author}")

    variable integer64 counter = 1
    while counter < 1_000_000
        counter = counter + 1
    print("{counter}")
```

Every name the language defines is a whole English word: `variable`, `constant`, `function`, `integer64`, `boolean`, `length`.

## Status

**Edition 0.1, roadmap phase 1.** The compiler checks Lymra programs and compiles them to native executables through C. Phase 1 covers integers of every size and checked conversions between them, booleans, text values, functions, `variable` parameters, `if`, `while`, `for` over `range`, and `print`. Every integer operation is checked for overflow, in every build mode, except where the compiler proves the check can never fail. A failed check, or recursion that runs out of stack, stops the program with an error message and exit status 70 instead of a crash. Anything later phases add is rejected with an error that names the phase.

## Try it

The compiler is written in Rust and has no dependencies. It needs Rust 1.85 or newer, and GCC or Clang to build programs. It works on Linux, macOS and Windows, and every change is tested on all three. On Windows, use GCC from MinGW-w64 or Clang; on macOS, the Xcode command line tools provide Clang.

```bash
cargo build --release
```

```bash
./target/release/lymra run examples/hello.lym
```

| Command | What it does |
|---|---|
| `lymra run <file.lym>` | Builds a program and runs it |
| `lymra build <file.lym>` | Builds a program into an executable next to its source |
| `lymra check <file.lym>` | Checks a file and reports every error in it |
| `lymra emit <file.lym>` | Prints the C code the compiler generates |
| `lymra parse <file.lym>` | Prints the syntax tree of a file |
| `lymra explain <code>` | Explains an error code: the rule, why it exists, and the fix |

The C compiler is the one in the `CC` environment variable, or else `cc`, `gcc` or `clang`. `#!optimize speed`, `size` or `debug` at the top of a file chooses how it is optimized; the default is `speed`.

An error names the rule, shows the place, and shows the corrected line:

```
error[L0203]: this declaration needs `variable` or `constant` in front
 --> src/main.lym:2:5
  |
2 |     string name = "Kero"
  |     ^^^^^^^^^^^ a declaration starts with a keyword
  |
help: write `constant` for a value that never changes, or `variable` for one that does
  |
2 |     constant string name = "Kero"
```

A program that fails a check while it runs stops with the same kind of message, and exit status 70:

```
error: integer overflow
 --> examples/overflow.lym:9:17
  |
9 |         level = level + 1
  |                 ^^^^^^^^^
```

## Speed

`cargo run --release -p lymra-bench` times six programs written in Lymra, C and Rust, and writes [bench/results.md](bench/results.md). On the last run:

- Where the compiler proves that no overflow check in a loop can fail, Lymra runs as fast as C. In one benchmark that lets GCC and Clang replace the loop with a formula: it takes 1 millisecond, against 0.62 seconds for Rust with overflow checks, which keeps every check.
- Where checks stay in hot code, Lymra takes up to 1.7 times as long as C, about what Rust with overflow checks takes.

[bench/README.md](bench/README.md) explains every number, including the one that flatters Lymra by accident.

## Documents

- [Language specification](docs/spec.md)
- [Error codes](docs/errors/README.md)
- Examples: [hello](examples/hello.lym), [overflow](examples/overflow.lym)
- [Brand assets](branding/README.md)

## Repository layout

| Path | Contents |
|---|---|
| `compiler/src/lexer` | Source text to tokens, including indentation |
| `compiler/src/parser` | Tokens to the syntax tree, with error recovery |
| `compiler/src/ast` | The syntax tree, its printer, and a visitor |
| `compiler/src/lint` | Rules that need the whole tree, such as naming |
| `compiler/src/semantic` | Names, types and control flow, producing the checked program |
| `compiler/src/ir` | The checked program: every name resolved, every expression typed |
| `compiler/src/optimize` | Interval analysis that removes the overflow checks it proves can never fail |
| `compiler/src/codegen` | The checked program to C, and the runtime every program includes |
| `compiler/src/driver.rs` | Finds the C compiler, builds, and runs |
| `compiler/src/diagnostics` | Error codes and how errors are rendered |
| `compiler/tests/ui` | Golden syntax errors and syntax trees |
| `compiler/tests/check` | Golden errors about names, types and control flow |
| `compiler/tests/run` | Programs that are built with GCC and Clang, with and without the optimizer, and run, with their recorded output |
| `compiler/tests/overflow_proofs` | Thousands of generated programs run in an exact interpreter, to catch a check removed by a wrong proof |
| `bench` | The same programs in Lymra, C and Rust, and the runner that times them |
| `docs/errors` | One page per error code, embedded in the compiler for `lymra explain` |

## Development

```bash
cargo test
```

The tests compare the compiler's exact output with recorded files: error reports, syntax trees, and the output of programs it builds. They compile every test program with GCC and with Clang, with all warnings as errors, so the generated C stays clean. After a deliberate change, accept the new output:

```bash
LYMRA_BLESS=1 cargo test
```

## Roadmap

Each phase ends with a compiler that passes its own test suite. Nothing advances on a broken phase.

| Phase | Scope | Exit criterion | State |
|---|---|---|---|
| 0 | Lexer, parser, diagnostics format | Every snippet in the spec parses or fails with the documented error | Done |
| 1 | Integers, booleans, functions, `if`, `while`, `for` over `range`, `print`. Emits C. | `examples/hello.lym` compiles and runs | Done |
| 2 | Floats, arrays, custom types and choices, comparing text, `length`, wrapping arithmetic | Type errors carry fix suggestions | Next |
| 3 | Data stacks, `list`, `stack`, `view`, text built from values, `with` blocks, root inference | Escaping-reference programs are rejected with `L0701` | |
| 4 | Modules and `public` | Multi-file programs build | |
| 5 | Pools, error handling, threads | Benchmark suite runs against C++ and Rust | |

The compiler emits C. That gets native speed from mature optimizers (GCC, Clang) without writing a code generator first.

## License

Copyright 2026 Cresmar Mat-an. Licensed under the Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
