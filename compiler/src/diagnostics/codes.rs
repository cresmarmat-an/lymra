/*!
 * The registry of error codes.
 *
 * The first two digits group codes by topic: 01 source text and layout,
 * 02 bindings, 03 grammar, and from 04 on the spec chapter the rule comes
 * from. Each code has a page in `docs/errors/`, which is embedded here so
 * `lymra explain` always matches the compiler that printed the error.
 */

use std::fmt;

macro_rules! error_codes {
    ($($code:ident => $summary:literal,)+) => {
        /// A stable identifier for one rule the compiler enforces.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum ErrorCode {
            $($code,)+
        }

        impl ErrorCode {
            pub const ALL: &'static [ErrorCode] = &[$(ErrorCode::$code,)+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$code => stringify!($code),)+
                }
            }

            /// A short name for the rule, used as the title of its page.
            pub const fn summary(self) -> &'static str {
                match self {
                    $(Self::$code => $summary,)+
                }
            }

            /// The full page from `docs/errors/`: the rule, why it exists, and how to fix it.
            pub const fn explanation(self) -> &'static str {
                match self {
                    $(Self::$code => include_str!(concat!(
                        "../../../docs/errors/",
                        stringify!($code),
                        ".md"
                    )),)+
                }
            }
        }
    };
}

error_codes! {
    L0101 => "directive after the top of the file",
    L0102 => "tabs and spaces mixed in indentation",
    L0103 => "name does not follow the naming style",
    L0105 => "unknown directive",
    L0106 => "directive used incorrectly",
    L0107 => "inconsistent indentation depth",
    L0108 => "unexpected character",
    L0109 => "multiline comment is never closed",
    L0110 => "code after a multiline comment",
    L0111 => "text is never closed",
    L0112 => "unknown escape sequence",
    L0113 => "malformed interpolation",
    L0114 => "invalid number",
    L0115 => "source file is not valid UTF-8",
    L0201 => "assignment to an undeclared name",
    L0202 => "binding that is never used",
    L0203 => "declaration without `variable` or `constant`",
    L0204 => "declaration without an initial value",
    L0205 => "`variable` outside a function",
    L0206 => "unknown name",
    L0207 => "name declared twice",
    L0208 => "constant or parameter changed",
    L0209 => "variable that never changes",
    L0210 => "top-level constant not known at compile time",
    L0301 => "unexpected token",
    L0302 => "code at the wrong level",
    L0303 => "missing indented block",
    L0304 => "unexpected indentation",
    L0305 => "syntax from another language",
    L0306 => "chained comparison",
    L0307 => "misplaced `with` block",
    L0308 => "invalid assignment target",
    L0309 => "feature not available yet",
    L0310 => "nesting too deep",
    L0402 => "mismatched types",
    L0403 => "unknown type",
    L0404 => "number does not fit its type",
    L0405 => "operator used on the wrong type",
    L0406 => "division by zero",
    L0501 => "wrong number of arguments",
    L0502 => "function can end without returning a value",
    L0503 => "`return` does not match the function",
    L0504 => "`variable` mark does not match the parameter",
    L0505 => "value is not used",
    L0506 => "missing or malformed `main`",
    L0507 => "function and value confused",
    L0601 => "`break` or `continue` outside a loop",
    L0602 => "unreachable code",
}

impl ErrorCode {
    /// Looks a code up by its spelling, ignoring letter case.
    pub fn from_text(text: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|code| code.as_str().eq_ignore_ascii_case(text))
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_is_titled_with_its_code_and_summary() {
        for code in ErrorCode::ALL {
            let title = format!("# {code}: {}", code.summary());
            let first_line = code.explanation().lines().next().unwrap_or_default();
            assert_eq!(
                first_line, title,
                "docs/errors/{code}.md has the wrong title"
            );
        }
    }

    #[test]
    fn every_page_explains_the_rule_and_the_fix() {
        for code in ErrorCode::ALL {
            let page = code.explanation();
            for section in ["## Example", "## Why", "## Fix"] {
                assert!(
                    page.contains(section),
                    "docs/errors/{code}.md is missing `{section}`"
                );
            }
        }
    }

    #[test]
    fn codes_are_found_in_any_letter_case() {
        assert_eq!(ErrorCode::from_text("l0102"), Some(ErrorCode::L0102));
        assert_eq!(ErrorCode::from_text("L9999"), None);
    }

    #[test]
    fn codes_are_listed_in_ascending_order() {
        let codes: Vec<&str> = ErrorCode::ALL.iter().map(|code| code.as_str()).collect();
        assert!(
            codes.is_sorted(),
            "keep the registry sorted so the index stays readable"
        );
    }
}
