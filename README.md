<p align="center">
  <img alt="Lymra logo: an L whose corner fills with a stack of blocks, which then clears" src="branding/animated/lymra-icon-animated.svg" width="160">
</p>

<h1 align="center">Lymra</h1>

A programming language for building safe, fast, and efficient software. Lymra keeps memory safe without a garbage collector, reference counting, or lifetime annotations, and its compiler explains every error it rejects with a concrete fix.

Status: **design draft 0.1**. There is no compiler yet.

## Documents

- [Language specification](docs/spec.md)
- [Example](examples/hello.lym)
- [Brand assets](branding/README.md)

## Roadmap

Each phase ends with a compiler that passes its own test suite. Nothing advances on a broken phase.

| Phase | Scope | Exit criterion |
|---|---|---|
| 0 | Lexer, parser, diagnostics format | Every snippet in the spec parses or fails with the documented error |
| 1 | Integers, bools, functions, `if`, `while`, `print`. Emits C. | `examples/hello.lym` compiles and runs |
| 2 | Type inference, `string`, `array`, custom types | Type errors carry fix suggestions |
| 3 | Data stacks, `list`, `view`, root inference | Escaping-reference programs are rejected with `L0701` |
| 4 | Modules and `public` | Multi-file programs build |
| 5 | Pools, error handling, threads | Benchmark suite runs against C++ and Rust |

The compiler emits C in early phases. That gets native speed from mature backends (GCC, Clang) without writing a code generator first.

## License

Copyright 2026 Cresmar Mat-an. Licensed under the Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
