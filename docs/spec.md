# Lymra Language Specification, Draft 0.1

Status: draft. Every rule here can change until a compiler implements it and a test proves it.

The compiler in `compiler/` implements the syntax of this whole document. Its test suite parses every code sample here, so the spec and the parser cannot drift apart. It also checks and compiles the core of the language: integers, floats, booleans, text built from values, functions, arrays, custom types, choices and `match`, lists and stacks of values that do not grow, views, pools and handles, `if`, `while`, `for`, and `print`. The rest arrives in the phases of the roadmap in the README, and until then the compiler rejects it with `L0309`, naming the phase.

## 1. Goals

Each goal has to be checkable. If a goal can't be measured, it's just a slogan.

1. **Memory safety without runtime machinery.** No garbage collector, no reference counting, no general purpose heap allocator, no manual `free`, no lifetime annotations. A program that compiles cannot read freed memory, read uninitialized memory, write out of bounds, or race on shared memory.
2. **Speed through layout, not tricks.** Data is stored inline and contiguously by default. Memory is released by moving a pointer, not by calling an allocator. Speed claims are made only against the public benchmark suite (Section 14), losses included.
3. **The compiler teaches.** Every rejected program gets an error code, the exact location, the reason, and at least one concrete fix.
4. **One way to write a thing.** If two spellings mean the same thing, one of them gets removed.
5. **Nothing out of reach.** A rule can ask for a program to be written another way, but never makes it impossible to write. Every error names a way that works, and every limit of the memory model has a safe way around it: a `pool` where lifetimes don't nest (Section 7.6), and calls into C for what Lymra cannot say yet (O8).

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

The compiler proves an index from what it knows at that point: the bounds of a `for` loop over `range`, the conditions of `if`, `while` and `assert`, the left side of `and` and `or`, and values known while compiling, carried through arithmetic. It follows each variable through a `while` loop until what it knows settles, a parameter holds the values every call in the program passes to it, and a call gives the values its function can return. A value read from an array can be any value of its type. A later compiler may prove more, never less, so a file that compiles with `#!prove bounds` keeps compiling.

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
true false external assert with fails fail try
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
- Parentheses group. Expressions, types and blocks may nest 64 levels deep, and one expression holds at most 1000 operators, calls, indexes and field accesses (`L0310`).

## 3. Bindings

```lymra
variable string name = "Cresmar"   # mutable, type written
variable count = 0                  # mutable, type inferred as integer64
constant string greeting = "Hi"     # immutable, type written
constant copy_of_name = name        # immutable, type inferred
```

- `variable` declares a mutable binding.
- `constant` declares an immutable binding. Changing a constant, a parameter or a loop variable is error `L0208`.
- A top-level `constant` is computed by the compiler, from literals, operators and other top-level constants. Anything only known at run time, such as a function call, is error `L0210`, and so is a constant that depends on itself, directly or through other constants and types. A top-level constant cannot hold an array or a value of a declared type either (`L0210`): they have no literal form yet (O6), so their parts could not be written one by one.
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

Integer literals default to `integer64`. Float literals default to `float64`. The defaults are the same on every target, 32-bit ones included, so a program computes the same numbers and overflows at the same point everywhere. `length` returns an `integer64`, and an index can be any integer type: a negative or too large index fails its bounds check.

The names are whole words (Section 2.5), and a number type always states its size in bits. There is no plain `integer` or `float`, because a second spelling of the same type would break Goal 4.

Type rules:

- **No conversions behind your back.** A value never changes type by itself. Mixing `integer64` and `unsigned8` in one operation, passing one where the other is expected, or using a number as a condition is error `L0402`. Convert explicitly by calling the type, as in `unsigned8(count)` (Section 5.1). Unlike a cast in C or a conversion in Go, a conversion never cuts a value down to fit: if the value does not fit, the program stops with a runtime error. A conversion that cannot fail, such as `unsigned8` to `integer64`, costs nothing.
- **Literals take the type of their place.** An integer literal has no type of its own. It takes the type its context asks for, such as a declared type or the other side of an operator, and `integer64` when nothing asks. It has to fit (`L0404`). Arithmetic on integer literals is exact and happens while compiling, so `constant unsigned8 small = 300 - 100` is fine. A float literal takes `float32` or `float64` the same way, `float64` when nothing asks, and is rounded once from its digits to the nearest float of that type. One beyond the largest float of its type, or one that is not zero but would round to zero, is `L0404`. An integer literal never becomes a float: write `2.0`.
- **Values become text through interpolation.** `print` takes a `string`. A number, a `boolean` or another text becomes part of a text when it is interpolated: `"{count} left"` is a `string` like any other, which a binding keeps, a function returns and a type holds (Section 4.5).

