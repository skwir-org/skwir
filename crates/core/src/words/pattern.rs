//! A message's shape: text, arguments, and selectors that choose a variant
//! by an argument (a plural category, a kind's word, an exact number). A
//! translation may also choose by a noun's gender and place a noun in one of
//! its forms (`GENDER`, `FORM`: ADR-139, 5), its noun's attributes.
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
    /// One of `variants`, chosen by the gender of the argument `on`
    /// (`GENDER($place)`): its noun's `.gender` in the language the message
    /// is said in; `other` for an argument that is no noun (a file's name,
    /// a path, text as written). `default` where none is.
    Gender {
        on: Cow<'static, str>,
        variants: Cow<'static, [Variant]>,
        default: usize,
    },
    /// The argument `of` in its form `case` (`FORM($place, case:
    /// "dative")`): its noun's attribute of that name in the language the
    /// message is said in; an argument that is no noun, as it is said.
    Form {
        of: Cow<'static, str>,
        case: Cow<'static, str>,
    },
}

/// The functions a translation may call (ADR-139, 3): what chooses by a
/// noun's gender, and what places a noun in one of its forms.
pub const GENDER: &str = "GENDER";
pub const FORM: &str = "FORM";
/// `FORM`'s one named argument: the form, as the language names it.
pub const CASE: &str = "case";
/// The attribute a noun's gender is: its other attributes are its forms.
pub const GENDER_ATTRIBUTE: &str = "gender";
/// The gender of what is no noun, the default of every `GENDER` selector.
pub const NO_GENDER: &str = "other";

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
/// reads it (ADR-139, 3): text, variables, selectors on a variable or on a
/// noun's gender (`GENDER`), a noun's form (`FORM`). `Err` says what it
/// holds beyond (a term, another function, another message).
pub fn from_fluent(pattern: &fluent_syntax::ast::Pattern<&str>) -> Result<Vec<Element>, String> {
    use fluent_syntax::ast::{Expression, InlineExpression, PatternElement};
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
                Expression::Inline(InlineExpression::FunctionReference { id, arguments })
                    if id.name == FORM =>
                {
                    let (of, case) = form_of(arguments)?;
                    out.push(Element::Form {
                        of: Cow::Owned(of),
                        case: Cow::Owned(case),
                    });
                }
                Expression::Select { selector, variants } => {
                    let (on, by_gender) = match selector {
                        InlineExpression::VariableReference { id } => (id.name, false),
                        InlineExpression::FunctionReference { id, arguments }
                            if id.name == GENDER =>
                        {
                            (gender_of(arguments)?, true)
                        }
                        _ => {
                            return Err(
                                "a selector chooses by a variable, or by GENDER($noun)".into()
                            );
                        }
                    };
                    let (variants, default) = variants_of(variants)?;
                    let (on, variants) = (Cow::Owned(on.to_owned()), Cow::Owned(variants));
                    out.push(if by_gender {
                        Element::Gender {
                            on,
                            variants,
                            default,
                        }
                    } else {
                        Element::Select {
                            on,
                            variants,
                            default,
                        }
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

/// A selector's variants, and which is the default.
fn variants_of(
    variants: &[fluent_syntax::ast::Variant<&str>],
) -> Result<(Vec<Variant>, usize), String> {
    use fluent_syntax::ast::VariantKey;
    let mut chosen = Vec::with_capacity(variants.len());
    let mut default = 0;
    for (at, variant) in variants.iter().enumerate() {
        let key = match &variant.key {
            VariantKey::Identifier { name } => Key::Word(Cow::Owned((*name).to_owned())),
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
    Ok((chosen, default))
}

/// `GENDER($noun)`'s argument: one variable, nothing else.
fn gender_of<'s>(
    arguments: &fluent_syntax::ast::CallArguments<&'s str>,
) -> Result<&'s str, String> {
    use fluent_syntax::ast::InlineExpression;
    match (arguments.positional.as_slice(), arguments.named.is_empty()) {
        ([InlineExpression::VariableReference { id }], true) => Ok(id.name),
        _ => Err("GENDER takes one argument, a variable: GENDER($place)".into()),
    }
}

/// `FORM($noun, case: "dative")`'s argument and form.
fn form_of(
    arguments: &fluent_syntax::ast::CallArguments<&str>,
) -> Result<(String, String), String> {
    use fluent_syntax::ast::InlineExpression;
    let wrong =
        || format!("FORM takes a variable and its {CASE}: FORM($place, {CASE}: \"dative\")");
    let ([InlineExpression::VariableReference { id }], [named]) =
        (arguments.positional.as_slice(), arguments.named.as_slice())
    else {
        return Err(wrong());
    };
    match &named.value {
        InlineExpression::StringLiteral { value } if named.name.name == CASE => {
            Ok((id.name.to_owned(), unescape(value)))
        }
        _ => Err(wrong()),
    }
}

/// What a placeable Skwir does not read is.
fn what(expression: &fluent_syntax::ast::InlineExpression<&str>) -> &'static str {
    use fluent_syntax::ast::InlineExpression as E;
    match expression {
        E::FunctionReference { id, .. } if id.name == GENDER => {
            "GENDER alone (it chooses a variant: { GENDER($place) -> … })"
        }
        E::FunctionReference { .. } => "a function other than GENDER and FORM",
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
                .contains("a function other than GENDER and FORM")
        );
        assert!(
            parsed("a = { other }")
                .unwrap_err()
                .contains("another message")
        );
    }

    /// A translation chooses by a noun's gender and places a noun in one of
    /// its forms: exactly `GENDER($x)` as a selector and `FORM($x, case:
    /// "…")` as a placeable, nothing else of either.
    #[test]
    fn a_noun_s_gender_and_forms_are_read() {
        let elements = parsed(
            "a = { GENDER($place) ->\n    [feminine] dans { FORM($place, case: \"definite\") }\n   *[other] dans { $place }\n}\n",
        )
        .unwrap();
        let [
            Element::Gender {
                on,
                variants,
                default,
            },
        ] = elements.as_slice()
        else {
            panic!("{elements:?}");
        };
        assert_eq!((on.as_ref(), *default), ("place", 1));
        assert_eq!(variants[0].key, Key::Word("feminine".into()));
        assert_eq!(
            variants[0].value.as_ref(),
            [
                Element::Text("dans ".into()),
                Element::Form {
                    of: "place".into(),
                    case: "definite".into()
                }
            ]
        );
        for wrong in [
            "a = { FORM($place) }",
            "a = { FORM($place, form: \"dative\") }",
            "a = { FORM($place, case: 3) }",
            "a = { FORM(\"x\", case: \"dative\") }",
            "a = { GENDER($a, $b) ->\n   *[other] x\n}\n",
            "a = { GENDER($a, case: \"x\") ->\n   *[other] x\n}\n",
            "a = { GENDER($place) }",
            "a = { CASE($place) ->\n   *[other] x\n}\n",
        ] {
            assert!(parsed(wrong).is_err(), "{wrong}");
        }
    }
}
