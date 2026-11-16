/*!
 * The runtime's data stacks on their own, with every C compiler: the
 * reservation, memory committed in doubling steps as a value grows, moves
 * between and within stacks, freeing, shrinking, where text lies, and the
 * two errors a growing value or new text can stop the program with. Built with AddressSanitizer, it
 * also checks which bytes are marked as freed. Programs only reach these through generated
 * code, which cannot make a stack small enough to fill or put a value under
 * another on purpose, so the harness calls the runtime directly.
 */

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use lymra::driver::{Compiler, TemporaryDirectory, Warnings};

/// Three data stacks, so moves between stacks and the split of the reservation are both tried.
const PRELUDE: &str = "#define LYMRA_DATA_STACKS 3\n";

/// Checks the data stacks and prints `ok` with the number of checks, or runs one of the errors named on its command line.
const HARNESS: &str = r#"
static unsigned long checks;
static unsigned long failures;

static void require(const char *what, bool holds) {
    checks++;
    if (!holds) {
        printf("wrong: %s\n", what);
        failures++;
    }
}

static size_t span(const char *start, const char *end) {
    return (size_t)(end - start);
}

#if defined(LYMRA_ADDRESS_SANITIZER)
int __asan_address_is_poisoned(void const volatile *address);

/* Whether AddressSanitizer marks the `count` bytes at `start` as freed, every one if `freed`, else none. */
static bool marked(const void *start, size_t count, bool freed) {
    const char *bytes = (const char *)start;
    for (size_t index = 0; index < count; index++) {
        if ((__asan_address_is_poisoned(bytes + index) != 0) != freed) {
            return false;
        }
    }
    return true;
}
#endif

/* Every stack has a range of its own, a multiple of the commit step, with nothing committed or used yet. */
static void check_reservation(void) {
    for (size_t index = 0; index < LYMRA_DATA_STACKS; index++) {
        lymra_data_stack *stack = &lymra_data_stacks[index];
        size_t size = span(stack->base, stack->limit);
        require("an empty stack starts at its base", stack->top == stack->base && stack->committed == stack->base);
        require("a range is a whole number of commit steps", size >= LYMRA_COMMIT_STEP && size % LYMRA_COMMIT_STEP == 0);
        require("a range starts where any value can", (uintptr_t)stack->base % 16 == 0);
        if (index > 0) {
            require("ranges follow one another", lymra_data_stacks[index - 1].limit == stack->base);
        }
    }
#if SIZE_MAX > 0xFFFFFFFFu
    require("a 64-bit system reserves up to 64 GiB a stack",
            span(lymra_data_stacks[0].base, lymra_data_stacks[0].limit) <= ((size_t)1 << 36));
#else
    require("a 32-bit system shares 1 GiB",
            span(lymra_data_stacks[0].base, lymra_data_stacks[LYMRA_DATA_STACKS - 1].limit) <= ((size_t)1 << 30));
#endif
}

/* A value that grows one element at a time keeps every element, and memory is committed in doubling steps. */
static void check_growth(void) {
    lymra_data_stack *stack = &lymra_data_stacks[0];
    int64_t *elements = (int64_t *)(void *)lymra_data_stack_align(stack, _Alignof(int64_t));
    size_t count = 0;
    size_t commits = 0;
    const size_t total = ((size_t)3 << 20) / sizeof(int64_t);
    for (; count < total; count++) {
        char *before = stack->committed;
        int64_t *slot = (int64_t *)(void *)lymra_data_stack_extend(stack, (char *)(elements + count), sizeof(int64_t),
                                                                      "growth");
        *slot = (int64_t)count * 3;
        if (stack->committed != before) {
            size_t committed = span(stack->base, stack->committed);
            commits++;
            require("each commit doubles what is committed", committed == (LYMRA_COMMIT_STEP << (commits - 1)));
        }
    }
    require("3 MiB took 7 commits", commits == 7);
    require("the top ends the value", stack->top == (char *)(elements + count));
    bool kept = true;
    for (size_t index = 0; index < count; index++) {
        kept = kept && elements[index] == (int64_t)index * 3;
    }
    require("every element is kept as the value grows", kept);
    lymra_data_stack_release(stack, (char *)elements);
    require("freeing restores the top", stack->top == stack->base);
    require("freeing keeps the memory committed", span(stack->base, stack->committed) == (size_t)LYMRA_COMMIT_STEP << 6);
}

