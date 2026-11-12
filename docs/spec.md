# Lymra Language Specification, Draft 0.1

Status: draft. Every rule here can change until a compiler implements it and a test proves it.

The compiler in `compiler/` implements the syntax of this whole document. Its test suite parses every code sample here, so the spec and the parser cannot drift apart. It also checks and compiles the core of the language: integers, booleans, text values, functions, `if`, `while`, `for` over `range`, and `print`. The rest arrives in the phases of the roadmap in the README, and until then the compiler rejects it with `L0309`, naming the phase.

## 1. Goals

Each goal has to be checkable. If a goal can't be measured, it's just a slogan.

1. **Memory safety without runtime machinery.** No garbage collector, no reference counting, no general purpose heap allocator, no manual `free`, no lifetime annotations. A program that compiles cannot read freed memory, read uninitialized memory, write out of bounds, or race on shared memory.
2. **Speed through layout, not tricks.** Data is stored inline and contiguously by default. Memory is released by moving a pointer, not by calling an allocator. Speed claims are made only against the public benchmark suite (Section 14), losses included.
3. **The compiler teaches.** Every rejected program gets an error code, the exact location, the reason, and at least one concrete fix.
4. **One way to write a thing.** If two spellings mean the same thing, one of them gets removed.

### Non-goals

