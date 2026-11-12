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
| [L0104](L0104.md) | index not proven in range |
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
| [L0201](L0201.md) | assignment to an undeclared name |
| [L0202](L0202.md) | binding that is never used |
| [L0203](L0203.md) | declaration without `variable` or `constant` |
| [L0204](L0204.md) | declaration without an initial value |
| [L0205](L0205.md) | `variable` outside a function |
| [L0206](L0206.md) | unknown name |
| [L0207](L0207.md) | name declared twice |
| [L0208](L0208.md) | constant or parameter changed |
| [L0209](L0209.md) | variable that never changes |
| [L0210](L0210.md) | top-level constant not known at compile time |
| [L0301](L0301.md) | unexpected token |
| [L0302](L0302.md) | code at the wrong level |
| [L0303](L0303.md) | missing indented block |
| [L0304](L0304.md) | unexpected indentation |
| [L0305](L0305.md) | syntax from another language |
| [L0306](L0306.md) | chained comparison |
| [L0307](L0307.md) | misplaced `with` block |
| [L0308](L0308.md) | invalid assignment target |
| [L0309](L0309.md) | feature not available yet |
| [L0310](L0310.md) | nesting too deep |
| [L0402](L0402.md) | mismatched types |
| [L0403](L0403.md) | unknown type |
| [L0404](L0404.md) | number does not fit its type |
| [L0405](L0405.md) | operator used on the wrong type |
| [L0406](L0406.md) | division by zero |
| [L0409](L0409.md) | array type written wrong |
| [L0410](L0410.md) | array too large for the call stack |
| [L0411](L0411.md) | index out of range |
| [L0412](L0412.md) | `array(value)` without a type |
| [L0501](L0501.md) | wrong number of arguments |
| [L0502](L0502.md) | function can end without returning a value |
| [L0503](L0503.md) | `return` does not match the function |
| [L0504](L0504.md) | `variable` mark does not match the parameter |
| [L0505](L0505.md) | value is not used |
| [L0506](L0506.md) | missing or malformed `main` |
| [L0507](L0507.md) | function and value confused |
| [L0508](L0508.md) | binding changed and used in one expression |
| [L0601](L0601.md) | `break` or `continue` outside a loop |
| [L0602](L0602.md) | unreachable code |
