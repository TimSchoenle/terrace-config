//! The portable pattern subset: regular expressions every engine reading a contract agrees on.
//!
//! A pattern a [`Refinement`](super::Refinement) publishes is read by whatever validates the
//! contract — an ECMA-262 engine behind a JSON Schema validator, Rust's `regex`, Java's
//! `java.util.regex`, Python's `re`. Those engines disagree about a great deal: what `.` excludes,
//! whether `\d` is ASCII, whether `\s` holds U+FEFF, whether `$` matches before a trailing newline,
//! whether `[a&&b]` is an intersection. A pattern leaning on any of it means one thing to the
//! producer that checked it and another to the gate that applies it, and the contract is then
//! exact in one language and unsound in the next.
//!
//! So a refinement's pattern is held to a subset in which every construct means the same thing in
//! all of them, and refused — with the construct named and the portable spelling suggested —
//! otherwise. The subset is normative; `spec/v1/FORMAT.md` (*Portable patterns*) states it for a
//! second implementation, and [`check`] is this crate's reading of it:
//!
//! - **Literals**: any character of the Basic Multilingual Plane except the syntax characters
//!   `^ $ \ . * + ? ( ) [ ] { } |`, which are written escaped.
//! - **Escapes**: a syntax character, `\t`, `\n`, `\r`, `\f`, and `\uXXXX` naming a character of the
//!   Basic Multilingual Plane that is not a surrogate. Inside a class, `\-` as well.
//! - **Classes**: `[…]` and `[^…]`, holding characters and ascending ranges of them. A literal `-`
//!   stands first or last, or is escaped; `[` is escaped; `&&`, `~~` and `||` do not appear.
//! - **Groups**: `(…)` and `(?:…)`, with `|` between alternatives.
//! - **Repetition**: `?`, `*`, `+`, `{n}`, `{n,}` and `{n,m}`, with `n ≤ m ≤ 1000`, greedy only.
//! - **Anchors**: `^` and `$`, meaning the start and the end of the whole text.
//!
//! Matching is JSON Schema's: unanchored, over characters. Outside the subset stay `.`, the class
//! escapes `\d \w \s \b` and their negations, back-references, lookaround, lazy repetition, flags,
//! and any character beyond U+FFFF. Each has a spelling inside it — a class naming exactly the
//! characters meant — and that spelling is the one every engine reads alike.
//!
//! One difference survives, and only an ECMA-262 engine run without the `u` flag can observe it:
//! such an engine reads a character beyond U+FFFF in the *value* as two, and a negated class matches
//! each half. Every other construct in the subset names only characters of the Basic Multilingual
//! Plane, so it cannot tell.

use regex::Regex;

/// The longest pattern accepted, in characters. A pattern is written by a person to describe a name
/// or a value; one longer than this is a generated artefact that no reader of a rendering can check.
const MAX_LENGTH: usize = 4096;

/// The largest count a repetition may name. Well inside every engine's own limit, and far beyond
/// anything a configuration value is measured in.
const MAX_REPEAT: u32 = 1000;

/// How deep groups may nest, for the same reason every walk in this crate stops somewhere.
const MAX_DEPTH: usize = super::MAX_DEPTH;

/// The characters that mean something outside a class, and are therefore escaped to mean
/// themselves.
const SYNTAX: &[char] = &[
    '^', '$', '\\', '.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|',
];

/// Refuse a pattern outside the portable subset, saying which construct and where.
///
/// # Errors
/// A sentence naming the first construct outside the subset, its position in characters, and the
/// portable way to say what it most likely meant.
pub(super) fn check(source: &str) -> Result<(), String> {
    if source.is_empty() {
        return Err(
            "an empty pattern matches every string, so publishing it states nothing".to_owned(),
        );
    }
    let length = source.chars().count();
    if length > MAX_LENGTH {
        return Err(format!(
            "it is {length} characters long, beyond the {MAX_LENGTH} a pattern may be"
        ));
    }
    let mut parser = Parser {
        chars: source.chars().collect(),
        at: 0,
        depth: 0,
    };
    parser.alternation()?;
    match parser.peek() {
        None => Ok(()),
        Some(_) => {
            Err(parser.refuse("`)` closes a group that was never opened; escape it as `\\)`"))
        }
    }
}

/// `source` compiled for matching, when it is inside the portable subset.
///
/// [`None`] for a pattern outside it: this crate matches only what it can promise every other
/// engine matches alike, and a pattern it cannot promise that for is not one to refuse a value
/// over.
pub(super) fn matcher(source: &str) -> Option<Regex> {
    check(source).ok()?;
    // Every construct in the subset means in `regex` what it means in ECMA-262: `$` is the end of
    // the text without the multi-line flag, and `\uXXXX` is a four-digit character code.
    Regex::new(source).ok()
}

