//! A message's shape: text, arguments, and selectors that choose a variant
//! by an argument (a plural category, a kind's word, an exact number).
//! The `words!` macro writes English as statics of it at build time; a
//! translation is turned into it from Fluent's syntax when first said.
//! One evaluator says both.

use std::borrow::Cow;

/// A part of a message.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Element {
    /// Words as they are.
    Text(Cow<'static, str>),
    /// An argument, by its name, said as its type says it.
    Arg(Cow<'static, str>),
    /// One of `variants`, chosen by the argument `on`; `default` where none
    /// is.
    Select {
        on: Cow<'static, str>,
        variants: Cow<'static, [Variant]>,
        default: usize,
    },
}

/// A selector's variant: what chooses it, and what it says.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Variant {
    pub key: Key,
    pub value: Cow<'static, [Element]>,
}

/// What chooses a variant.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// A plural category (`one`, `few`, `other`), or a kind's word
    /// (`folder`).
    Word(Cow<'static, str>),
    /// A count of exactly this.
    Number(i64),
}

/// The parts of a message's value written in Fluent's syntax, as Skwir
/// reads it (ADR-139, 3): text, variables, selectors on a variable. `Err`
/// says what it holds beyond (a term, a function, another message).
pub fn from_fluent(pattern: &fluent_syntax::ast::Pattern<&str>) -> Result<Vec<Element>, String> {
    use fluent_syntax::ast::{Expression, InlineExpression, PatternElement, VariantKey};
    let mut out = Vec::new();
    for element in &pattern.elements {
        match element {
            PatternElement::TextElement { value } => {
                out.push(Element::Text(Cow::Owned((*value).to_owned())));
            }
            PatternElement::Placeable { expression } => match expression {
                Expression::Inline(InlineExpression::VariableReference { id }) => {
                    out.push(Element::Arg(Cow::Owned(id.name.to_owned())));
                }
                // `{"{"}`: a sign Fluent's text cannot hold.
                Expression::Inline(InlineExpression::StringLiteral { value }) => {
                    out.push(Element::Text(Cow::Owned(unescape(value))));
                }
                Expression::Inline(InlineExpression::NumberLiteral { value }) => {
                    out.push(Element::Text(Cow::Owned((*value).to_owned())));
                }
                Expression::Select { selector, variants } => {
                    let InlineExpression::VariableReference { id } = selector else {
                        return Err("a selector chooses by a variable only".into());
                    };
                    let mut chosen = Vec::with_capacity(variants.len());
                    let mut default = 0;
                    for (at, variant) in variants.iter().enumerate() {
                        let key = match &variant.key {
                            VariantKey::Identifier { name } => {
                                Key::Word(Cow::Owned((*name).to_owned()))
                            }
                            VariantKey::NumberLiteral { value } => Key::Number(
                                value
                                    .parse()
                                    .map_err(|_| format!("`{value}` is no whole number"))?,
                            ),
                        };
                        if variant.default {
                            default = at;
                        }
                        chosen.push(Variant {
                            key,
                            value: Cow::Owned(from_fluent(&variant.value)?),
                        });
                    }
                    out.push(Element::Select {
                        on: Cow::Owned(id.name.to_owned()),
                        variants: Cow::Owned(chosen),
                        default,
                    });
                }
                Expression::Inline(other) => {
                    return Err(format!("{} is not said by Skwir", what(other)));
                }
            },
        }
    }
    Ok(out)
}

/// What a placeable Skwir does not read is.
fn what(expression: &fluent_syntax::ast::InlineExpression<&str>) -> &'static str {
    use fluent_syntax::ast::InlineExpression as E;
    match expression {
        E::FunctionReference { .. } => "a function",
        E::MessageReference { .. } => "another message",
        E::TermReference { .. } => "a term",
        E::Placeable { .. } => "a placeable in a placeable",
        _ => "this placeable",
    }
}

/// A Fluent string literal's text: `\"`, `\\` and `\uXXXX` read.
fn unescape(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len());
    let mut chars = literal.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                out.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
            }
            Some('U') => {
                let hex: String = chars.by_ref().take(6).collect();
                out.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// The variant of `variants` an argument chooses: a count by its exact
/// number first, then by its plural category in the message's language
/// (`category`); a word by itself; else the default.
pub fn choose<'a>(
    variants: &'a [Variant],
    default: usize,
    count: Option<i64>,
    word: Option<&str>,
    category: impl Fn(i64) -> &'static str,
) -> &'a [Element] {
    let found = |key: &Key| variants.iter().find(|variant| variant.key == *key);
    let chosen = match (count, word) {
        (Some(n), _) => found(&Key::Number(n)).or_else(|| {
            let category = category(n);
            variants
                .iter()
                .find(|variant| matches!(&variant.key, Key::Word(word) if word == category))
        }),
        (None, Some(word)) => variants
            .iter()
            .find(|variant| matches!(&variant.key, Key::Word(key) if key == word)),
        (None, None) => None,
    };
    chosen
        .or_else(|| variants.get(default))
        .map_or(&[], |variant| &variant.value)
}

#[cfg(test)]
mod tests {
    use super::{Element, Key, from_fluent};

    fn parsed(ftl: &str) -> Result<Vec<Element>, String> {
        let resource =
            fluent_syntax::parser::parse_runtime(ftl).map_err(|(_, e)| format!("{e:?}"))?;
        let Some(fluent_syntax::ast::Entry::Message(message)) = resource.body.into_iter().next()
        else {
            return Err("no message".into());
        };
        from_fluent(message.value.as_ref().ok_or("no value")?)
    }

    #[test]
    fn fluent_s_syntax_is_read_as_skwir_says_it() {
        let elements = parsed("a = Moved { $count ->\n    [one] one item\n   *[other] { $count } items\n} to the Trash\n").unwrap();
        assert!(matches!(&elements[0], Element::Text(t) if t == "Moved "));
        let Element::Select {
            on,
            variants,
            default,
        } = &elements[1]
        else {
            panic!("{elements:?}");
        };
        assert_eq!((on.as_ref(), *default), ("count", 1));
        assert_eq!(variants[0].key, Key::Word("one".into()));
        assert!(matches!(&elements[2], Element::Text(t) if t == " to the Trash"));
        assert_eq!(
            parsed("a = { \"{\" } braces").unwrap()[0],
            Element::Text("{".into())
        );
        assert!(parsed("a = { -brand }").unwrap_err().contains("a term"));
        assert!(
            parsed("a = { NUMBER($n) }")
                .unwrap_err()
                .contains("a function")
        );
        assert!(
            parsed("a = { other }")
                .unwrap_err()
                .contains("another message")
        );
    }
}
