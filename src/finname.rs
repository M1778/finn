//! Whether a package name can be spelled in Fin source, and what to write when it cannot.
//!
//! A legal registry name is not always a Fin identifier, and the gap is real rather than
//! theoretical. The registry's rule is `^[a-z][a-z0-9]*(-[a-z0-9]+)*$` (length 2-64), so
//! `http-client` is a legal package name -- and `http-client` is the register's own worked
//! example. Fin's lexer is `ID {ALPHA}({ALPHA}|{DIGIT})*` over `ALPHA [a-zA-Z_]`
//! (`Fin/src/lexer/lexer.l:63-64`): **no hyphen**, and `-` lexes as `MINUS`. So
//! `import http-client;` does not fail with "bad name", it reads as a subtraction of two
//! names nobody declared. Roughly thirty Fin keywords -- `type`, `class`, `if`, `in`, `as`,
//! `do`, `fun`, `for`, `let`, `try`, `pub` -- also satisfy the registry's rule.
//!
//! Three things follow, and all three are finn's job rather than the registry's:
//!
//! 1. **The install directory is named exactly the registry name.** `http-client` is never
//!    rewritten to `http_client`. Two spellings for one package is the same class of mistake
//!    as fabricating a version: it invents a fact and then makes the user reconcile it.
//! 2. **The import form is chosen per name.** `import { A, B } from "<name>";` works for
//!    every legal name, because the path is a string literal and the bound names are the
//!    library's own exports. There is no aliasing escape hatch to offer instead: `KW_AS`
//!    attaches to `module_path`, never to `STRING_LITERAL`, so `import "http-client" as hc;`
//!    is a syntax error (`Fin/src/parser/parser.y:717`).
//! 3. **This is said at `finn add` time, not at compile time.** It is knowable the moment
//!    the name is resolved, and it costs nothing to say. The alternative is a compiler error
//!    about an undefined variable that the user cannot connect to the package they installed.

/// Fin's reserved words, transcribed from the keyword rules in `Fin/src/lexer/lexer.l`
/// (lines 140-220), filtered to those a registry name could actually collide with -- that
/// is, the ones that satisfy `^[a-z][a-z0-9]*$`. `Self` (capitalised) and `as_ptr`
/// (underscored) are excluded because no legal package name can be spelled either way.
///
/// This list is a copy of another project's grammar and can therefore fall out of date. It
/// is used only to *warn*, never to reject: a name missing from this list is at worst a
/// warning finn failed to print, never an install finn wrongly refused.
const FIN_KEYWORDS: &[&str] = &[
    "any",
    "as",
    "auto",
    "blame",
    "bool",
    "break",
    "cast",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "define",
    "delete",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "false",
    "float",
    "fn",
    "for",
    "foreach",
    "from",
    "fun",
    "if",
    "implements",
    "import",
    "in",
    "int",
    "interface",
    "let",
    "long",
    "macro",
    "namespace",
    "new",
    "noret",
    "null",
    "operator",
    "priv",
    "pub",
    "quote",
    "readonly",
    "return",
    "sizeof",
    "special",
    "static",
    "string",
    "struct",
    "super",
    "true",
    "try",
    "type",
    "typeof",
    "void",
    "while",
];

/// How a package name fares when a Fin file has to name it.
#[derive(Debug, PartialEq, Eq)]
pub enum NameFit {
    /// Lexes as an `ID` and collides with nothing: every import form works.
    Identifier,
    /// Contains a character `ID` does not accept, named here so the message can point at it.
    /// A hyphen is the case that matters in practice, because the registry allows it.
    NotAnIdentifier(char),
    /// Lexes fine but *is* a reserved word, so it can never appear where a name is expected.
    Keyword,
}

/// Classify a name against Fin's lexer. Cheap, allocation-free, and never fails.
pub fn classify(name: &str) -> NameFit {
    // `ID {ALPHA}({ALPHA}|{DIGIT})*`, `ALPHA [a-zA-Z_]`. A leading digit is therefore also
    // not an identifier, which is why the first character is checked separately.
    let mut chars = name.chars();
    match chars.next() {
        None => return NameFit::NotAnIdentifier(' '),
        Some(c) if !c.is_ascii_alphabetic() && c != '_' => return NameFit::NotAnIdentifier(c),
        Some(_) => {}
    }

    if let Some(bad) = chars.find(|c| !c.is_ascii_alphanumeric() && *c != '_') {
        return NameFit::NotAnIdentifier(bad);
    }

    if FIN_KEYWORDS.contains(&name) {
        return NameFit::Keyword;
    }

    NameFit::Identifier
}