struct Parser {
    chars: Vec<char>,
    at: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.at + offset).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let next = self.peek()?;
        self.at += 1;
        Some(next)
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    /// A refusal at the current position, counted in characters from 1.
    fn refuse(&self, why: &str) -> String {
        format!("at character {}, {why}", self.at + 1)
    }

    /// A refusal at the character just consumed.
    fn refuse_previous(&self, why: &str) -> String {
        format!("at character {}, {why}", self.at)
    }

    fn alternation(&mut self) -> Result<(), String> {
        loop {
            self.sequence()?;
            if !self.eat('|') {
                return Ok(());
            }
        }
    }

    fn sequence(&mut self) -> Result<(), String> {
        while let Some(next) = self.peek() {
            match next {
                '|' | ')' => break,
                '^' | '$' => {
                    self.bump();
                    if matches!(self.peek(), Some('*' | '+' | '?' | '{')) {
                        return Err(self.refuse("an anchor cannot be repeated"));
                    }
                }
                _ => {
                    self.atom()?;
                    self.repetition()?;
                }
            }
        }
        Ok(())
    }

    fn atom(&mut self) -> Result<(), String> {
        let Some(next) = self.bump() else {
            return Ok(());
        };
        match next {
            '(' => self.group(),
            '[' => self.class(),
            '\\' => self.escape(false).map(|_| ()),
            '.' => Err(self.refuse_previous(
                "`.` excludes a different set of line terminators in every engine; write the class \
                 of characters meant, such as `[^\\n]`",
            )),
            '*' | '+' | '?' => Err(self.refuse_previous(&format!(
                "`{next}` has nothing to repeat; escape it as `\\{next}` to mean the character"
            ))),
            '{' => Err(self.refuse_previous(
                "`{` has nothing to repeat; escape it as `\\{` to mean the character",
            )),
            ']' | '}' => Err(self.refuse_previous(&format!(
                "`{next}` on its own is read differently by different engines; escape it as \
                 `\\{next}`"
            ))),
            literal => beyond_the_plane(literal).map_err(|why| self.refuse_previous(&why)),
        }
    }

    fn group(&mut self) -> Result<(), String> {
        if self.peek() == Some('?') {
            if self.peek_at(1) == Some(':') {
                self.at += 2;
            } else {
                return Err(self.refuse(
                    "only `(?:…)` may follow `(` with `?`; lookaround, named groups and flags are \
                     not read alike by every engine",
                ));
            }
        }
        if self.depth >= MAX_DEPTH {
            return Err(self.refuse(&format!("groups nest more than {MAX_DEPTH} deep")));
        }
        self.depth += 1;
        self.alternation()?;
        self.depth -= 1;
        if self.eat(')') {
            Ok(())
        } else {
            Err(self.refuse("a group is not closed; add the `)` it needs"))
        }
    }

    fn repetition(&mut self) -> Result<(), String> {
        match self.peek() {
            Some('*' | '+' | '?') => {
                self.bump();
            }
            Some('{') => {
                self.bump();
                let least = self.count()?;
                let most = if self.eat(',') {
                    if self.peek() == Some('}') {
                        None
                    } else {
                        Some(self.count()?)
                    }
                } else {
                    Some(least)
                };
                if !self.eat('}') {
                    return Err(self.refuse(
                        "a counted repetition is `{n}`, `{n,}` or `{n,m}`; escape `{` as `\\{` to \
                         mean the character",
                    ));
                }
                if let Some(most) = most
                    && most < least
                {
                    return Err(self.refuse_previous(&format!(
                        "`{{{least},{most}}}` asks for at least {least} and at most {most}"
                    )));
                }
            }
            _ => return Ok(()),
        }
        if matches!(self.peek(), Some('*' | '+' | '?' | '{')) {
            return Err(self.refuse(
                "a repetition cannot itself be repeated or made lazy; group what is meant with \
                 `(?:…)`",
            ));
        }
        Ok(())
    }

    fn count(&mut self) -> Result<u32, String> {
        let start = self.at;
        let mut value: u32 = 0;
        while let Some(digit) = self.peek().and_then(|next| next.to_digit(10)) {
            self.bump();
            value = value.saturating_mul(10).saturating_add(digit);
        }
        if self.at == start {
            return Err(self.refuse(
                "a counted repetition needs a number; escape `{` as `\\{` to mean the character",
            ));
        }
        if value > MAX_REPEAT {
            return Err(self.refuse_previous(&format!(
                "a repetition counts at most {MAX_REPEAT}, and this asks for {value}"
            )));
        }
        Ok(value)
    }

    /// The escape after a `\` just consumed, and the one character it stands for.
    fn escape(&mut self, in_class: bool) -> Result<char, String> {
        let Some(next) = self.bump() else {
            return Err(self.refuse_previous("`\\` ends the pattern with nothing to escape"));
        };
        match next {
            syntax if SYNTAX.contains(&syntax) => Ok(syntax),
            '-' if in_class => Ok('-'),
            't' => Ok('\t'),
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            'f' => Ok('\u{c}'),
            'u' => self.code_point(),
            'd' | 'D' | 'w' | 'W' | 's' | 'S' | 'b' | 'B' => Err(self.refuse_previous(&format!(
                "`\\{next}` names a different set of characters in different engines — `\\s` \
                 holds U+FEFF in ECMA-262 and not in Rust, `\\d` is Unicode in Rust and ASCII in \
                 ECMA-262; write the class of characters meant"
            ))),
            '0'..='9' => Err(self.refuse_previous(
                "a back-reference or octal escape is not read alike by every engine",
            )),
            other => Err(self.refuse_previous(&format!(
                "`\\{other}` is not an escape every engine reads alike; the portable escapes are a \
                 syntax character, `\\t`, `\\n`, `\\r`, `\\f` and `\\uXXXX`"
            ))),
        }
    }

    /// The four hex digits after `\u`.
    fn code_point(&mut self) -> Result<char, String> {
        let mut value = 0;
        for _ in 0..4 {
            let Some(digit) = self.peek().and_then(|next| next.to_digit(16)) else {
                return Err(self.refuse("`\\u` takes exactly four hex digits"));
            };
            self.bump();
            value = value * 16 + digit;
        }
        char::from_u32(value).ok_or_else(|| {
            self.refuse_previous(&format!(
                "`\\u{value:04X}` is a surrogate, which is half of a character rather than one"
            ))
        })
    }

    /// A class, its `[` just consumed.
    fn class(&mut self) -> Result<(), String> {
        self.eat('^');
        let mut first = true;
        loop {
            self.no_set_difference()?;
            let Some(low) = self.class_member(first)? else {
                return Ok(());
            };
            first = false;
            self.no_set_difference()?;
            if self.peek() == Some('-') && !matches!(self.peek_at(1), Some(']') | None) {
                self.bump();
                self.no_set_difference()?;
                let Some(high) = self.class_member(false)? else {
                    return Err(self.refuse_previous("a range needs an upper end"));
                };
                if high < low {
                    return Err(self.refuse_previous(&format!(
                        "the range `{}-{}` runs backwards",
                        low.escape_default(),
                        high.escape_default()
                    )));
                }
            }
        }
    }

    /// Refuse an unescaped `--` inside a class, which Rust reads as set difference and Python warns
    /// it will.
    fn no_set_difference(&self) -> Result<(), String> {
        if self.peek() == Some('-') && self.peek_at(1) == Some('-') {
            Err(self.refuse(
                "`--` inside a class is a set operation in Rust; escape the `-` meant as a \
                 character as `\\-`",
            ))
        } else {
            Ok(())
        }
    }

    /// One character of a class, or [`None`] at the `]` that closes it.
    fn class_member(&mut self, first: bool) -> Result<Option<char>, String> {
        let Some(next) = self.bump() else {
            return Err(self.refuse_previous("a class is not closed; add the `]` it needs"));
        };
        match next {
            ']' if first => Err(self.refuse_previous(
                "an empty class matches nothing in ECMA-262 and is a literal `]` elsewhere; escape \
                 it as `\\]` to mean the character",
            )),
            ']' => Ok(None),
            '[' => Err(self.refuse_previous(
                "`[` inside a class opens a nested class in Rust and Java; escape it as `\\[`",
            )),
            '\\' => self.escape(true).map(Some),
            '-' if first || self.peek() == Some(']') => Ok(Some('-')),
            '-' => Err(self.refuse_previous(
                "a `-` inside a class is a range, or stands first or last; escape it as `\\-` \
                 anywhere else",
            )),
            '&' | '~' | '|' if self.peek() == Some(next) => Err(self.refuse_previous(&format!(
                "`{next}{next}` inside a class is a set operation in Rust or Java; escape one of \
                 them"
            ))),
            literal => beyond_the_plane(literal)
                .map(|()| Some(literal))
                .map_err(|why| self.refuse_previous(&why)),
        }
    }
}