| Operators | Work on | Result |
|---|---|---|
| `+` `-` `*` `/` | two numbers of the same type | that type |
| `%` | two integers of the same type | that type |
| `-` in front of a value | a signed integer or a float | the same type |
| `<` `<=` `>` `>=` | two numbers of the same type, or two `string` values | `boolean` |
| `==` `!=` | two numbers of the same type, two `boolean` values, or two `string` values | `boolean` |
| `and` `or` `not` | `boolean` values | `boolean` |

Text compares by its UTF-8 bytes, one at a time, which is the order of the characters' code points: `"apple" < "banana"`, every capital letter comes before every small one, and a text that starts another comes first, as `"app" < "apple"`. The order is the same on every system, whatever its language settings.

Any other use is error `L0405`. `and` and `or` only evaluate their right side when the left side does not decide the result already.

The parts of an expression run from left to right: the operands of an operator, the arguments of a call, the fields of a construction and the values in a text, each finished before the next one starts and before the operation itself. When two parts call functions that print, their lines come out in that order, and when two checks could fail, the one further left stops the program. Every C compiler builds the same order. In an assignment, the indexes of the target are computed and checked first, from the binding inward, then the value, and the value is stored last.

Division truncates toward zero, and a remainder takes the sign of the dividend: `-7 / 2` is `-3` and `-7 % 2` is `-1`. This matches C.

Floats follow IEEE 754. Every operation is rounded to the nearest float of its type, ties to even, and to nothing wider: the compiler keeps C compilers from computing in 80 bits on 32-bit x86 and from fusing a multiply and an add into one rounding. So a program computes the same floats on every system. Values known while compiling are computed the same way, so `0.1 + 0.2` is `0.30000000000000004` whether it is computed now or when the program runs. An operation that overflows gives infinity and `0.0 / 0.0` gives NaN, values a float can hold, so float arithmetic never stops the program; only a result known while compiling that would be infinity or NaN is an error (`L0404`, or `L0406` for a division by zero). NaN is unequal to every value, itself included, and every comparison with it but `!=` is false.

Converting with `float32(...)` or `float64(...)` turns an integer into the nearest float and a `float32` into the same `float64`. A `float64` becomes the nearest `float32`, and one beyond the largest `float32` stops the program. A float converted to an integer type loses its fraction, rounding toward zero as in C, and the program stops if the whole part does not fit the type or the float is NaN, where C would make up a number.

A float is written as the shortest decimal that reads back as the same float, and of two as close, the one whose last digit is even: `0.1`, `0.30000000000000004`, `100.0`. Between 0.0001 and 1e16 it is written plainly, with a digit on each side of the point; outside that it takes an exponent, as `1.0e16` or `2.5e-7`. Either way it is a Lymra float literal for the same float. The others are written `infinity`, `-infinity`, `NaN` and `-0.0`. Lymra's runtime finds the digits itself, with the Schubfach algorithm, so no C library can write them differently.

### 4.2 Integer overflow

Integer overflow is a runtime trap in every build mode, including `speed`. If you want wrapping, you have to ask for it with `wrapping_add`, `wrapping_multiply`, and so on. Rust wraps silently in release builds. Lymra doesn't, because a silent wrong number is a bug that compiles.

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
| growable | changes while the value lives | `list[T]`, `stack[T]`, `pool[T]` |

A custom type's class is the largest class of its fields.

### 4.4 Collections

| Type | Class | Meaning |
|---|---|---|
| `array[T, N]` | fixed | N elements. N is a number, or a top-level integer constant, of at least 1 (`L0409`). |
| `list[T]` | growable | Grow-only. `push` adds to the end. |
| `stack[T]` | growable | Grow and shrink: `push` and `pop`. |
| `view[T]` | fixed | A read-only window into an array, list, or string. Does not own its data. |
| `pool[T]` | growable | Values that come and go in any order, each named by a `handle[T]` (Section 7.6). |

`list[T]` never moves its elements, because it always sits on top of its data stack (Section 7.2) and grows in place. A reference to an element stays valid for as long as the list exists, no matter how many pushes come after it. C++ `std::vector` and Rust `Vec` reallocate when they grow, which is why they can't make this promise.

A list is a value too. `list()` starts an empty one, its type taken from where it goes (`L0412`), and `push(variable items, value)` adds an element at the end. A copy is independent, a `variable` parameter changes the caller's list, and a function can return one. `items = new_value` replaces the whole list (Section 7.9). An index works as it does for an array, but a list's length is only known when the program runs, so an index past the end always stops the program then. In 0.1 a list holds values that do not grow: numbers, booleans, text, arrays, handles, and custom types and choices made of them. It keeps the text of its elements beside them (Section 4.5). A list inside another value comes later in phase 3.

```lymra
function squares(integer64 count) returns list[integer64]
    variable list[integer64] result = list()
    for number in range(0, count)
        push(variable result, number * number)
    return result

function main()
    variable values = squares(4)
    push(variable values, 100)
    print("{length(values)} {values[3]} {values[4]}")
```

