//! `words!(id = "English", argument = value, …, note = "…")`: a message of
//! the crate that writes it, in English beside its id (ADR-139, 2;
//! `docs/invariants/words.md`).
//!
//! At compile time the English is read as Fluent's syntax, as much of it
//! as Skwir says (text, arguments, selectors on an argument): what it holds
//! beyond, or an argument it names that is not given, or one given that it
//! does not name, is an error where it is written. The English is built as
//! statics of `skwir_core::words::Element`, so nothing parses it at run
//! time; the macro gives `skwir_core::words::Words`.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Expr, Ident, LitStr, Token};

/// A message of the crate it is written in: `words!(trash_moved = "Moved
/// { $count -> [one] one item *[other] { $count } items } to the Trash",
/// count = n)`.
#[proc_macro]
pub fn words(input: TokenStream) -> TokenStream {
    let call = syn::parse_macro_input!(input as Call);
    match expand(&call) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// What a call writes: the id and its English, then its arguments and
/// note.
struct Call {
    id: Ident,
    english: LitStr,
    args: Vec<(Ident, Expr)>,
    note: Option<LitStr>,
}

impl Parse for Call {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let id: Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        let english: LitStr = input.parse()?;
        let mut args = Vec::new();
        let mut note = None;
        if input.parse::<Option<Token![,]>>()?.is_some() {
            let rest = Punctuated::<Named, Token![,]>::parse_terminated(input)?;
            for named in rest {
                if named.name == "note" {
                    let Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(text),
                        ..
                    }) = named.value
                    else {
                        return Err(syn::Error::new_spanned(
                            named.value,
                            "a note is a string: what a translator needs to know",
                        ));
                    };
                    note = Some(text);
                } else {
                    args.push((named.name, named.value));
                }
            }
        }
        Ok(Call {
            id,
            english,
            args,
            note,
        })
    }
}

/// `name = value`.
struct Named {
    name: Ident,
    value: Expr,
}

impl Parse for Named {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse()?;
        input.parse::<Token![=]>()?;
        Ok(Named {
            name,
            value: input.parse()?,
        })
    }
}