/// Refuse a literal character beyond U+FFFF, which an ECMA-262 engine without the `u` flag reads
/// as two.
fn beyond_the_plane(literal: char) -> Result<(), String> {
    if u32::from(literal) > 0xFFFF {
        Err(format!(
            "`{literal}` lies beyond U+FFFF, which an ECMA-262 engine without the `u` flag reads as \
             two characters"
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{check, matcher};

    /// The patterns the issue that asked for this names, and the idioms around them.
    const PORTABLE: &[&str] = &[
        "^[a-z0-9][a-z0-9_-]{0,63}$",
        "^[A-Za-z]{2,3}(?:[-_][A-Za-z]{4})?(?:[-_](?:[A-Za-z]{2}|[0-9]{3}))?$",
        "[^\\t\\n\\u000B\\f\\r \\u0085\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000]",
        "^(a|b|)$",
        "^[-a]$",
        "^[a-]$",
        "^[\\^\\]\\[\\-\\\\]$",
        "^\\$\\.\\*\\+\\?\\(\\)\\{\\}\\|$",
        "^x{3}y{1,}z{0,1000}$",
        "^é$",
    ];

    #[test]
    fn the_portable_idioms_are_accepted_and_compile() {
        for pattern in PORTABLE {
            assert_eq!(check(pattern), Ok(()), "{pattern}");
            assert!(matcher(pattern).is_some(), "{pattern}");
        }
    }

    #[test]
    fn every_construct_outside_the_subset_is_refused_with_its_position() {
        let refused = [
            ("", "empty pattern"),
            ("a.b", "at character 2, `.`"),
            ("\\s", "`\\s`"),
            ("\\S", "`\\S`"),
            ("\\d", "`\\d`"),
            ("\\w", "`\\w`"),
            ("\\bx", "`\\b`"),
            ("(a)\\1", "back-reference"),
            ("\\x41", "`\\x`"),
            ("\\v", "`\\v`"),
            ("\\-", "`\\-`"),
            ("(?=a)", "lookaround"),
            ("(?i)a", "lookaround"),
            ("(?<n>a)", "lookaround"),
            ("a*?", "lazy"),
            ("a**", "lazy"),
            ("a{2,1}", "at most 1"),
            ("a{1001}", "at most 1000"),
            ("a{", "counted repetition"),
            ("a{x}", "counted repetition"),
            ("*a", "nothing to repeat"),
            ("{1}", "nothing to repeat"),
            ("^*", "anchor"),
            ("a]", "`]`"),
            ("a}", "`}`"),
            ("a)", "never opened"),
            ("(a", "not closed"),
            ("[a", "not closed"),
            ("[]a]", "empty class"),
            ("[a[b]]", "nested class"),
            ("[a-b-c]", "`-`"),
            ("[z-a]", "backwards"),
            ("[a&&b]", "set operation"),
            ("[a~~b]", "set operation"),
            ("[a||b]", "set operation"),
            ("[--/]", "set operation"),
            ("[!--]", "set operation"),
            ("[a--b]", "set operation"),
            ("\\uD800", "surrogate"),
            ("\\u12", "four hex digits"),
            ("😀", "beyond U+FFFF"),
            ("[😀]", "beyond U+FFFF"),
            ("a\\", "nothing to escape"),
        ];
        for (pattern, reason) in refused {
            let refusal = check(pattern).expect_err(pattern);
            assert!(
                refusal.contains(reason),
                "{pattern:?} was refused as {refusal:?}"
            );
            assert!(matcher(pattern).is_none(), "{pattern:?}");
        }
    }

    #[test]
    fn a_pattern_matches_unanchored_and_dollar_is_the_end_of_the_text() {
        let slug = matcher("^[a-z0-9][a-z0-9_-]{0,63}$").expect("portable");
        assert!(slug.is_match("terms"));
        assert!(!slug.is_match("Terms"));
        // The one place Java and Python part company with ECMA-262, and the subset's meaning.
        assert!(!slug.is_match("terms\n"));
        assert!(!slug.is_match(&"a".repeat(65)));
        assert!(slug.is_match(&"a".repeat(64)));

        let contains = matcher("b").expect("portable");
        assert!(contains.is_match("abc"));
    }

    #[test]
    fn the_escaped_code_point_means_the_character() {
        let space = matcher("^\\u0020$").expect("portable");
        assert!(space.is_match(" "));
        assert!(!space.is_match("u0020"));
    }
}
