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

**Edition 0.1, roadmap phase 3 under way.** The compiler checks Lymra programs and compiles them to native executables through C. It covers integers of every size and floats, with checked conversions between them, wrapping arithmetic, booleans, text built from any values and kept by bindings, lists and types, and comparing it, arrays, lists that grow on data stacks with no garbage collector and no `free`, stacks that also shrink with `pop`, views that read arrays, lists, stacks and text without copying them, with `slice` and `bytes`, pools whose values come and go in any order behind generation-checked handles, custom types, choices and `match`, functions, `variable` parameters, functions that fail, whose failures every caller handles with `match` or passes on with `try`, the command-line arguments as text and an exit status from `main`, reading and writing files and reading standard input, `if`, `while`, `for` over `range`, arrays, lists, stacks and views, and `print`. Every integer operation and every index is checked, in every build mode, except where the compiler proves the check can never fail. A failed check, or recursion that runs out of stack, stops the program with an error message and exit status 70 instead of a crash. Nothing can read a list, a stack or a text after it is freed, or change it over another, and no handle can be used with a pool it didn't come from: the compiler works out what every view shows and where every handle comes from across the whole program, with no annotations, and rejects the programs that could (`L0701` to `L0705`). Every test program also runs under AddressSanitizer in CI. Ten of the twelve programs of the [memory model study](docs/memory/README.md) build and run. Anything later phases add is rejected with an error that names the phase.

## Try it

The compiler is written in Rust and has no dependencies. It needs Rust 1.85 or newer, and GCC, Clang or MSVC to build programs. On Windows, use GCC from MinGW-w64, Clang, or MSVC's `cl` from a Visual Studio developer prompt, in Visual Studio 2019 version 16.8 or newer; on macOS, the Xcode command line tools provide Clang.

Every change is built and tested on each of these:

| System | Processors |
|---|---|
| Linux | x86-64, ARM64 and 32-bit x86; under QEMU, 32-bit ARM, 64-bit RISC-V, 64-bit PowerPC and big-endian IBM Z |
| macOS | ARM64, and x86-64 both on an Intel Mac and under Rosetta |
| Windows | x86-64, ARM64 and 32-bit x86, with the compilers of Visual Studio 2019, 2022 and the current release |
| FreeBSD | x86-64 and ARM64 |
| OpenBSD, NetBSD | x86-64 |

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

The C compiler is the one in the `CC` environment variable, or else the first of `cc`, `gcc`, `clang` and `cl` that is installed. `#!optimize speed`, `size` or `debug` at the top of a file chooses how it is optimized; the default is `speed`.

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

`cargo run --release -p lymra-bench` times seven programs written in Lymra, C and Rust, and writes [bench/results.md](bench/results.md). On the last run:

- Where the compiler proves that no check can fail, Lymra runs as fast as C: four of the seven programs keep no check, and two of them take 0.96 to 1.04 times C's time across runs. In another, GCC and Clang replace the loop with a formula: it takes 1 millisecond, against 0.64 seconds for Rust with overflow checks, which keeps every check.
- A loop the C compilers leave slow, Lymra writes so they can make it fast. In the fourth, a remainder on every turn keeps C's loop as written; Lymra runs it in blocks of turns where the remainder is a constant, and it takes 1 millisecond, against 0.29 seconds for C and for Rust.
- A list fills at least as fast as a C array that doubles as it grows: a sieve over a hundred million booleans, pushed one at a time, took 0.95 to 1.09 times C's time built with GCC across runs, and 0.91 times with Clang. The loop that pushes them reserves room for every element first, so the C compiler turns it into one fill of memory. The compiler proves every index of its loops inside the list, so no bounds check runs.
- The checks that stay guard values no bound the compiler finds can hold, such as a Collatz chain. Built with GCC, that program takes 1.04 to 1.14 times as long as C across runs; built with Clang, 1.69 times, level with Rust with overflow checks.