A stack is a list that can also shrink. `stack()` starts an empty one, `push(variable items, value)` adds an element on top, `pop(variable items)` takes the top element off and returns it, and `top(items)` returns it and leaves it in place. Either stops the program when the stack is empty. Everything else works as it does for a list: an index counts from the bottom, `length` gives the number of elements, a `for` loop and a view go from the bottom up, and a copy is independent. A `pop` frees the top element, so a view of the stack, or a `for` loop over it, could read a slot that no longer holds anything: popping while one is still used is error `L0702`.

```lymra
function main()
    variable stack[integer64] pending = stack()
    push(variable pending, 1)
    push(variable pending, 2)
    while length(pending) > 0
        constant next = pop(variable pending)
        print("{next}")
```

A view reads elements that an array, a list, a stack or a text keeps, and copies none of them. An array, a list or a stack goes wherever a view of its elements is expected, so one function that takes a `view[integer64]` works on arrays of any length, on lists and on stacks. `slice(items, start, end)` shows part of an array, a list, a stack or a view, and `bytes(text)` shows the bytes of a text. A view reads the elements as they are when it reads them; nothing changes through it (`L0208`). Its length is the one it had when it was made: a `push` adds to the list behind it, not to the view. A view can't outlive what it shows (`L0701`), a list can't be replaced while a view of it is still used (`L0703`), and a stack can't be popped then (`L0702`); Section 7.4 has the rules. In 0.1 a view lives in a binding, a parameter or a function's result, not inside another value.

```lymra
function total(view[integer64] values) returns integer64
    variable integer64 sum = 0
    for value in values
        sum = sum + value
    return sum

function main()
    constant array[integer64, 3] fixed = array(2)
    variable list[integer64] growing = list()
    push(variable growing, 5)
    push(variable growing, 7)
    print("{total(fixed)} {total(growing)} {total(slice(growing, 1, 2))}")
```

An array is a value, like a number. Assigning it to another binding or passing it to a function gives an independent copy (Section 3.1); the compiler passes an array parameter by address, which no one can tell apart, since the function cannot change it. A `variable` parameter changes the caller's array. An array lives on the call stack, so one array takes at most 1 MiB, its length times the size of an element (`L0410`): a call stack holds a few MiB in all. Larger data belongs in a `list`. Arrays cannot be compared with `==` or put into text; compare or print their elements.

```lymra
function main()
    variable array[integer32, 4] scores = array(0)
    scores[1] = 7
    variable copy = scores
    copy[1] = 9
    print("{scores[1]} {copy[1]} {length(scores)}")
```

### 4.5 Nested data stays inline

A value is stored as one block of bytes, its fields in place, however deeply it nests. Text is the one part kept beside it: a field that holds text holds where the text starts and how many bytes it has, 16 bytes on a 64-bit system, and the bytes lie on a data stack that whatever holds the value keeps (Section 7.2), or in the program itself for text written in it. Every value of a type therefore has the same size, so `people[index]` costs one read, as it does for numbers.

```lymra
type Person
    string name
    integer32 age

function main()
    variable list[Person] people = list()
    push(variable people, Person(name = "Ana", age = 42))
    push(variable people, Person(name = "{people[0].name} junior", age = 7))
    people[1].age = 8
    for person in people
        print("{person.name} is {person.age}")
```

- **The text a value holds is its own.** Storing text that something else keeps, such as another binding's text, a field of another value or a slice, copies its bytes to where the new owner keeps its text, so changing one never changes the other. Text built where it is stored, by interpolation or by a call that returns it, goes straight there, and text written in the program is never copied: it lasts as long as the program.
- **A list keeps its elements' text beside them, in order.** A list or a stack whose elements hold text keeps that text on a second data stack, one element's text after another, in the order of the elements. Iterating streams through both in order, which is what CPU caches are built for. `push` copies the new element's text after the text of the element before it, and `pop` frees the text of the top element.
- **An element of a list keeps the text it was given.** Its numbers and booleans can change, but new text in it is error `L0407`: the text would have to go after the text of the last element, out of order. Build the list again with the element changed and replace the old one, which frees all of its text in one step.
- **Parameters read text in place.** A parameter reads the text it is given without copying it. A `variable` parameter that holds text also gets the data stack its caller keeps that text on, so new text it gets replaces the caller's in place.
- **A returned text goes where the caller keeps it.** A function that returns text builds it on a data stack of its own, frees everything else it built, and copies the text where its caller keeps the result. When the function has nothing to free at that point, the text goes there straight away, and text written in the program is never copied.

Lists inside other values come later in phase 3. A value held directly by a binding will keep each of its lists on a data stack of its own, so each can grow (Section 7.2); inside another value, such as a list, a value's lists will no longer grow.