/// The warning to print for a name that cannot be namespace-imported, or `None` when the
/// name is fine.
///
/// The message always names the import form that *does* work, because a warning that only
/// reports a problem leaves the user to guess -- and the obvious guess, aliasing with `as`,
/// is a syntax error.
pub fn import_advice(name: &str) -> Option<String> {
    let problem = match classify(name) {
        NameFit::Identifier => return None,
        NameFit::Keyword => format!("'{}' is a reserved word in Fin", name),
        NameFit::NotAnIdentifier('-') => format!(
            "Fin's lexer has no hyphen, so the '-' in '{}' reads as subtraction",
            name
        ),
        NameFit::NotAnIdentifier(c) => format!(
            "'{}' contains '{}', which Fin cannot read as a name",
            name, c
        ),
    };

    Some(format!(
        "{}, so `import {};` will not compile. Import it by its exports instead:\n         \
         import {{ /* names */ }} from \"{}\";\n         \
         The package is installed as '{}' -- that exact spelling, unrewritten, is the one \
         to put in the string.",
        problem, name, name, name
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hyphen_is_the_case_the_registry_actually_allows() {
        assert_eq!(classify("http-client"), NameFit::NotAnIdentifier('-'));
        let advice = import_advice("http-client").unwrap();
        assert!(advice.contains("no hyphen"), "{}", advice);
        // The form that works, with the name spelled exactly as installed.
        assert!(
            advice.contains(r#"from "http-client";"#),
            "the advice must name the import form that works: {}",
            advice
        );
    }

    #[test]
    fn a_keyword_collision_is_reported_as_one() {
        for kw in [
            "type", "class", "if", "in", "as", "do", "fun", "for", "let", "try", "pub",
        ] {
            assert_eq!(classify(kw), NameFit::Keyword, "'{}' is a Fin keyword", kw);
            let advice = import_advice(kw).unwrap();
            assert!(advice.contains("reserved word"), "{}", advice);
        }
    }

    #[test]
    fn an_ordinary_name_is_left_alone() {
        for ok in ["json", "http2", "std", "fs", "my_lib", "Widget"] {
            assert_eq!(classify(ok), NameFit::Identifier, "'{}'", ok);
            assert!(import_advice(ok).is_none(), "'{}' needs no advice", ok);
        }
    }

    /// Aliasing is the obvious guess and it does not compile: `KW_AS` attaches to
    /// `module_path`, never to a `STRING_LITERAL`. The advice must not send anyone there.
    #[test]
    fn the_advice_never_suggests_aliasing() {
        for name in ["http-client", "type", "in"] {
            let advice = import_advice(name).unwrap();
            // The two shapes that would be wrong: `import "x" as y;` and `import x as y;`.
            assert!(
                !advice.contains("\" as") && !advice.contains(&format!("{} as ", name)),
                "the advice offered an alias for '{}': {}",
                name,
                advice
            );
        }
    }

    /// The table earns its place only if every entry is a name the registry could hand out:
    /// `^[a-z][a-z0-9]*$`, two characters or more. An entry that fails this is dead weight
    /// that can never fire.
    #[test]
    fn every_listed_keyword_is_a_name_the_registry_could_issue() {
        for kw in FIN_KEYWORDS {
            assert!(
                kw.len() >= 2,
                "'{}' is shorter than the registry minimum",
                kw
            );
            assert!(
                kw.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "'{}' cannot be a registry name, so it can never collide",
                kw
            );
            assert!(kw.starts_with(|c: char| c.is_ascii_lowercase()), "'{}'", kw);
        }
        // Sorted, so the next person adding one can find it.
        let mut sorted = FIN_KEYWORDS.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, FIN_KEYWORDS, "keep FIN_KEYWORDS sorted");
    }

    #[test]
    fn a_leading_digit_is_not_an_identifier_either() {
        assert_eq!(classify("2fast"), NameFit::NotAnIdentifier('2'));
        assert_eq!(classify(""), NameFit::NotAnIdentifier(' '));
    }
}
