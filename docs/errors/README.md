# Lymra error codes

Every error the compiler reports has a code. Each page says what the rule is, why it exists, and how to fix the code.
`lymra explain <code>` prints the same page.

The first two digits group the codes: 01 is source text and layout, 02 is bindings, 03 is grammar.
From 04 on, they are the chapter of the [specification](../spec.md) the rule comes from.

| Code | Rule |
|---|---|
| [L0101](L0101.md) | directive after the top of the file |
| [L0102](L0102.md) | tabs and spaces mixed in indentation |
| [L0103](L0103.md) | name does not follow the naming style |
| [L0105](L0105.md) | unknown directive |
| [L0106](L0106.md) | directive used incorrectly |
| [L0107](L0107.md) | inconsistent indentation depth |
| [L0108](L0108.md) | unexpected character |
| [L0109](L0109.md) | multiline comment is never closed |
| [L0110](L0110.md) | code after a multiline comment |
| [L0111](L0111.md) | text is never closed |
| [L0112](L0112.md) | unknown escape sequence |
| [L0113](L0113.md) | malformed interpolation |
| [L0114](L0114.md) | invalid number |
| [L0115](L0115.md) | source file is not valid UTF-8 |
| [L0203](L0203.md) | declaration without `variable` or `constant` |
| [L0204](L0204.md) | declaration without an initial value |
| [L0205](L0205.md) | `variable` outside a function |
| [L0301](L0301.md) | unexpected token |
| [L0302](L0302.md) | code at the wrong level |
| [L0303](L0303.md) | missing indented block |
| [L0304](L0304.md) | unexpected indentation |
| [L0305](L0305.md) | syntax from another language |
| [L0306](L0306.md) | chained comparison |
| [L0307](L0307.md) | misplaced `with` block |
| [L0308](L0308.md) | invalid assignment target |
| [L0309](L0309.md) | feature not available in this edition |
| [L0310](L0310.md) | nesting too deep |