- Catching logic errors. No compiler can reject every wrong program (Rice's theorem). Lymra rejects memory, type, initialization, and concurrency errors, plus a defined list of lint errors. A loop that counts to the wrong number still compiles.
- Safety switches. There is no directive, flag, or block that turns off safety checks.

## 2. Lexical structure

### 2.1 Source files

- Extension: `.lym`. Encoding: UTF-8. A file in any other encoding is error `L0115`. A byte order mark at the start is ignored.
- Lines end with a line feed, or with a carriage return and a line feed.
- One file is one module (Section 10).

### 2.2 Comments

```lymra
# Single line comment.

#[
    Multiline comment.
    #[ Nested comments are allowed, so you can comment out code that already has one. ]#
]#
```

- `#[` always opens a multiline comment. A single-line comment that begins with a bracket needs a space: `# [1] see below`. A comment that is never closed is error `L0109`.
- A comment that opens and closes on one line can sit anywhere, including before code. A comment that spans lines has to end its line: code after its closing `]#` is error `L0110`, because nobody looks for code there.

### 2.3 Directives

A line that starts with `#!` is a **directive**: an instruction to the compiler about the whole file. The general name for this feature is a *pragma*.

Rules:

- Directives may appear only at the top of a file, before any declaration. A directive anywhere else is error `L0101`.
- If line 1 starts with `#!/`, it is a Unix shebang and the compiler ignores it, so `#!/usr/bin/env lymra` works.
- An unknown directive is error `L0105`, not a warning. A misspelled directive that was silently ignored would leave the file compiled differently from what its author believes.
- A directive takes exactly one word from a fixed list and may be given once. Anything else is error `L0106`.
- A comment may follow a directive on its line.

Directives in 0.1:

| Directive | Meaning |
|---|---|
| `#!edition 0.1` | The language version this file is written against. Lets later versions change syntax without breaking old files. |
| `#!optimize speed` / `size` / `debug` | Overrides the build's optimization mode for this file only. |
| `#!prove bounds` | The compiler must prove every index in this file is in range. If any runtime bounds check would remain, compilation fails with `L0104` and lists each one with the fact it could not prove. |

`#!prove bounds` is the safe replacement for `#!nocheck`. Instead of switching checks off and trusting the programmer, it makes the compiler show that no check is needed. Hot loops get check-free code and keep their safety guarantee. The usual fix for an `L0104` is one `assert` before the loop, such as `assert length(items) >= count`, which pays for one check instead of one per iteration.

Rejected directives: `#!nocheck` and `#!nonstrict` would turn off safety, which contradicts Goal 1. `#!strict` would do nothing, because Lymra is always strict.

### 2.4 Indentation

- Blocks are defined by indentation, like Python. There are no braces and no `end` keyword.
- A file uses either tabs or spaces for indentation, never both. Mixing them is error `L0102`.
- With spaces, the indent width is set by the first indented line in the file and must stay the same everywhere in that file. With tabs, one tab is one level.
- A block is exactly one level deeper than the line that opens it. An amount of indentation that fits no level is error `L0107`.
- A line that opens a block has to be followed by at least one indented line (`L0303`), and only such a line may be followed by deeper indentation (`L0304`). A function that does nothing yet holds a single `return`.
- A statement ends at the end of its line. There is no `;`. Inside `( )` and `[ ]`, line breaks and indentation are ignored, which is how a long call continues over several lines:

```lymra
constant total = add(
    first_price,
    second_price
)
```

### 2.5 Keywords

All keywords are whole English words: `variable`, not `var`; `function`, not `fn`; `external`, not `extern`.

```
variable constant function returns return type choice packed use public
if else while for in break continue match case and or not
true false external assert with
```

The same rule holds for every name the language defines. The built-in types are `integer64` and `boolean`, not `i64` and `bool`, and the built-in functions are `length` and `print`, not `len`. A name that needs two words joins them with `_`, as in `wrapping_add`.

Spellings from other languages are not accepted as alternatives (Goal 4). Each gets error `L0305`, which names the Lymra spelling, and the compiler then checks the rest of the line as if that spelling had been written.

### 2.6 Naming

The compiler enforces naming style, so code reads the same in every project.

- Names use the letters `a-z` and `A-Z`, the digits `0-9` and `_`, and do not start with a digit. Two names that look the same are then always the same name. Text and comments may contain any character.
- Values, functions, parameters, fields, modules: `snake_case`.
- Types, choices, variants: `PascalCase`.
- A violation is error `L0103`, and the error message includes the corrected name. Only declarations are checked.
- The name `_` declares a binding that is deliberately never read. It is exempt from the style rule and from `L0202`.

### 2.7 Literals

- Integers: `42`, `1_000_000`, `0xFF`, `0b1010`.
- Floats: `3.14`, `1.0e-9`.
- Text: `"Hello, {name}"`. Braces interpolate any expression. Write `{{` and `}}` for literal braces.
- Booleans: `true` and `false`.

Numbers (errors are `L0114`):

- An underscore goes between two digits, one at a time.
- A decimal integer does not start with `0`. In C a leading zero means octal, so `010` is 8 there. Lymra rejects it instead of picking a meaning. There are no octal literals.
- A float has digits on both sides of the point: `1.0e9`, not `1e9`.
- The prefixes `0x` and `0b` and the exponent marker `e` are lowercase.
- A number has no type suffix. Its type comes from the declaration: `constant unsigned8 small = 42`.

Text:

- Text is written in double quotes and ends on the line where it starts (`L0111`).
- The escapes are `\n`, `\t`, `\r`, `\0`, `\\` and `\"`. Any other backslash sequence is error `L0112`.
- An interpolation holds one expression and cannot be empty (`L0113`). Text may appear inside an interpolation, up to 8 levels deep.

### 2.8 Operators

From the loosest binding to the tightest:

| Operators | Meaning | Grouping |
|---|---|---|
| `or` | either | left |
| `and` | both | left |
| `not` | negation | prefix |
| `==` `!=` `<` `<=` `>` `>=` | comparison | does not chain |
| `+` `-` | addition, subtraction | left |
| `*` `/` `%` | multiplication, division, remainder | left |
| `-` | negation | prefix |
| `f(x)` `items[i]` `item.field` | call, element, field | left |

- `not` covers a whole comparison: `not a == b` means `not (a == b)`.
- Comparisons do not chain. `a < b < c` is error `L0306`; write `a < b and b < c`. In C the chain compiles and compares the result of the first comparison with `c`, which is almost never what was meant.
- Parentheses group. Expressions, types and blocks may nest 64 levels deep (`L0310`).

## 3. Bindings

```lymra
variable string name = "Cresmar"   # mutable, type written
variable count = 0                  # mutable, type inferred as integer64
constant string greeting = "Hi"     # immutable, type written
constant copy_of_name = name        # immutable, type inferred
```

- `variable` declares a mutable binding.
- `constant` declares an immutable binding. Changing a constant, a parameter or a loop variable is error `L0208`.
- A top-level `constant` is computed by the compiler, from literals, operators and other top-level constants. Anything only known at run time, such as a function call, is error `L0210`, and so is a constant that depends on itself.
- Every declaration starts with one of these two keywords. A type and a name before `=` with no keyword is error `L0203`.
- Only `constant` may be declared at the top level of a file. A top-level `variable` would be global mutable state, and is error `L0205`.
- `name = expr` with no keyword always means assignment. Assigning to a name that hasn't been declared is error `L0201`, and the message suggests `variable` or `constant`.
- An assignment is a statement, not an expression, and its left side is a binding, a field or an element (`L0308`). There is no `+=` and no `++`: write `count = count + 1`.
- Reading a binding before it is assigned is impossible: every declaration has an initializer. Leaving it out is error `L0204`.
- An unused binding is error `L0202`. A binding declared `variable` that nothing changes is error `L0209`.
- A name can be declared only once where it is visible: a local cannot reuse the name of another local in an enclosing block, a parameter, or a top-level constant or function. Redeclaring a name is error `L0207`. Nothing ever hides anything, so a name means one thing wherever it is visible.
- The name `_` declares a binding that is never read. `constant _ = save_file()` runs the call and drops its value.

**Why there's no keyword-less form (`string b = a`):** with generic types it gets ambiguous. In `list[integer32] x = y`, the parser can't tell whether `list[integer32]` is a type or an indexing expression until it has looked past the closing bracket. C and C++ have this exact problem. Starting every declaration with a keyword makes the grammar parseable with one token of lookahead.

### 3.1 Copy semantics

Assigning a value to a new binding gives that binding an **independent value**. Changing one never changes the other.

The compiler doesn't have to copy bytes to give you this guarantee. If neither binding is mutated while both are alive, they share storage. Only an actual mutation forces a copy. Independence is part of the language's meaning; copying is an implementation detail.

## 4. Types

### 4.1 Primitive types

| Type | Meaning |
|---|---|
| `integer8` `integer16` `integer32` `integer64` | Signed integers |
| `unsigned8` `unsigned16` `unsigned32` `unsigned64` | Unsigned integers |
| `float32` `float64` | IEEE 754 floats |
| `boolean` | `true` or `false` |
| `string` | Immutable UTF-8 text |

Integer literals default to `integer64`. Float literals default to `float64`.

The names are whole words (Section 2.5), and a number type always states its size in bits. There is no plain `integer` or `float`, because a second spelling of the same type would break Goal 4.

Type rules:

- **No conversions behind your back.** A value never changes type by itself. Mixing `integer64` and `unsigned8` in one operation, passing one where the other is expected, or using a number as a condition is error `L0402`. Convert explicitly by calling the type, as in `unsigned8(count)` (Section 5.1). Unlike a cast in C or a conversion in Go, a conversion never cuts a value down to fit: if the value does not fit, the program stops with a runtime error. A conversion that cannot fail, such as `unsigned8` to `integer64`, costs nothing.
- **Literals take the type of their place.** An integer literal has no type of its own. It takes the type its context asks for, such as a declared type or the other side of an operator, and `integer64` when nothing asks. It has to fit (`L0404`). Arithmetic on literals is exact and happens while compiling, so `constant unsigned8 small = 300 - 100` is fine.
- **Text is printed through interpolation.** `print` takes a `string`. Other values are turned into text by interpolating them: `print("{count}")`.

| Operators | Work on | Result |
|---|---|---|
| `+` `-` `*` `/` `%` | two integers of the same type | that type |
| `-` in front of a value | a signed integer | the same type |
| `<` `<=` `>` `>=` | two integers of the same type | `boolean` |
| `==` `!=` | two integers, or two `boolean` values, of the same type | `boolean` |
| `and` `or` `not` | `boolean` values | `boolean` |

Any other use is error `L0405`. `and` and `or` only evaluate their right side when the left side does not decide the result already.

Division truncates toward zero, and a remainder takes the sign of the dividend: `-7 / 2` is `-3` and `-7 % 2` is `-1`. This matches C.

### 4.2 Integer overflow

Overflow is a runtime trap in every build mode, including `speed`. If you want wrapping, you have to ask for it with `wrapping_add`, `wrapping_multiply`, and so on. Rust wraps silently in release builds. Lymra doesn't, because a silent wrong number is a bug that compiles.

- Every `+`, `-`, `*`, `/` and `%` on integers is checked. Division by zero is checked too, and so is the one division whose result does not fit, the lowest value of a signed type divided by `-1`.
- When every value in an operation is known while compiling, the compiler does the check itself: an overflow is error `L0404`, and a division by a value that is always zero is error `L0406`.
- Otherwise a failed check stops the program with a runtime error (Section 11).
- The compiler leaves out a check when it can prove the check never fails, from the conditions around the operation, the bounds of `for` loops, and known values. Which checks it can prove only changes how fast a program runs, never what it does (Section 13).

### 4.3 Size classes

Every type belongs to exactly one size class. The class decides where a value can live (Section 7.1).

| Class | Size known | Examples |
|---|---|---|
| fixed | at compile time | `integer64`, `boolean`, `array[float32, 4]`, a type whose fields are all fixed |
| frozen | when the value is built, then never changes | `string`, a type with a `string` field |
| growable | changes while the value lives | `list[T]`, `stack[T]` |

A custom type's class is the largest class of its fields.

### 4.4 Collections

| Type | Class | Meaning |
|---|---|---|
| `array[T, N]` | fixed | N elements, N known at compile time |
| `list[T]` | growable | Grow-only. `push` adds to the end and returns a reference to the new element. |
| `stack[T]` | growable | Grow and shrink: `push` and `pop`. |
| `view[T]` | fixed | A read-only window into an array, list, or string. Does not own its data. |

`list[T]` never moves its elements, because it always sits on top of its data stack (Section 7.2) and grows in place. A reference to an element stays valid for as long as the list exists, no matter how many pushes come after it. C++ `std::vector` and Rust `Vec` reallocate when they grow, which is why they can't make this promise.

`stack[T]` can shrink, so a reference into it could point at a popped slot. Holding a reference into a `stack` past the next `pop` is error `L0702`.

### 4.5 Nested data stays inline

A value is stored as one contiguous block of bytes with no pointers inside it, however deeply it nests. A frozen field is stored in place, prefixed with its length.

```lymra
type Item
    string sku
    unsigned32 quantity

type Order
    unsigned64 id
    string customer
    list[Item] items
```

One `Order` with two items is a single block on a data stack. Reading a `list[Order]` streams through memory in order, which is what CPU caches are built for. The cost: a list whose elements have different sizes can be iterated but not indexed by position (Section 7.5).

Indexing starts at 0. Every index is bounds-checked. The compiler removes a check when it can prove the index is in range, for example `for index in range(0, length(items))`.

### 4.6 Custom types

```lymra
type Frog
    string name
    unsigned8 age
```

- Fields are stored inline. A `Frog` with a `string` field holds the string data in the same place, not a pointer to it elsewhere.
- Construction: `constant frog = Frog(name = "Kero", age = 3)`. All fields have to be given.

### 4.7 Choices (sum types)

A `choice` is a value that is exactly one of several variants. It is the only form of runtime polymorphism. There is no inheritance and there are no virtual tables.

```lymra
choice Shape
    Circle(float64 radius)
    Rect(float64 width, float64 height)
    Dot

function area(Shape shape) returns float64
    match shape
        case Circle(radius)
            return 3.14159 * radius * radius
        case Rect(width, height)
            return width * height
        case Dot
            return 0.0
```

- A `match` has to cover every variant. A missing variant is error `L0401`, and the message lists the missing cases.
- A plain `Shape` reserves room for its largest variant, so a `list[Shape]` can be indexed and its elements can be overwritten.
- A `packed Shape` takes exactly the size of the variant it holds. In the example above, a `list[packed Shape]` stores a `Dot` in 1 byte instead of 17. The trade is that a packed list can be iterated but not indexed, and a packed value can't change its variant after it is built.

## 5. Functions

```lymra
function area(float64 width, float64 height) returns float64
    return width * height

function main()
    print("{area(2.0, 3.0)}")
```

- `returns T` is required whenever the function returns a value. A function without `returns` returns nothing.
- Every path through a function with `returns` has to end in `return` and a value (`L0502`). Every `return` has to match the declaration (`L0503`).
- A call passes exactly one argument for each parameter, in order (`L0501`). There are no default values and no two functions with the same name.
- A call whose result is not used is error `L0505`. Drop a result on purpose with `constant _ = f()`.
- Functions are not values: there are no function pointers. Using a function's name without calling it, or calling a value, is error `L0507`.
- Parameters are immutable. Because the function can't modify them, the compiler is free to pass them by reference or by copy, whichever is faster for that type.
- A parameter declared `variable` can be modified, and the change is visible to the caller. The caller has to mark the argument too, so the mutation is visible where the call is written. Only a `variable` binding can be passed this way. A missing or extra mark is error `L0504`.
- An expression that passes a binding with `variable` cannot use that binding anywhere else, whether it reads it or passes it again (`L0508`). Whether the read or the change came first would be easy to misread, so such an expression has to be split into statements, which run in order. Each condition of an `if` and `else if` is its own expression.

```lymra
function add_item(variable list[integer32] items, integer32 value)
    push(variable items, value)

function main()
    variable list[integer32] numbers = list()
    add_item(variable numbers, 7)
```

- Return values are built directly in the caller's storage. Returning a `list` never copies it.
- `main` is the entry point. Top-level code can only contain declarations, so the order of program startup is never ambiguous.
- In edition 0.1, `main` takes no parameters and returns nothing (`L0506`). A program exits with status 0, or 70 when it stops on a runtime error.

### 5.1 Built-in functions

| Function | Does |
|---|---|
| `print(string text)` | Writes the text and a line break to standard output, as one whole line |
| `range(start, end)` | The integers from `start` up to `end`, without `end`. It is only used in a `for` loop for now. |
| `integer8(value)` to `integer64(value)` | Converts an integer to that signed type |
| `unsigned8(value)` to `unsigned64(value)` | Converts an integer to that unsigned type |

Calling an integer type converts a value to it. A conversion stops the program if the value does not fit the target type. When the value is known while compiling, a value that does not fit is error `L0404` instead. `boolean` and `string` convert nothing: compare a number to get a `boolean`, and interpolate it to get text.

The names of built-in functions and types cannot be declared again (`L0207`). No binding or function can be called `print`, `integer32`, `boolean` or `float64`.

## 6. Control flow

```lymra
if score > 90
    print("A")
else if score > 80
    print("B")
else
    print("C")

while count < 10
    count = count + 1

for item in items
    print("{item}")

for index in range(0, 10)
    print("{index}")
```

- A condition needs no parentheses, and nothing follows it on its line: no `:`, no `then`, no `{`.
- A condition is a `boolean`. Lymra has no "truthy" values: write `if count != 0`, not `if count` (`L0402`).
- `for name in range(start, end)` computes `start` and `end` once, then runs the block with `name` set to each integer from `start` up to `end`, without `end`. The loop variable cannot be changed. Name it `_` when the loop only repeats.
- `else if` continues a chain. There is no `elif`.
- `break` and `continue` work in `while` and `for`. Anywhere else they are error `L0601`.
- A statement after `return`, `break` or `continue`, or after an `if` whose every branch leaves, can never run. It is error `L0602`.
- `match` picks a block by the variant of a `choice` (Section 4.7).
- `return` ends the function, with a value when the function `returns` one.
- `assert condition` stops the program with a runtime error if the condition is false. The compiler will also use it as a fact when it removes bounds checks (Section 2.3).

## 7. Memory model

This section is the core of the language. Everything else depends on it.

### 7.1 Where memory lives

A Lymra program has exactly four places memory can live:

1. **Static data.** Top-level constants. Lives for the whole program.
2. **The call stack.** Fixed-size values: primitives, `array`, and types whose fields are all fixed-size.
3. **Data stacks.** Frozen and growable values (`string`, `list`, `stack`, and types that contain them). Each data stack is a large reserved range of virtual address space with a bump pointer. The compiler decides how many data stacks a program needs.
4. **Pools.** Slots for data whose lifetime isn't tied to a scope (Section 7.6).

There is no `malloc`, no `new`, and no `free`.

### 7.2 Data stacks

- The compiler assigns every growable value to a data stack. Only one growable value is live at the top of a given stack at a time.
- Because the growable value is always on top, growing it means moving the bump pointer forward. Its data never gets moved, and there's no capacity check against the other values on that stack.
- When a scope ends, all of its growable values are freed by resetting the bump pointer. That costs one store instruction, whether the scope held one integer list or a million nested orders. There are no destructors and nothing walks the data to free it.
- Each data stack is reserved as address space up front, and the operating system commits pages only when they are first touched. Unused reservation costs no physical memory.
- The end of each reservation is a guard page. A program that outgrows its data stack hits the guard page and stops with a clear error. It never overwrites other memory.
- The compiler assigns stacks statically: two growable values that are live at the same time never share a stack, and values with separate lifetimes reuse one. The runtime only tracks the bump pointers.

### 7.3 When memory is freed

Memory is freed when its scope ends. That timing is deterministic, and you can see it in the source code.

The compiler also frees a value **earlier**, at its last use, when that value is on the top of its data stack. When it isn't on top, the value is freed at scope end.

Why there's no "free every value right after its last use": data stacks free in last-in, first-out order. If `a` dies but `b` sits above it, the memory under `b` can't be reclaimed without a general purpose allocator, and Lymra doesn't have one. Freeing at the last use whenever possible is exactly what's achievable without a heap.

Values on the call stack (integers, floats, fixed arrays) aren't "freed" at all. They live in registers or the stack frame, and they cost nothing to free. A loop counter doesn't allocate anything.

### 7.4 References and roots

- A `view[T]`, or any other reference, has a **root**: the binding that owns the data it points to. The compiler infers the root. You never write it.
- A reference can't outlive its root. If it would (by being returned from the root's scope, or stored in something that outlives the root), the program is rejected with error `L0701`. The message names the root and the place the reference escapes to.
- Functions are specialized based on the roots of their arguments, which is why no lifetime annotations are needed.
- This is the only reference rule. There are no exclusivity rules: two mutable references to the same data may exist at once. That is safe here because memory never moves while a reference to it lives (Section 4.4) and nothing is freed while its root is in scope.