fn expand(call: &Call) -> syn::Result<Tokens> {
    let id = call.id.to_string();
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(syn::Error::new_spanned(
            &call.id,
            "a message's id is snake_case: what it is for",
        ));
    }
    let english = call.english.value();
    let span = call.english.span();
    let source = format!("{id} = {english}\n");
    let resource =
        fluent_syntax::parser::parse_runtime(source.as_str()).map_err(|(_, errors)| {
            let what: Vec<String> = errors.iter().map(|e| format!("{:?}", e.kind)).collect();
            syn::Error::new(
                span,
                format!("not a message in Fluent's syntax: {}", what.join("; ")),
            )
        })?;
    let mut entries = resource.body.into_iter();
    let (Some(fluent_syntax::ast::Entry::Message(message)), None) =
        (entries.next(), entries.next())
    else {
        return Err(syn::Error::new(span, "the English is one message"));
    };
    if !message.attributes.is_empty() {
        return Err(syn::Error::new(span, "a message here has no attributes"));
    }
    let Some(value) = message.value else {
        return Err(syn::Error::new(span, "a message says something"));
    };
    let mut statics = Vec::new();
    let mut named = Vec::new();
    let root = pattern(&value, &mut statics, &mut named, span)?;
    // Every argument named is given, and every one given is named.
    for name in &named {
        if !call.args.iter().any(|(given, _)| given == name) {
            return Err(syn::Error::new(
                span,
                format!("`${name}` is named but not given"),
            ));
        }
    }
    for (given, _) in &call.args {
        if !named.iter().any(|name| given == name) {
            return Err(syn::Error::new_spanned(
                given,
                format!("`{given}` is given but the English does not name it"),
            ));
        }
    }
    let note = call.note.as_ref().map_or_else(String::new, LitStr::value);
    let args = call.args.iter().map(|(name, value)| {
        let name = name.to_string();
        quote! { (#name, ::skwir_core::words::Arg::from(#value)) }
    });
    // A call, its statics in the block of its first argument: where it is
    // written reads an expression, never a block of statements (a match's
    // arm stays one).
    Ok(quote! {
        ::skwir_core::words::Words::message(
            {
                #(#statics)*
                static MESSAGE: ::skwir_core::words::Message = ::skwir_core::words::Message {
                    domain: ::core::env!("CARGO_PKG_NAME"),
                    id: #id,
                    english: &#root,
                    note: #note,
                };
                &MESSAGE
            },
            ::std::vec![#(#args),*],
        )
    })
}

/// A pattern as a static array of elements, its own name, the statics it
/// needs pushed before it; the arguments it names pushed to `named`.
fn pattern(
    value: &fluent_syntax::ast::Pattern<&str>,
    statics: &mut Vec<Tokens>,
    named: &mut Vec<String>,
    span: Span,
) -> syn::Result<Ident> {
    use fluent_syntax::ast::{Expression, InlineExpression, PatternElement, VariantKey};
    let mut elements = Vec::new();
    for element in &value.elements {
        elements.push(match element {
            PatternElement::TextElement { value } => {
                quote! { ::skwir_core::words::Element::Text(::std::borrow::Cow::Borrowed(#value)) }
            }
            PatternElement::Placeable { expression } => match expression {
                Expression::Inline(InlineExpression::VariableReference { id }) => {
                    let name = id.name;
                    if !named.iter().any(|n| n == name) {
                        named.push(name.to_owned());
                    }
                    quote! { ::skwir_core::words::Element::Arg(::std::borrow::Cow::Borrowed(#name)) }
                }
                Expression::Inline(InlineExpression::StringLiteral { value }) => {
                    let text = unescape(value);
                    quote! { ::skwir_core::words::Element::Text(::std::borrow::Cow::Borrowed(#text)) }
                }
                Expression::Select { selector, variants } => {
                    let InlineExpression::VariableReference { id } = selector else {
                        return Err(syn::Error::new(span, "a selector chooses by an argument"));
                    };
                    let on = id.name;
                    if !named.iter().any(|n| n == on) {
                        named.push(on.to_owned());
                    }
                    let mut chosen = Vec::new();
                    let mut default = 0usize;
                    for (at, variant) in variants.iter().enumerate() {
                        let key = match &variant.key {
                            VariantKey::Identifier { name } => quote! {
                                ::skwir_core::words::Key::Word(::std::borrow::Cow::Borrowed(#name))
                            },
                            VariantKey::NumberLiteral { value } => {
                                let n: i64 = value.parse().map_err(|_| {
                                    syn::Error::new(span, format!("`[{value}]` is no whole number"))
                                })?;
                                quote! { ::skwir_core::words::Key::Number(#n) }
                            }
                        };
                        if variant.default {
                            default = at;
                        }
                        let value = pattern(&variant.value, statics, named, span)?;
                        chosen.push(quote! {
                            ::skwir_core::words::Variant {
                                key: #key,
                                value: ::std::borrow::Cow::Borrowed(&#value),
                            }
                        });
                    }
                    let name = format_ident!("VARIANTS_{}", statics.len());
                    let len = chosen.len();
                    statics.push(quote! {
                        static #name: [::skwir_core::words::Variant; #len] = [#(#chosen),*];
                    });
                    quote! {
                        ::skwir_core::words::Element::Select {
                            on: ::std::borrow::Cow::Borrowed(#on),
                            variants: ::std::borrow::Cow::Borrowed(&#name),
                            default: #default,
                        }
                    }
                }
                Expression::Inline(InlineExpression::FunctionReference { .. }) => {
                    return Err(syn::Error::new(
                        span,
                        "no functions: an argument arrives formatted (a Count, a Size)",
                    ));
                }
                Expression::Inline(InlineExpression::TermReference { .. }) => {
                    return Err(syn::Error::new(span, "no terms (ADR-139, 3)"));
                }
                Expression::Inline(InlineExpression::MessageReference { .. }) => {
                    return Err(syn::Error::new(
                        span,
                        "no message refers to another: give it as a Words argument",
                    ));
                }
                Expression::Inline(_) => {
                    return Err(syn::Error::new(span, "this placeable is not said by Skwir"));
                }
            },
        });
    }
    let name = format_ident!("ELEMENTS_{}", statics.len());
    let len = elements.len();
    statics.push(quote! {
        static #name: [::skwir_core::words::Element; #len] = [#(#elements),*];
    });
    Ok(name)
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
