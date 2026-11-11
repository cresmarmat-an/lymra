/*!
 * Words from other languages, and the Lymra keyword each one stands for.
 *
 * A programmer's first Lymra file is full of habits from elsewhere. Each of
 * these words gets one error that names the Lymra spelling, and parsing then
 * continues as if that spelling had been written.
 */

use crate::ast::Mutability;
use crate::lexer::Keyword;
use crate::similarity;

/// Keywords that can start a top-level declaration.
pub(super) const ITEM_KEYWORDS: [Keyword; 6] = [
    Keyword::Function,
    Keyword::Type,
    Keyword::Choice,
    Keyword::Use,
    Keyword::Constant,
    Keyword::Public,
];

/// Keywords that start a statement and are followed by more on the same line.
pub(super) const STATEMENT_KEYWORDS: [Keyword; 8] = [
    Keyword::Variable,
    Keyword::Constant,
    Keyword::If,
    Keyword::While,
    Keyword::For,
    Keyword::Match,
    Keyword::Return,
    Keyword::Assert,
];

pub(super) fn item_keyword(word: &str) -> Option<Keyword> {
    match word {
        "fn" | "func" | "fun" | "def" => Some(Keyword::Function),
        "struct" | "class" | "record" => Some(Keyword::Type),
        "enum" | "union" => Some(Keyword::Choice),
        "import" | "include" | "using" | "require" => Some(Keyword::Use),
        "extern" => Some(Keyword::External),
        _ => None,
    }
}

pub(super) fn binding_keyword(word: &str) -> Option<Mutability> {
    match word {
        "var" | "mut" => Some(Mutability::Variable),
        "let" | "const" | "val" | "final" => Some(Mutability::Constant),
        _ => None,
    }
}

pub(super) fn is_visibility(word: &str) -> bool {
    matches!(word, "pub" | "export")
}

pub(super) fn is_else_if(word: &str) -> bool {
    matches!(word, "elif" | "elsif" | "elseif")
}

/// Words that end a block header in other languages, where Lymra has only the line break.
pub(super) fn is_block_opener(word: &str) -> bool {
    matches!(word, "then" | "do")
}

/// The keyword among `candidates` that `word` is most likely a misspelling of.
pub(super) fn misspelled(word: &str, candidates: &[Keyword]) -> Option<Keyword> {
    let spellings = candidates.iter().map(|keyword| keyword.as_str());
    similarity::closest_match(word, spellings).and_then(Keyword::from_text)
}