Indexing starts at 0, and an index can have any integer type. Every index is bounds-checked. One known while compiling that falls outside its array is error `L0411`; any other stops the program when it falls outside, with an error that gives the index and the length. The compiler removes a check when it can prove the index is in range, for example `for index in range(0, length(items))`.

### 4.6 Custom types

```lymra
type Frog
    string name
    unsigned8 age

type Pond
    array[Frog, 3] frogs
    integer64 visits

function main()
    variable pond = Pond(visits = 0, frogs = array(Frog(name = "Kero", age = 3)))
    pond.frogs[1].age = 4
    pond.visits = pond.visits + 1
    print("{pond.frogs[1].name} is {pond.frogs[1].age}")
```

- A `type` declaration lists its fields, a type and a name on each line. Declarations can come in any order, and a field can hold an array or another declared type. Field names follow the rules for names (Section 2.6), and a name used twice is error `L0207`.
- A construction gives every field once, by name, in any order: `Frog(name = "Kero", age = 3)`. A field left out, given twice or given without its name is error `L0414`. Lymra never fills in a field by itself, so no field can be read before something was put there. The values run in the order they are written (Section 4.1).
- `frog.age` reads a field, and `frog.age = 4` changes one when the binding is a `variable`; fields and elements chain, as in `pond.frogs[1].age`. A field the type does not have is error `L0413`, and the message suggests the closest one.
- A value of a declared type is a value like a number (Section 3.1). Assigning or passing it gives an independent copy, and a `variable` parameter changes the caller's. Like an array, it lives on the call stack and takes at most 1 MiB (`L0410`).
- Fields are stored inline, in the order they are declared, and a `string` field holds where its text lies (Section 4.5). A type that holds itself, directly or through other types, is error `L0415`: a value of it would have no end. A field that refers to another value of the same type holds that value's index in an array instead.
- Values of a declared type cannot be compared with `==` or put into text yet; compare or print their fields.

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

- A variant is built like a value of a declared type, with every field named once (Section 4.6): `Circle(radius = 2.0)`. A variant without fields is written by its name alone, as `Dot`. Variant names share the top level of a file with types, functions and constants, so each is declared once (`L0207`).
- A `case` names one variant and the fields it uses, each bound to a constant for the body of the case: `case Rect(width, height)`. A case can leave out fields it does not use. Where a field's name is taken, the field is bound under another, as in `case Rect(width = rect_width)`, since one name never hides another (Section 3).
- A `match` has to cover every variant, once. A missing variant, a variant handled twice, or a case that names no variant of the choice is error `L0401`, and the message lists the missing cases. There is no case for every other variant, like `_` in Rust or `default` in C: adding a variant to a choice then shows every `match` that has to handle it.
- A choice is a value like a number (Section 3.1). Assigning or passing it gives an independent copy, and a `variable` parameter changes the caller's. A choice whose variants hold no fields is compared with `==` and `!=`; other choices are told apart in a `match`. A choice cannot be put into text yet.
- A plain `Shape` reserves room for its largest variant, so a `list[Shape]` can be indexed and its elements can be overwritten.
- A `packed Shape` takes exactly the size of the variant it holds. In the example above, a `list[packed Shape]` stores a `Dot` in 1 byte instead of the 24 a plain `Shape` takes: a tag, then room for a `Rect`'s two `float64` fields at their alignment. The trade is that a packed list can be iterated but not indexed (`L0408`), since it keeps no element positions, and a packed value can't change its variant after it is built. `packed` comes with lists, in roadmap phase 3.

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

- Return values are built directly in the caller's storage. Returning a `list` never copies it. When the call still reads that storage through an argument, as in `board = step(board)`, the value is built in a temporary first and then copied, so the call never reads its own half-built result.
- `main` is the entry point. Top-level code can only contain declarations, so the order of program startup is never ambiguous.
- `main` takes nothing, or the command-line arguments as a `view[string]`, and returns nothing or an `unsigned8` exit status (`L0506`). It can also fail with text (Section 8). The arguments leave out the program's own name and reach the program as UTF-8 text on every system, Windows included; an argument that is not UTF-8 stops the program before `main` runs, with exit status 64.

```lymra
function main(view[string] arguments) returns unsigned8
    if length(arguments) == 0
        print("usage: greet <name>")
        return 64
    for name in arguments
        print("hello {name}")
    return 0
```

- A program exits with the status `main` returns, 0 when it returns none, 1 when `main` fails, 70 when it stops on a runtime error, and 74 when its output cannot be written.

### 5.1 Built-in functions