/* A request larger than everything committed so far commits all it needs at once. */
static void check_large_request(void) {
    lymra_data_stack *stack = &lymra_data_stacks[2];
    const size_t size = ((size_t)5 << 20) + 3;
    char *start = lymra_data_stack_extend(stack, stack->top, size, "large");
    memset(start, 7, size);
    require("one large request is met", span(stack->base, stack->top) == size);
    require("it commits a whole number of steps",
            span(stack->base, stack->committed) >= size && span(stack->base, stack->committed) % LYMRA_COMMIT_STEP == 0);
    lymra_data_stack_release(stack, start);
}

static void fill(int32_t *values, size_t count, int32_t first) {
    for (size_t index = 0; index < count; index++) {
        values[index] = first + (int32_t)index;
    }
}

static bool counts_up(const int32_t *values, size_t count, int32_t first) {
    for (size_t index = 0; index < count; index++) {
        if (values[index] != first + (int32_t)index) {
            return false;
        }
    }
    return true;
}

/*
 * `place` copies to the top of a stack from anywhere: another stack, the
 * same stack below the top, and the same stack above it, where a returning
 * function's result lies once its lists are dropped.
 */
static void check_places(void) {
    lymra_data_stack *first = &lymra_data_stacks[0];
    lymra_data_stack *second = &lymra_data_stacks[1];
    const size_t count = 100000;
    const size_t bytes = count * sizeof(int32_t);

    int32_t *source = (int32_t *)(void *)lymra_data_stack_extend(first, first->top, bytes, "source");
    fill(source, count, 11);
    int32_t *copy = (int32_t *)(void *)lymra_data_stack_place(second, source, bytes, _Alignof(int32_t), "copy");
    require("a copy to another stack is the same", counts_up(copy, count, 11));
    require("a copy ends its stack", second->top == (char *)(copy + count));

    int32_t *again = (int32_t *)(void *)lymra_data_stack_place(first, source, bytes, _Alignof(int32_t), "again");
    require("a copy from lower on the same stack is the same", again == source + count && counts_up(again, count, 11));

    /* Dropped down to `source`, with the copy right above it still readable: the copy moves down into place. */
    lymra_data_stack_drop(first, (char *)source);
    int32_t *moved = (int32_t *)(void *)lymra_data_stack_place(first, again, bytes, _Alignof(int32_t), "moved");
    require("a move down keeps every element", moved == source && counts_up(moved, count, 11));
    require("the move ends its stack", first->top == (char *)(source + count));

    /* A move by less than its own length overlaps itself. */
    fill(source, count, 50);
    lymra_data_stack_drop(first, (char *)source);
    int32_t *shifted = (int32_t *)(void *)lymra_data_stack_place(first, source + 3, bytes - 3 * sizeof(int32_t),
                                                                  _Alignof(int32_t), "shifted");
    require("a move overlapping itself keeps every element", shifted == source && counts_up(shifted, count - 3, 53));

    int32_t *empty = (int32_t *)(void *)lymra_data_stack_place(second, source, 0, _Alignof(int32_t), "empty");
    require("an empty copy starts at the top", (char *)empty == second->top);

    lymra_data_stack_release(first, first->base);
    lymra_data_stack_release(second, second->base);
}

/* A value can grow only from the top of its stack. */
static void check_top(void) {
    lymra_data_stack *stack = &lymra_data_stacks[1];
    char *older = lymra_data_stack_extend(stack, stack->top, 16, "older");
    require("the newest value is on top", stack->top == older + 16);
    char *newer = lymra_data_stack_extend(stack, stack->top, 16, "newer");
    require("a newer value lies above the older one", newer == older + 16);
    lymra_data_stack_release(stack, newer);
    lymra_data_stack_extend(stack, older + 16, 16, "older again");
    require("the older value grows again once it is on top", stack->top == older + 32);
    lymra_data_stack_release(stack, older);
}