### 7.5 Known limitations of this model

These are the costs of having no heap. They are intentional.

- A recursive function can't keep a growable value alive across its own recursive call.
- A `list` of variable-size elements can be iterated but not indexed.
- No closures that escape their defining scope. No function pointers.
- Whole-program compilation only. No separately compiled libraries in 0.1.

### 7.6 Pools and generation-checked handles

For graphs, trees, and anything else whose lifetime isn't tied to a scope, use `pool[T]`:

```lymra
variable pool[Frog] frogs = pool()
constant handle = insert(variable frogs, Frog(name = "Kero", age = 3))
remove(variable frogs, handle)
get(frogs, handle)   # traps: the handle is stale
```

- A `handle[T]` is a slot index plus a generation counter.
- `remove` increments the slot's generation number.
- `get` compares the handle's generation against the slot's. If they don't match, the program traps.

Without the generation check, a stale handle would silently read whatever value got put into the slot afterward: the right type, the wrong data. Lymra spends one integer comparison per access to turn that silent bug into an immediate, reported failure.

### 7.7 Function arguments that are code

A function can take a block of code as an argument. The block is a compile-time value: it is never stored, never escapes, and every call to it is inlined.

```lymra
constant evens = filter(numbers) with number
    number % 2 == 0
```

- `with` is followed by the block's parameters, separated by commas. A block that takes none is written `with` alone.
- The value of the block is the value of its last line.
- The block belongs to the call written right before `with`, and that call has to be the whole expression of its statement. `1 + sum(numbers) with number` is error `L0307`: a reader could not tell whether the block goes to `sum` or to the sum.