| Function | Does |
|---|---|
| `print(string text)` | Writes the text and a line break to standard output, as one whole line. The values in the text are computed first, so a call among them that prints writes its own line before this one. |
| `range(start, end)` | The integers from `start` up to `end`, without `end`. It is only used in a `for` loop for now. |
| `length(items)` | The number of elements of an array, a list, a stack or a view, or of bytes in a `string`, as an `integer64`. An array's length is part of its type, so it is known while compiling. |
| `array(value)` | An array with every element set to `value`. Its type, and so its length, comes from the place it goes: a declared type, a parameter, or a function's return type (`L0412`). |
| `list()` | An empty list. Its type comes from the place it goes, as for `array(value)` (`L0412`). |
| `push(variable items, value)` | Adds `value` to the end of a list, or on top of a stack. Elements never move, so nothing that reads the list sees it change but for its new end. |
| `stack()` | An empty stack. Its type comes from the place it goes, as for `list()` (`L0412`). |
| `pop(variable items)` | Takes the top element off a stack and returns it. It stops the program when the stack is empty. |
| `top(items)` | The top element of a stack, left in place. It stops the program when the stack is empty. |
| `pool()` | An empty pool. Its type comes from the place it goes, as for `list()` (`L0412`). |
| `insert(variable items, value)` | Stores `value` in a free slot of a pool and returns its handle. A slot that `remove` freed takes the next value, under a new generation. |
| `remove(variable items, handle)` | Takes the value a handle names out of its pool. The handle is stale from then on: using it, here or anywhere, stops the program. |
| `slice(items, start, end)` | The elements of an array, a list, a stack or a view from `start` up to `end`, without `end`, as a view, with nothing copied. A slice of a `string` is a `string` that shares its bytes; its ends have to fall between characters. Each position is checked as soon as it is computed: the start lies from 0 to the length, the end from the start to the length. |
| `bytes(text)` | The UTF-8 bytes of a `string`, as a `view[unsigned8]` |
| `wrapping_add(left, right)`, `wrapping_subtract`, `wrapping_multiply` | Arithmetic that wraps around the type instead of stopping the program: the exact result, keeping only as many low bits as the type has, read as two's complement for a signed type. The operands follow the rules of `+`. |
| `integer8(value)` to `integer64(value)` | Converts an integer to that signed type |
| `unsigned8(value)` to `unsigned64(value)` | Converts an integer to that unsigned type |

Calling an integer type converts a value to it. A conversion stops the program if the value does not fit the target type. When the value is known while compiling, a value that does not fit is error `L0404` instead. `boolean` and `string` convert nothing: compare a number to get a `boolean`, and interpolate it to get text.

Later phases add these, and until then the compiler rejects each with `L0309`:

| Function | Does | Phase |
|---|---|---|
| `filter(items) with name`, `sort(variable items) with left, right` | The elements the block accepts; the elements in the block's order (Section 7.7) | 3 |

The names of built-in functions and types cannot be declared again (`L0207`). No binding or function can be called `print`, `length`, `wrapping_add`, `integer32`, `boolean`, `array` or `float64`.

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
- `for name in range(start, end)` computes `start`, then `end`, each once, then runs the block with `name` set to each integer from `start` up to `end`, without `end`. The loop variable cannot be changed. Name it `_` when the loop only repeats.
- `for name in items` over an array, a list, a stack or a view runs the block once for each element, in order, with `name` set to a copy of it. The array, list, stack or view is computed once, and each element is read when its turn comes, so a change the block makes to a later element is seen. A list is walked over the elements it holds when the loop starts, so the block can `push` to it without making the loop longer; replacing the list, or passing it with `variable` to a function that can replace it, would free the elements the loop reads, and is error `L0703`. A stack is walked from the bottom up the same way; popping it, or passing it with `variable` to a function that can pop it, would free an element the loop is still to read, and is error `L0702`. A view binding the block gives another view leaves the loop going over the first one.
- `else if` continues a chain. There is no `elif`.
- `break` and `continue` work in `while` and `for`. Anywhere else they are error `L0601`.
- A statement after `return`, `break` or `continue`, or after an `if` whose every branch leaves, can never run. It is error `L0602`.
- `match` picks a block by the variant of a `choice` (Section 4.7).
- `return` ends the function, with a value when the function `returns` one.
- `assert condition` stops the program with a runtime error if the condition is false. The compiler also uses it as a fact when it removes checks (Section 2.3).

## 7. Memory model

This section is the core of the language. Everything else depends on it. [The memory model study](memory/README.md) tests it against twelve programs; the rules below include what that study changed.

### 7.1 Where memory lives

A Lymra program has exactly four places memory can live:

1. **Static data.** Top-level constants and the text written in the program. Lives for the whole program.
2. **The call stack.** Values whose size is fixed: numbers, booleans, arrays, handles, and types and choices made of them. A text, a list or a stack keeps where its bytes lie here, and the bytes on a data stack.
3. **Data stacks.** The bytes of text the program builds while it runs, and the elements of lists, stacks and pools. Each data stack is a large reserved range of virtual address space with a bump pointer. The compiler decides how many data stacks a program needs.
4. **Pools.** Slots for data whose lifetime isn't tied to a scope (Section 7.6).