/*
 * Taking the last elements off a value frees only them, and the value grows
 * back into their place. One-byte elements end inside the eight-byte blocks
 * AddressSanitizer marks, which it has to handle a byte at a time.
 */
static void check_shrink(void) {
    lymra_data_stack *stack = &lymra_data_stacks[2];
    int64_t *numbers = (int64_t *)(void *)lymra_data_stack_align(stack, _Alignof(int64_t));
    for (size_t count = 0; count < 4; count++) {
        lymra_data_stack_extend(stack, (char *)(numbers + count), sizeof(int64_t), "numbers");
        numbers[count] = (int64_t)count;
    }
    lymra_data_stack_shrink(stack, (char *)(numbers + 3), sizeof(int64_t));
    require("shrinking moves the top to the freed element", stack->top == (char *)(numbers + 3));
    lymra_data_stack_extend(stack, (char *)(numbers + 3), sizeof(int64_t), "numbers again");
    require("the value grows back into what it freed", stack->top == (char *)(numbers + 4));
    lymra_data_stack_shrink(stack, (char *)(numbers + 3), sizeof(int64_t));
    lymra_data_stack_shrink(stack, (char *)(numbers + 2), sizeof(int64_t));
    require("shrinking keeps the elements below", numbers[0] == 0 && numbers[1] == 1);
#if defined(LYMRA_ADDRESS_SANITIZER)
    require("freed elements are marked", marked(numbers + 2, 2 * sizeof(int64_t), true));
    require("kept elements are not", marked(numbers, 2 * sizeof(int64_t), false));
#endif
    lymra_data_stack_release(stack, (char *)numbers);

    unsigned char *bytes = (unsigned char *)lymra_data_stack_align(stack, 1);
    for (size_t count = 0; count < 13; count++) {
        lymra_data_stack_extend(stack, (char *)(bytes + count), 1, "bytes");
        bytes[count] = (unsigned char)count;
    }
    for (size_t count = 13; count > 5; count--) {
        lymra_data_stack_shrink(stack, (char *)(bytes + count - 1), 1);
    }
    require("shrinking a byte at a time keeps the bytes below", bytes[4] == 4 && stack->top == (char *)(bytes + 5));
#if defined(LYMRA_ADDRESS_SANITIZER)
    require("freed bytes are marked, inside a block too", marked(bytes + 5, 8, true));
    require("kept bytes are not, inside a block too", marked(bytes, 5, false));
#endif
    lymra_data_stack_release(stack, (char *)bytes);
}

/* Grows a value past the end of a stack whose range is made small. */
static void fill_up(void) {
    lymra_data_stack *stack = &lymra_data_stacks[0];
    stack->limit = stack->base + 2 * LYMRA_COMMIT_STEP;
    char *start = lymra_data_stack_extend(stack, stack->top, LYMRA_COMMIT_STEP, "  --> fill.lym:1:1\n");
    lymra_data_stack_extend(stack, start + LYMRA_COMMIT_STEP, 2 * LYMRA_COMMIT_STEP, "  --> fill.lym:2:1\n");
}

/* Where text starts and ends on a data stack, and that text the program itself keeps is never copied. */
static void check_text_ends(void) {
    lymra_data_stack *stack = &lymra_data_stacks[1];
    char *start = stack->top;
    lymra_string kept = {"kept", 4};
    lymra_text_own(&kept, stack, "kept");
    require("text the program keeps is not copied", stack->top == start && kept.data[0] == 'k');
    char *bytes = lymra_data_stack_extend(stack, stack->top, 3, "bytes");
    memcpy(bytes, "abc", 3);
    lymra_string copy = {bytes, 3};
    lymra_text_own(&copy, stack, "copy");
    require("text on a data stack is copied to its top", copy.data == bytes + 3 && memcmp(copy.data, "abc", 3) == 0);
    lymra_string empty = {"", 0};
    require("text ends where its last byte is", lymra_text_highest(&copy, stack, start) == bytes + 6);
    require("text the program keeps ends on no data stack", lymra_text_highest(&kept, stack, start) == start);
    require("empty text ends on no data stack", lymra_text_highest(&empty, stack, start) == start);
    require("text starts where its first byte is", lymra_text_lowest(&copy, stack, NULL) == bytes + 3);
    lymra_data_stack_release(stack, start);
}

