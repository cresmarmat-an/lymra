# Memory model study

Section 7 of the [specification](../spec.md) is the core of Lymra: memory safety without a garbage collector, reference counting or a borrow checker. None of it is implemented yet, and phase 2 is about to fix how arrays and custom types are laid out. Before that happens, this study tests the model against programs people actually write, while changing it still costs only text.

## Method

Twelve programs, written strictly in Lymra as the spec defines it. Where the spec has no way to do something a program needs, the program uses the smallest operation that would do it, and this page lists every such operation. Each program says in its opening comment where its memory lives. `compiler/tests/memory_study.rs` parses all of them, so they stay valid Lymra as the grammar changes. None of them runs yet: they use phase 2 and phase 3 features.

| Program | What it does | What it puts under strain |
|---|---|---|
| [tokens](tokens.lym) | Splits arithmetic into tokens | Fixed-size records that point into text |
| [expressions](expressions.lym) | Parses calls and operators into a tree, then evaluates it | Recursion that keeps a list alive across its own call |
| [game_world](game_world.lym) | Moves, damages and removes creatures over several frames | Pools, handles, per-frame scratch lists, replacing a list |
| [routes](routes.lym) | Finds the fewest hops through a network | Building adjacency from edges in any order |
| [orders](orders.lym) | Groups order lines and finds the largest order | Values holding text and lists inline |
| [word_count](word_count.lym) | Counts words with a hash table it writes itself | Growing a table by replacing it |
| [shapes](shapes.lym) | Shrinks shapes and sums their area | Overwriting choices while matching on them |
| [undo](undo.lym) | Undoes edits from a history | `stack` and `pop` |
| [report](report.lym) | Ranks players and prints a report | Lists of text, sorting with a code argument |
| [life](life.lym) | Runs Game of Life | Arrays of arrays on the call stack |
| [document_tree](document_tree.lym) | Reads a nested list into a tree and prints it | Trees in a flat list, recursion |
| [statistics](statistics.lym) | Mean, spread and extremes of samples | The common case: one list of numbers |

## What held up

- **Flat lists of fixed-size records** (tokens, routes, document_tree, statistics) fit with no friction, and they are the fastest layout anyway: one block, read in order, positions that never go stale because lists never move.
- **Scratch lists per loop iteration** (game_world) cost one store to free each frame.
- **Code arguments** (report) sort with a block that reads the enclosing function's values, with no closure object.
- **Pools** (game_world) handle things that come and go in any order, and a stale handle traps.
- **Threads** share nothing (section 9): each has its own data stacks and they exchange only values without references. Allowing two mutable references at once therefore never leads to a data race.

## Holes in the safety argument

Section 7.4 allows two mutable references to the same data at once, because "memory never moves while a reference to it lives and nothing is freed while its root is in scope". The programs found four ways to break memory safety that this argument does not cover.

**H1. A variant changes under a reference to its payload.** Memory that never moves can still change meaning:

```
match shapes[index]
    case Circle(radius)
        shapes[index] = Rect(width = 1.0, height = 2.0)
        print("{radius}")
```

`radius` refers into the payload of a `Circle`, and the assignment turns that payload into a `Rect`. Read through the old reference, the new bytes are taken for the old variant's fields. With a variant that holds text, its length prefix is read from the wrong bytes, and the program reads past the end of the value.

**H2. A growable value is replaced under a reference into it.** `slots = grown(slots)` in word_count frees the old table. A reference taken into the old `slots` before the assignment would point at freed memory, even though its root, the binding `slots`, is still in scope. The spec never says what replacing a growable value does at all.

**H3. A returned value is built over an argument the call still reads.** The spec builds return values directly in the caller's storage (section 5) and lets the compiler pass parameters by reference. In `board = step(board)` (life), building the result in `board` while `step` reads `board` mixes new cells into the generation being computed. With choices, the new variant's tag is written while the call still reads the old payload: H1 again, inside the compiler's own optimization.

**H4. A handle is used with another pool of the same type.** Section 7.6 checks a handle's generation but says nothing about which pool the handle came from or whether its slot exists in this pool. A handle from a bigger pool indexes past the end of a smaller one.

Two contradictions came up as well. Section 4.4 says `push` returns a reference to the new element, but section 5 calls `push` without using a result, which is error L0505. And section 4.5 stores a value as one block, which cannot hold a type with two growable fields while both grow.

## Where the rules got in the way

**F1. Lists of variable-size values can't be indexed.** `orders[index]`, `scores[left]` and `lines[0]` are all forbidden today, because the elements differ in size. Lists of records that hold text are among the most common data in real programs, and the workaround, keeping positions into the list by hand, is what the language should do itself.

**F2. Nested lists have to be built in order.** Only the newest value on a data stack can grow, so an order's items can't receive a line that arrives after the next order started (orders), and a list of neighbour lists can't take edges in any order (routes). Grouping input first, or using a flat layout, works and is usually faster, but it has to be taught.