There is no `malloc`, no `new`, and no `free`.

### 7.2 Data stacks

- The compiler assigns every growable value, and the text of every binding that holds text, to a data stack. Only one of them is live at the top of a given stack at a time.
- Because the growable value is always on top, growing it means moving the bump pointer forward. Its data never gets moved, and there's no capacity check against the other values on that stack.
- When a scope ends, all of its growable values are freed by resetting the bump pointer. That costs one store instruction, whether the scope held one integer list or a million nested orders. There are no destructors and nothing walks the data to free it.
- Each data stack is reserved as address space up front, and the operating system commits pages only when they are first touched. Unused reservation costs no physical memory.
- A value that would grow past the end of its reservation stops the program with a clear error before it touches anything beyond it. It never overwrites other memory.
- A growable value grows only while it is on top of its data stack. A `push` or a `pop` on a value that another value may sit above at that point is error `L0704`, and the message names the value above it. New text for a binding is the same: it goes where the binding's text lies, so the binding has to be on top. Within one call, a function's lists never share a stack, so this comes up in recursion: a call that holds its own copy of a binding cannot change the copy of an earlier call that reached it through a `variable` parameter, since its own copy sits above it. The compiler follows where each list and text can travel through calls, so it reports only one that really can be under a newer one. The runtime checks it again before every `push`, every `pop` and all new text, as a second line of defense.
- The compiler assigns stacks statically: two growable values that are live at the same time never share a stack, and values with separate lifetimes reuse one. The copies of one binding at different depths of a recursion share a data stack, each above the copy of the call before it; a call frees its own before it returns, so the one below is on top again when it grows. A list or a stack whose elements hold text keeps that text on a data stack of its own (Section 4.5). The runtime only tracks the bump pointers.
- A data stack reserves address space, not memory. On a 64-bit system each reserves up to 64 GiB. On a 32-bit system, where a process has 2 to 3 GiB in all, a program's data stacks share 1 GiB. The runtime reserves one region for all of them, halving its request until the system grants it, and splits it evenly. A value that outgrows its data stack stops the program with `error: data stack full`.

### 7.3 When memory is freed

Memory is freed when its scope ends. That timing is deterministic, and you can see it in the source code.

The compiler can also free a value **earlier**, at its last use, when that value is on the top of its data stack. When it isn't on top, the value is freed at scope end. The 0.1 compiler frees every value at the end of its scope, and a list or a text a statement computes without a binding, such as the argument in `total(squares(4))` or the text in `print(shouted(name))`, at the end of that statement.

Why there's no "free every value right after its last use": data stacks free in last-in, first-out order. If `a` dies but `b` sits above it, the memory under `b` can't be reclaimed without a general purpose allocator, and Lymra doesn't have one. Freeing at the last use whenever possible is exactly what's achievable without a heap.

Values on the call stack (integers, floats, fixed arrays) aren't "freed" at all. They live in registers or the stack frame, and they cost nothing to free. A loop counter doesn't allocate anything.

### 7.4 References and roots

- A `view[T]`, or any other reference, has a **root**: what keeps the data it points to. That is a binding of the function, a parameter, whose root the caller keeps alive through the call, text in the program itself, or a value a statement computes without a binding, which it frees when it ends. The compiler infers the roots of every reference, from everything it is given anywhere in its function. You never write them.
- A reference can't outlive its root, and the program is rejected with error `L0701` where it would: a binding that holds a reference to a value its statement frees, or to a binding of a block that ends before the reference's; a function that returns a reference to one of its own bindings; or a `variable` reference parameter given a reference to a binding of the function, which the caller would keep. The message names the root.
- A function that returns a reference returns one rooted in some of its parameters, and the compiler works out which ones across the whole program. A call's result then has the roots of the arguments it passed there. That is why no lifetime annotations are needed.
- A value can't be replaced while a reference into it is still used, when the replacement could change what its bytes mean: assigning a choice, which can change its variant, assigning a growable value or a text, whose storage is released (Section 7.9), giving new text to a part of a value, which rewrites all of its text, or removing a pool slot. Each is error `L0703`, and so is passing the value with `variable` to a function that can replace it. A reference is still used when a later statement can read it, or a `for` loop around the replacement goes over it. The message names the reference and where it was made. A `pop` frees the top element of a stack, so popping a stack while a reference into it is still used, or a `for` loop goes over it, is the same mistake: error `L0702`, and so is passing the stack with `variable` to a function that can pop it.
- These are the only reference rules. There are no exclusivity rules: two mutable references to the same data may exist at once. That is safe because memory never moves while a reference to it lives (Section 4.4), nothing is freed while its root is in scope, and no value changes layout under a reference. Threads share nothing (Section 9), so two references never race.