This compiles to the same loop you would write by hand, and the result is built directly into `evens` (Section 5). There are no closure objects and no function pointers, so higher-order code costs nothing at runtime. The block can read the enclosing function's bindings because it can't outlive them.

### 7.8 Narrow links (planned for 0.2)

A normal reference is a full 8-byte address. A `link[T, unsigned32]` stores the same connection as a 4-byte offset from its own position to a target in the same list. It is checked like any other reference.

- A tree or graph built from links in a `list` takes half the memory of a pointer-based one.
- Offsets don't depend on where the list sits in memory, so the whole structure can be written to a file in one write and read back in one read.
- Loading bytes from outside goes through `from_bytes`, which checks every length, variant tag, and link before the data becomes a value. Corrupt or hostile input produces an error, never a wild reference.

## 8. Errors (open)

Lymra has no exceptions and no stack unwinding. 0.1 hasn't settled the design for recoverable errors. The candidates:

1. **`result[T, E]` plus a `try` operator.** Familiar from Rust. Every function in the call chain changes its return type and every call needs `try`.
2. **Return from an outer call.** `return error from load` ends the innermost active call of `load` with `error`, however many calls deep the current function is. The functions in between keep their plain signatures. The compiler checks that every call to the failing function happens inside a call to `load`, so nothing goes uncaught. It's implemented as a hidden flag each frame checks on return, not as an unwinder.