[bench/README.md](bench/README.md) explains every number, including the one that flatters Lymra by accident.

## Documents

- [Language specification](docs/spec.md)
- [Memory model study](docs/memory/README.md): the memory model tested against twelve programs
- [Error codes](docs/errors/README.md)
- Examples: [hello](examples/hello.lym), [overflow](examples/overflow.lym), [sieve](examples/sieve.lym)
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
| `compiler/src/optimize` | Interval analysis that removes the overflow and index checks it proves can never fail |
| `compiler/src/codegen` | The checked program to C, and the runtime every program includes |
| `compiler/src/driver.rs` | Finds the C compiler, builds, and runs |
| `compiler/src/diagnostics` | Error codes and how errors are rendered |
| `compiler/tests/ui` | Golden syntax errors and syntax trees |
| `compiler/tests/check` | Golden errors about names, types and control flow |
| `compiler/tests/run` | Programs that are built with every C compiler on the machine, with and without the optimizer, and run, with their recorded output |
| `compiler/tests/runtime_arithmetic.rs` | The runtime's overflow checks, in both the GCC and Clang form and the portable form, against exact arithmetic |
| `compiler/tests/runtime_data_stacks.rs` | The runtime's data stacks on their own: growth, moves, freeing, shrinking, and the errors a list can stop a program with |
| `compiler/tests/windows_console.rs`, `pseudo_console.rs` | On Windows, output reaches a console, and a terminal's pseudo console, as the characters printed |
| `compiler/tests/overflow_proofs` | Thousands of generated programs run in an exact interpreter, to catch a check removed by a wrong proof, and generated list programs built with every C compiler that have to print what the interpreter prints |
| `bench` | The same programs in Lymra, C and Rust, and the runner that times them |
| `docs/errors` | One page per error code, embedded in the compiler for `lymra explain` |
| `docs/memory` | The memory model study: twelve programs written against the spec, and what they changed in it |

## Development

```bash
cargo test
```

The tests compare the compiler's exact output with recorded files: error reports, syntax trees, and the output of programs it builds. They compile every test program with each C compiler on the machine, GCC, Clang and MSVC, with all warnings as errors, so the generated C stays clean. `CC` adds a compiler with its arguments, as in `CC="gcc -m32"`, and `LYMRA_TEST_COMPILERS` names the exact list instead, separated by commas, as in `LYMRA_TEST_COMPILERS="cl, clang -m32"`. Naming a compiler with the sanitizers runs every program under them, as CI does:

```bash
LYMRA_TEST_COMPILERS="gcc -fsanitize=address -fsanitize=undefined -fno-sanitize-recover=all" cargo test
```

After a deliberate change, accept the new output:

```bash
LYMRA_BLESS=1 cargo test
```

## Roadmap

Each phase ends with a compiler that passes its own test suite. Nothing advances on a broken phase.

| Phase | Scope | Exit criterion | State |
|---|---|---|---|
| 0 | Lexer, parser, diagnostics format | Every snippet in the spec parses or fails with the documented error | Done |
| 1 | Integers, booleans, functions, `if`, `while`, `for` over `range`, `print`. Emits C. | `examples/hello.lym` compiles and runs | Done |
| 2 | Floats, arrays, custom types and choices, comparing text, `length`, wrapping arithmetic | Type errors carry fix suggestions | Done |
| 3 | Data stacks, `list`, `stack`, `view`, text built from values, `with` blocks, root inference | Escaping-reference programs are rejected with `L0701` | Data stacks, lists, stacks, views, text built from values and root inference done |
| 4 | Modules and `public` | Multi-file programs build | |
| 5 | Pools, error handling, threads | Benchmark suite runs against C++ and Rust | Pools and error handling done |

The compiler emits C. That gets native speed from mature optimizers (GCC, Clang) without writing a code generator first.

## License

Copyright 2026 Cresmar Mat-an. Licensed under the Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