### 7.5 Known limitations of this model

These are the costs of having no heap. They are intentional.

- A value can't grow while another value sits above it on its data stack (`L0704`). Nested lists are built in order: an earlier element's list can't take an item that arrives later. Group the input first, or keep the items in one flat list, as `docs/memory/routes.lym` does. A recursive function adds to the one list it was given, or returns a new list, rather than filling a list of an earlier call while holding one of its own.
- An element of a list or a stack can't get new text (`L0407`); the list is built again instead. A `packed` list can't be indexed (`L0408`).
- No closures that escape their defining scope. No function pointers.
- In 0.1, a view can't be stored inside another value: an array, a list, or a declared type. Values keep positions into a list instead, as `docs/memory/tokens.lym` does.
- Whole-program compilation only. No separately compiled libraries in 0.1.

### 7.6 Pools and generation-checked handles

For graphs, trees, and anything else whose lifetime isn't tied to a scope, use `pool[T]`:

```lymra
variable pool[Creature] creatures = pool()
constant handle = insert(variable creatures, Creature(position = 0, health = 3))
creatures[handle].health = 2
remove(variable creatures, handle)
print("{creatures[handle].health}")   # traps: the handle is stale
```

- A `handle[T]` is a slot index plus a generation counter. Its root is the pool it came from (Section 7.4): using it with another pool is error `L0705`, and keeping it after its pool ends is `L0701`.
- `creatures[handle]` reads or changes a slot, as `items[index]` does for a list. Every access checks that the slot exists and that the handle's generation matches the slot's. If not, the program traps.
- `remove` increments the slot's generation number. Removing a slot while a reference into it is still used is error `L0703`. A slot whose generation would wrap around is never used again, so no old handle can ever match it.
- A pool holds fixed-size values, so every slot has the same size (`L0706`). Text that belongs to pool values goes in a list, and the values keep positions into it.
- Two handles are equal (`==`) when they name the same value: the same slot in the same generation. Handles have no order.
- The compiler follows every handle from the `insert` that made it, through bindings, lists, the fields of other values and calls. A function that takes a pool and a handle can't know where the handle came from, so it requires that the handle comes from that pool, and every call is checked: error `L0705` points at the call that breaks it.
- A pool grows on a data stack like a list (Section 7.2): `insert` into a pool that a newer one may sit above is `L0704`. Replacing a pool, or passing it with `variable` to a function that can replace it, while a handle into it is still used or a `for` loop goes over handles into it, is `L0703`: the new pool's slots start over at the first generation, so an old handle could match one. A pool that is copied or returned keeps the handles in its values; they name the slots of the new pool.
- A type can hold handles to values of its own type, as the nodes of a tree do: a handle holds a slot and a generation, never the value. There is no empty handle. A link that can be missing is a choice:

```lymra
type Node
    integer64 value
    Link next

choice Link
    End
    To(handle[Node] node)
```

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

### 7.9 Replacing a growable value or text

`items = new_value` frees the old value and puts the new one in its place. Text works the same way, and so does new text for a part of a value, which gathers all of the value's text again where it started.

- When the new value reads the old one, itself or through a view, as in `slots = grown(slots)` or `name = "{name}!"`, both are alive while it is built. The new value is built right above the old one and then moved down into its place with one copy.
- When it isn't, as in `pending = list()`, the old value is freed first and the new one is built where it was, with no copy.
- No reference into the old value can still be used at that point (`L0703`).

## 8. Errors

Lymra has no exceptions and no stack unwinding. A function that can fail says so in its declaration, a failure is a value, and every call to such a function handles the failure or passes it on where the reader can see it.

```lymra
choice ParseError
    Empty
    TooLong
    NotADigit(integer64 position)

function digits(string text) returns integer64 fails ParseError
    if length(text) == 0
        fail Empty
    variable integer64 value = 0
    variable integer64 position = 0
    for byte in bytes(text)
        if byte < 48 or byte > 57
            fail NotADigit(position = position)
        if position == 18
            fail TooLong
        value = value * 10 + integer64(byte - 48)
        position = position + 1
    return value

function sum(string left, string right) returns integer64 fails ParseError
    return try digits(left) + try digits(right)

function main() fails string
    match sum("12", "3x")
        case Success(value)
            print("{value}")
        case Failure(error)
            match error
                case NotADigit(position)
                    print("not a digit at {position}")
                case Empty
                    print("nothing to add")
                case TooLong
                    print("too many digits")
    constant total = try sum("4", "5") else fail "could not add 4 and 5"
    print("{total}")
```