Option 2 removes the boilerplate that makes people skip error handling. It is also new, so it needs real programs written in it before it gets locked in. Section 12 tracks this.

## 9. Concurrency (planned, not in 0.1)

Threads share nothing. Each thread has its own data stacks. Threads communicate through typed queues of values that contain no references, so sending a value is a single memory copy. This gets rid of data races by construction, because there's no shared mutable memory in the first place.

## 10. Modules

```lymra
use math.geometry
use math.geometry.area
```

- One file is one module. `math/geometry.lym` is module `math.geometry`.
- `use a.b` imports module `b`, which you then refer to as `geometry.area(...)`. `use a.b.item` imports a single item.
- Declarations are private by default. Put `public` in front of one to export it.
- The `use geometry from math` form is rejected under Goal 4: it's a second way of writing the same import.

## 11. Compiler diagnostics

Every error follows this format:

```
error[L0201]: assignment to undeclared name `total`
  --> src/main.lym:12:5
   |
12 |     total = 0
   |     ^^^^^ `total` is not declared in this scope
   |
help: declare it first
   |
12 |     variable total = 0
```

Each error code has a page in [`docs/errors/`](errors/README.md) that explains the rule, why it exists, and how to fix the code. `lymra explain L0201` prints the same page.

- Every error carries a fix. Where the compiler can write the corrected line, it shows it.
- One mistake gets one error. After reporting, the compiler skips the broken statement and keeps checking the rest of the file, so one run shows every independent mistake.
- The first two digits of a code group it by topic:

| Codes | Topic |
|---|---|
| `L01xx` | Source text and layout: characters, comments, text, numbers, indentation, directives, naming |
| `L02xx` | Bindings |
| `L03xx` | Grammar |
| `L04xx` to `L10xx` | The chapter of this document the rule comes from: `L07xx` is the memory model |

A program that fails a check while it runs stops with a runtime error, in the same format without a code or a fix. The exit status is 70.

```
error: integer overflow
 --> src/main.lym:4:17
  |
4 |         level = level + 1
  |                 ^^^^^^^^^
```

The checks are integer overflow, division by zero, a conversion whose value does not fit, and `assert`. Calls that nest too deeply, as in recursion that never reaches its end, stop the program the same way with `error: stack overflow`, without a location: on POSIX systems and on Windows a handler catches the overflow when the stack runs out, so calls cost nothing extra for the check. Before the error, the program writes every line it printed. A line appears whole or not at all, so a program that stops in the middle of a `print` leaves no half line behind. Output is UTF-8 text with `\n` line breaks on every system; a Windows console, and a terminal that runs programs in a pseudo console, such as Windows Terminal, show it as the same characters. Output that cannot be written, for example to a full disk, ends the program with exit status 74.

## 12. Open questions

| ID | Question | Current lean |
|---|---|---|
| O1 | Recoverable error handling | Prototype both options in Section 8, then pick using real programs |
| O2 | Generics syntax for user functions | `function first[Item](view[Item] items) returns Item` |
| O3 | `match` on `packed` values: can a case bind a reference into the payload? | Yes, read-only |
| O5 | Integer type defaults on 32-bit targets | Undecided |
| O6 | Literal syntax for `array` and `list` values | `[1, 2, 3]` |
| O7 | Bitwise operations | Functions with whole-word names such as `bit_and` and `shift_left`, not symbols |
| O8 | Calling C: the syntax and rules of `external` declarations | `external function hypot(float64 x, float64 y) returns float64` |
| O10 | Reading the command line and choosing an exit status | Parameters of `main` and a `returns` type, once the needed types exist |
| O11 | Removing the overflow checks that stay. The compiler removes every check its interval analysis proves can never fail. Measured in `bench/results.md`: where that covers a whole loop, Lymra runs at C's speed, and a loop that GCC and Clang turn into a formula finishes in a millisecond instead of 0.6 seconds. The checks that stay, on variables a `while` loop changes and on results of calls, cost up to 1.7 times C's time, about what Rust with overflow checks costs. | Bound `while` loops by repeating the analysis until it settles, and carry facts about parameters and results across calls |