/* Gives new text to a value that newer text sits above, which the compiler rejects in programs (L0704). */
static void text_under(void) {
    lymra_data_stack *stack = &lymra_data_stacks[0];
    char *start = stack->top;
    char *bytes = lymra_data_stack_extend(stack, stack->top, 2, "  --> text.lym:1:1\n");
    memcpy(bytes, "ab", 2);
    lymra_string older = {bytes, 2};
    lymra_data_stack_extend(stack, stack->top, 2, "  --> text.lym:2:1\n");
    lymra_data_stack_require_top(stack, lymra_text_highest(&older, stack, start), "  --> text.lym:3:1\n");
}

/* Grows a value that a newer value sits above. */
static void grow_under(void) {
    lymra_data_stack *stack = &lymra_data_stacks[0];
    char *older = lymra_data_stack_extend(stack, stack->top, 8, "  --> under.lym:1:1\n");
    lymra_data_stack_extend(stack, stack->top, 8, "  --> under.lym:2:1\n");
    lymra_data_stack_extend(stack, older + 8, 8, "  --> under.lym:3:1\n");
}

int main(int argc, char **argv) {
    lymra_start();
    if (argc > 1 && strcmp(argv[1], "full") == 0) {
        fill_up();
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "under") == 0) {
        grow_under();
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "text_under") == 0) {
        text_under();
        return 0;
    }
    check_reservation();
    check_growth();
    check_large_request();
    check_places();
    check_top();
    check_shrink();
    check_text_ends();
    printf("%s: %lu checks, %lu wrong\n", failures == 0 ? "ok" : "failed", checks, failures);
    return failures == 0 ? 0 : 1;
}
"#;

fn build(compiler: &Compiler, directory: &Path) -> std::path::PathBuf {
    let runtime = common::read(&common::repository_root().join("compiler/src/codegen/runtime.h"));
    let executable = directory.join(format!("data_stacks{}", std::env::consts::EXE_SUFFIX));
    compiler
        .compile(
            &format!("{PRELUDE}{runtime}{HARNESS}"),
            &executable,
            None,
            Warnings::Strict,
        )
        .unwrap_or_else(|error| panic!("{} rejects the harness:\n{error}", compiler.name()));
    executable
}

fn run(executable: &Path, argument: Option<&str>) -> Output {
    Command::new(executable)
        .args(argument)
        .output()
        .expect("the harness runs")
}

/// Checks that a run stopped with the runtime error status and an error that starts with `message` and points at `site`.
fn assert_stopped(output: &Output, message: &str, site: &str, compiler: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(70),
        "{compiler}: expected the runtime error status\n{stderr}"
    );
    assert!(
        stderr.starts_with(message) && stderr.contains(site) && stderr.contains("help: "),
        "{compiler}: expected `{message}` at `{site}` with help, found:\n{stderr}"
    );
}

#[test]
fn data_stacks_grow_move_and_free() {
    let mut runs = 0;
    for compiler in common::c_compilers() {
        let workspace = TemporaryDirectory::new().expect("a temporary directory can be made");
        let executable = build(&compiler, workspace.path());

        let output = run(&executable, None);
        let report = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && report.starts_with("ok: "),
            "{}:\n{report}{}",
            compiler.name(),
            String::from_utf8_lossy(&output.stderr)
        );

        assert_stopped(
            &run(&executable, Some("full")),
            "error: data stack full\n",
            "fill.lym:2:1",
            &compiler.name(),
        );
        assert_stopped(
            &run(&executable, Some("under")),
            "error: a value changed while a newer value sat above it on its data stack\n",
            "under.lym:3:1",
            &compiler.name(),
        );
        assert_stopped(
            &run(&executable, Some("text_under")),
            "error: a value changed while a newer value sat above it on its data stack\n",
            "text.lym:3:1",
            &compiler.name(),
        );
        fs::remove_file(&executable).ok();
        runs += 1;
    }
    assert!(runs > 0, "no C compiler built the harness");
}