- **`fails` names what can go wrong.** `fails ParseError` after the parameters and the `returns` type says the function can fail with a value of that type. The value is anything that does not grow and holds no view or handle: a number, a `boolean`, text, or an array, a declared type or a choice made of them (`L0802`). In 0.1 a function that can fail returns a value of that kind too; one that returns a list comes with lists inside other values.
- **`fail value` ends the function with a failure,** as `return` ends it with a value, and frees what the function built. The value has the type after `fails` (`L0402`), and `fail` in a function that cannot fail is error `L0801`.
- **A call gives a result.** A call to a function that can fail gives `Success`, with the field `value` that the function returned, or `Failure`, with the field `error` that it failed with. A function that returns nothing gives `Success` with no field. A result is used in one of three ways:
  - `match` handles both cases. `case Success(value)` and `case Failure(error)` bind the fields as for any choice (Section 4.7), and a match covers both (`L0401`).
  - `try call` gives the value of a `Success`. On a `Failure` the function the `try` is in fails at once with the same error, freeing what it built, and nothing after the `try` runs. That function has to fail with the same type, since a value never changes type by itself (`L0801`). `try` applies to the call right after it, which has to be a call to a function that can fail (`L0803`), and can stand wherever a value can. The parts of an expression run from left to right (Section 4.1), so in `try digits(left) + try digits(right)`, `right` is never read when `left` fails.
  - `try call else fail value` gives the value of a `Success` and fails with `value` on a `Failure`, which turns another function's error into this function's. It is the whole value of its statement: a declaration, an assignment, a `return` or a statement of its own (`L0805`).
- **No failure goes unnoticed.** A result can be kept in a binding and matched later, but a result that is never used is error `L0505`, as for any value.
- **`main` can fail with text.** `function main() fails string` ends the program by writing `error: ` and the text to standard error, after every line it printed, with exit status 1 (`L0804` for any other type).

A result is a choice: a tag beside the value or the error, returned like any other value. Checking it after a call is one comparison and a branch the processor predicts, and there are no unwinding tables, no hidden flags and no allocation.

**Why not exceptions:** an exception can leave any call, so a reader cannot tell which lines a function can stop at, and every language with exceptions has had to add `finally`, destructors or `using` to free what was built on the way. Lymra frees on every exit already, and a `try` marks each place a function can end early. **Why not a return from an outer call**, which would let the functions in between keep plain signatures: a reader of one of them could not see that a call ends it, which is the hidden control flow `L0508` exists to rule out.

## 9. Concurrency (planned, not in 0.1)

Threads share nothing. Each thread has its own data stacks and its own pools. Threads communicate through typed queues of values that contain no references, and a handle counts as one, so sending a value is a single memory copy. This gets rid of data races by construction, because there's no shared mutable memory in the first place.

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

The checks are integer overflow, division by zero, a conversion whose value does not fit, an index outside its array or list, a list or a text that outgrows its data stack, and `assert`. Calls that nest too deeply, as in recursion that never reaches its end, stop the program the same way with `error: stack overflow`, without a location: on POSIX systems and on Windows a handler catches the overflow when the stack runs out, so calls cost nothing extra for the check. Every program gets a stack of 8 MiB on Windows as well, the default of Linux and macOS, and the arrays of every call in progress share it. Where the C compiler can, a function with a large frame touches its stack a page at a time, so it cannot step over the guard page that ends the stack. Before the error, the program writes every line it printed. A line appears whole or not at all, so a program that stops in the middle of a `print` leaves no half line behind. Output is UTF-8 text with `\n` line breaks on every system; a Windows console, and a terminal that runs programs in a pseudo console, such as Windows Terminal, show it as the same characters. Output that cannot be written, for example to a full disk, ends the program with exit status 74.

## 12. Open questions

| ID | Question | Current lean |
|---|---|---|
| O2 | Generics syntax for user functions | `function first[Item](view[Item] items) returns Item` |
| O3 | `match` on `packed` values: can a case bind a reference into the payload? | Yes, read-only |
| O6 | Literal syntax for `array` and `list` values | `[1, 2, 3]` |
| O7 | Bitwise operations | Functions with whole-word names such as `bit_and` and `shift_left`, not symbols |
| O8 | Calling C: the syntax and rules of `external` declarations | `external function hypot(float64 x, float64 y) returns float64` |
| O12 | Reading files and standard input. Every program in the memory model study had to take its input from constants. | Built-in functions that fail with a built-in choice (Section 8) |
| O11 | Removing the overflow checks that stay. The compiler removes every check its interval analysis proves can never fail, with `while` loops worked out to a fixed point and what parameters receive and calls return carried across the program. Measured in `bench/results.md`: four of the six benchmarks keep no check and, built with GCC, run within 1% of C's time. The checks that stay guard values no interval bounds, such as a Collatz chain or a sum of recursive results; a check against a constant compares the other operand with a limit, so the C compiler keeps the arithmetic plain, and `collatz` built with GCC takes 1.12 times C's time. | Relations between variables, such as a result that grows with an argument, would bound some of the rest. Worth it once a real program needs one |

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