## 13. Where speed comes from

"Faster than C++" isn't something a language can promise. A C++ expert can always write the same machine code by hand. What a language can do is make the fast version the default version. Lymra does that in these ways:

| Technique | What it removes |
|---|---|
| Data stacks with bump pointers | Every `malloc` and `free` call, allocator locks, and fragmentation |
| Free on scope exit with one store | Destructor calls and walking a structure to free it |
| Inline nested data | Pointer chasing and cache misses |
| Grow-only lists that never move | Reallocation and copying, and the `reserve` calls written to avoid them |
| Building values in place | Temporaries and copies on return |
| `packed` choices | Padding every element up to the largest variant |
| Code arguments inlined at compile time | Indirect calls through closures or function pointers |
| Overflow-check elimination | Overflow and division checks that interval analysis proves can never fail |
| Bounds-check elimination and `#!prove bounds` | Checks the compiler can prove are redundant |
| Whole-program compilation to C | Cross-module call overhead, since everything can inline |

What Lymra pays for and C++ doesn't:

- Bounds checks the compiler can't remove.
- The overflow trap on every arithmetic operation (Section 4.2). On tight integer loops this is the largest cost, which is why it gets measured, not assumed.
- The generation check on pool access (Section 7.6).

## 14. Benchmarks

Speed claims are only allowed if they come with:

- A public benchmark suite in `bench/`, with the equivalent C and Rust programs next to each Lymra program.
- Published results for every benchmark, including the ones where Lymra loses.
- The compiler versions, flags, and hardware used.

`cargo run --release -p lymra-bench` builds each program as C, as Lymra with and without the optimizer, and as Rust with and without overflow checks. It checks that every version prints the same output and writes the timings to [`bench/results.md`](../bench/results.md). [`bench/README.md`](../bench/README.md) explains what the numbers show.
