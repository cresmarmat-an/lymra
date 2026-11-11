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

**Edition 0.1, roadmap phase 0.** The compiler reads Lymra source, checks its syntax and naming, and reports every error with a fix. It does not produce programs yet. That is phase 1.

## Try it

The compiler is written in Rust and has no dependencies. It needs Rust 1.85 or newer.

```bash
cargo build --release
```

```bash
./target/release/lymra check examples/hello.lym
```

| Command | What it does |
|---|---|
| `lymra check <file.lym>` | Checks a file and reports every error in it |
| `lymra parse <file.lym>` | Prints the syntax tree of a file |
| `lymra explain <code>` | Explains an error code: the rule, why it exists, and the fix |

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

## Documents

- [Language specification](docs/spec.md)
- [Error codes](docs/errors/README.md)
- [Example](examples/hello.lym)
- [Brand assets](branding/README.md)

## Repository layout

| Path | Contents |
|---|---|
| `compiler/src/lexer` | Source text to tokens, including indentation |
| `compiler/src/parser` | Tokens to the syntax tree, with error recovery |
| `compiler/src/ast` | The syntax tree, its printer, and a visitor |
| `compiler/src/lint` | Rules that need the whole tree, such as naming |
| `compiler/src/diagnostics` | Error codes and how errors are rendered |
| `compiler/tests` | Golden files for every error message, plus spec, CLI and robustness tests |
| `docs/errors` | One page per error code, embedded in the compiler for `lymra explain` |

## Development

```bash
cargo test
```

The tests compare the compiler's exact output with files under `compiler/tests/ui`. After a deliberate change to a message, accept the new output:

```bash
LYMRA_BLESS=1 cargo test
```

## Roadmap

Each phase ends with a compiler that passes its own test suite. Nothing advances on a broken phase.

| Phase | Scope | Exit criterion | State |
|---|---|---|---|
| 0 | Lexer, parser, diagnostics format | Every snippet in the spec parses or fails with the documented error | Done |
| 1 | Integers, booleans, functions, `if`, `while`, `print`. Emits C. | `examples/hello.lym` compiles and runs | Next |
| 2 | Type inference, `string`, `array`, custom types | Type errors carry fix suggestions | |
| 3 | Data stacks, `list`, `view`, root inference | Escaping-reference programs are rejected with `L0701` | |
| 4 | Modules and `public` | Multi-file programs build | |
| 5 | Pools, error handling, threads | Benchmark suite runs against C++ and Rust | |

The compiler emits C in early phases. That gets native speed from mature backends (GCC, Clang) without writing a code generator first.

## License

Copyright 2026 Cresmar Mat-an. Licensed under the Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