**F3. Recursion can't keep a list alive across its own call** (section 7.5), even when the inner calls are finished before the outer list grows again. A recursive descent parser collecting call arguments (expressions) is the textbook case.

**F4. Replacing a growable value has no defined meaning or cost** (word_count, game_world).

**F5. Missing operations.** Every program needed something the spec does not define:

| Needed | Used in | Decision |
|---|---|---|
| The bytes of a text, to scan it | tokens, expressions, word_count, document_tree | `bytes(text)` returns `view[unsigned8]` |
| Part of a list, array or text | tokens, word_count | `slice(items, start, end)`; a slice of a `string` is a `string` that shares storage (section 3.1) |
| The top of a stack without popping it | document_tree | `top(items)` returns a copy of the top element |
| An array with every element the same | life | `array(value)`, its size taken from the declared type |
| A map | word_count | Not in 0.1; a table over fixed-size slots works today, and a built-in map waits for generics (O2) |
| Reading files and standard input | every program, which use constants instead | Open question O12 |

## Decisions

**D1. Threads stay as section 9 has them**, and a handle counts as a reference: a value that holds one can't be sent to another thread, and each thread's pools are its own.

**D2. Data stacks on 32-bit systems.** A data stack reserves address space, not memory. On a 64-bit system each reserves up to 64 GiB; on a 32-bit system, where a process has 2 to 3 GiB in all, the data stacks share 1 GiB. The runtime reserves one region for all of a program's data stacks, halving its request until the system grants it, and splits it evenly, so a program with eight data stacks can still hold 128 MiB in each on a 32-bit system. A value that outgrows its data stack stops the program with `error: data stack full`.

**D3. Open question O5 is settled: `integer64` everywhere.** Literals default to `integer64` on every target, `length` returns `integer64`, and an index can be any integer type. A program then computes the same numbers, and overflows at the same point, on every system. On a 32-bit processor 64-bit arithmetic takes two instructions where the compiler can't prove a smaller range.

## Changes to the model

**M1. Lists of variable-size values can be indexed.** Such a list keeps the position of each element in a second list beside it, on its own data stack, so `items[index]` costs one more read. Its elements still can't be replaced, because a replacement could have another size (L0407). Iterating reads the elements in order and never uses the positions. A `packed` list stays iterate-only and keeps no positions, so it stays as small as possible (L0408).

**M2. A value grows only while it is on top of its data stack.** This replaces the blanket ban on keeping a growable value alive across a recursive call (section 7.5). The compiler rejects a `push` to a value that another value may sit above at that point (L0704). A recursive function may keep a list alive across its own call, because the values the call creates are freed before it returns; the copies of one binding at different depths of recursion share a data stack, one above the other.

**M3. A value can't be replaced while a reference into it lives.** Assigning a choice, which can change its variant, or a growable value, whose storage is released, while a reference into it is still used, is error L0703. So is removing a pool slot that a reference points into. This closes H1 and H2. Overwriting a fixed-size value that is not a choice stays allowed while references to it exist: its bytes keep their meaning, so no exclusivity rule is needed.

**M4. Pools are indexed by handle, and a handle belongs to its pool.** `creatures[handle]` reads or assigns a slot, checking the slot and its generation, and replaces `get`. A handle's root is the pool it came from, like a reference's root, so using it with another pool is error L0705 and keeping it past its pool is L0701. A pool holds fixed-size values (L0706), so every slot has the same size; text goes in a list, and the pool keeps positions into it. This closes H4.

**M5. Replacing a growable value.** `items = new_value` frees the old value and puts the new one in its place. When the new value reads the old one, as in `slots = grown(slots)`, it is built on a second data stack and then moved into place with one copy. When it doesn't, as in `pending = list()`, the old value is freed first and the new one is built in place, with no copy. M3 guarantees no reference sees the old value go.

**M6. Building values in place stays safe and stays fast.** A returned value is built directly in the caller's storage unless the call still reads that storage through an argument; then the compiler builds it in a temporary and copies it (H3). `push` returns nothing; `items[length(items) - 1]` reads the newest element. A value held directly by a binding keeps each of its growable fields on a data stack of its own, so each can grow; placing it inside another value, such as a list, lays it out as one block, which copies it once. Inside a list, a value's growable fields no longer grow.

## What the changes cost

| Change | Cost at run time |
|---|---|
| M1 | One 8-byte position per element, for lists of variable-size values only |
| M2, M3, L0705, L0706 | None: they are checks in the compiler |
| M4 | The same two checks per access that pools always had |
| M5 | One copy of the new value, only when it is built from the old one |
| M6 | One copy when a value with growable fields goes into another value, which the spec's in-order building needed anyway |

## Spec changes

The spec now carries every rule above: section 4.1 (D3), 4.4 and 4.5 (M1, M6), 4.7 (L0408), 5 (M6), 5.1 (the new operations), 7.2 (D2), 7.4 (M3), 7.5 (M2), 7.6 (M4), a new 7.9 (M5), 9 (D1) and 12 (O5 settled, O12 added).
