//! The part of a document's own styling that reading mode keeps.
//!
//! A cascade over scraper's tree, in the order a browser applies it: a
//! built-in stylesheet that says what the tags mean (`USER_AGENT`), then the
//! document's stylesheets (linked and `<style>`), then each element's own
//! `style` attribute, with `!important` declarations above all of them.
//! Only what says how text reads is computed: whether an element shows or
//! makes a block, emphasis, decoration, alignment, preserved line breaks,
//! relative size and left margins. Fonts, colours, absolute sizes and boxes
//! remain the reader's, as in any reading system that offers "publisher
//! styles": a book that carries its italics in a class (as Calibre and
//! `InDesign` write them) keeps them, and still reads in the theme.
//!
//! Rules are indexed by the rightmost id, class or tag of their selector, so
//! an element is only matched against the rules that could apply to it.
//! `@media` blocks apply only when they are for every medium or for
//! screens without conditions; `@import`, `@font-face` and `@page` are not
//! followed. Every stylesheet and the number of rules are bounded.
use crate::inspection::limits;
use bibli_core::inspection::Align;
use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser, ParserInput, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, StyleSheetParser, Token,
};
use scraper::{ElementRef, selector::Simple};
use selectors::{
    matching::{
        MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
        SelectorCaches, matches_selector,
    },
    parser::{Component, ParseRelative, Selector, SelectorList, SelectorParseErrorKind},
};
use std::{collections::HashMap, sync::Arc};

/// What the tags mean before a document says anything: which elements make
/// blocks, which are never shown, and the emphasis HTML gives its tags.
/// Everything not listed is inline, as in CSS.
const USER_AGENT: &str = "
html, body, div, p, h1, h2, h3, h4, h5, h6, blockquote, pre, ul, ol, li, dl, dt, dd,
section, article, aside, nav, header, footer, main, figure, figcaption, address, hgroup,
details, summary, table, caption, tr, center, form, fieldset, legend, listing, xmp,
plaintext { display: block }
head, script, style, title, template, iframe, object, noscript { display: none }
b, strong, th { font-weight: bold }
i, em, cite, var, dfn, figcaption { font-style: italic }
del, s, strike { text-decoration: line-through }
u, ins { text-decoration: underline }
center { text-align: center }
pre, listing, xmp, plaintext { white-space: pre }
dd { margin-left: 2em }
";

/// How an element takes part in the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    Inline,
    /// It makes a block of its own: a paragraph, a heading, a row.
    Block,
    None,
}

/// The computed style of an element, as far as reading mode keeps it.
// Each flag is a separate CSS property, computed independently.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub display: Display,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub align: Align,
    /// Line breaks and spaces are kept as written (`white-space: pre*`).
    pub pre: bool,
    /// Font size relative to the document's root.
    pub size: f32,
    /// This element's own left margin and padding, in ems.
    pub indent: f32,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            display: Display::Inline,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            align: Align::Start,
            pre: false,
            size: 1.,
            indent: 0.,
        }
    }
}
impl Style {
    /// What a child starts from: the inherited properties, and the
    /// decorations, which CSS draws through descendants rather than inherits.
    fn inherited(&self) -> Self {
        Self {
            display: Display::Inline,
            indent: 0.,
            ..*self
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Size {
    /// Relative to the parent's size (em, %, smaller, larger, keywords).
    Parent(f32),
    /// Relative to the root (rem, and absolute lengths against a 16 px root).
    Root(f32),
}

#[derive(Clone, Copy, Debug)]
enum Property {
    Display(Display),
    Bold(bool),
    Italic(bool),
    Decoration { underline: bool, strike: bool },
    Align(Align),
    Pre(bool),
    Size(Size),
    MarginLeft(f32),
    PaddingLeft(f32),
}

#[derive(Clone, Copy, Debug)]
struct Declaration {
    property: Property,
    important: bool,
}

/// Where a declaration comes from, lowest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    UserAgent,
    Author,
    Inline,
}

struct Rule {
    selector: Selector<Simple>,
    declarations: Arc<[Declaration]>,
    origin: Origin,
}

/// The rules of every stylesheet a page uses, indexed for matching.
#[derive(Default)]
pub struct Sheets {
    rules: Vec<Rule>,
    by_id: HashMap<String, Vec<usize>>,
    by_class: HashMap<String, Vec<usize>>,
    by_tag: HashMap<String, Vec<usize>>,
    universal: Vec<usize>,
    /// Bytes of author CSS read so far, against `limits::TEXT_INPUT`.
    bytes: usize,
    caches: SelectorCaches,
}
impl Sheets {
    pub fn new() -> Self {
        let mut sheets = Self::default();
        sheets.parse(USER_AGENT, Origin::UserAgent);
        sheets
    }
    /// Add one of the document's stylesheets. Past the bound, the rest of the
    /// document's CSS is left out: the page still reads, with less styling.
    pub fn add(&mut self, css: &str) {
        if self.bytes + css.len() > limits::TEXT_INPUT {
            return;
        }
        self.bytes += css.len();
        self.parse(css, Origin::Author);
    }
    fn parse(&mut self, css: &str, origin: Origin) {
        let mut input = ParserInput::new(css);
        let mut parser = Parser::new(&mut input);
        let mut rules = Rules {
            origin,
            out: Vec::new(),
            budget: limits::STYLE_RULES.saturating_sub(self.rules.len()),
            depth: 0,
        };
        rules.sheet(&mut parser);
        for rule in rules.out {
            self.insert(rule);
        }
    }
    fn insert(&mut self, rule: Rule) {
        let index = self.rules.len();
        // The rightmost compound decides which elements can match at all;
        // the most specific of its parts is the one to index by.
        let (mut id, mut class, mut tag) = (None, None, None);
        for component in rule.selector.iter() {
            match component {
                Component::ID(name) => id = Some(name.0.to_string()),
                Component::Class(name) => class = Some(name.0.to_string()),
                Component::LocalName(name) => tag = Some(name.lower_name.0.to_string()),
                _ => {}
            }
        }
        let bucket = if let Some(id) = id {
            self.by_id.entry(id).or_default()
        } else if let Some(class) = class {
            self.by_class.entry(class).or_default()
        } else if let Some(tag) = tag {
            self.by_tag.entry(tag).or_default()
        } else {
            &mut self.universal
        };
        bucket.push(index);
        self.rules.push(rule);
    }

    /// The style of `element`, whose parent's style is `parent`.
    pub fn compute(&mut self, element: ElementRef<'_>, parent: &Style) -> Style {
        let e = element.value();
        let mut candidates: Vec<usize> = self.universal.clone();
        if let Some(id) = e.id()
            && let Some(rules) = self.by_id.get(id)
        {
            candidates.extend(rules);
        }
        for class in e.classes() {
            if let Some(rules) = self.by_class.get(class) {
                candidates.extend(rules);
            }
        }
        if let Some(rules) = self.by_tag.get(&e.name().to_ascii_lowercase()) {
            candidates.extend(rules);
        }
        candidates.sort_unstable();
        candidates.dedup();
        let mut context = MatchingContext::new(
            MatchingMode::Normal,
            None,
            &mut self.caches,
            QuirksMode::NoQuirks,
            NeedsSelectorFlags::No,
            MatchingForInvalidation::No,
        );
        // (important, origin, specificity, order) orders the declarations
        // from weakest to strongest; applying them in turn leaves the winner.
        let mut matched: Vec<((bool, Origin, u32, usize), Declaration)> = Vec::new();
        for index in candidates {
            let rule = &self.rules[index];
            if matches_selector(&rule.selector, 0, None, &element, &mut context) {
                let specificity = rule.selector.specificity();
                for declaration in rule.declarations.iter() {
                    matched.push((
                        (declaration.important, rule.origin, specificity, index),
                        *declaration,
                    ));
                }
            }
        }
        if let Some(style) = e.attr("style") {
            for declaration in declarations(style) {
                matched.push((
                    (declaration.important, Origin::Inline, 0, usize::MAX),
                    declaration,
                ));
            }
        }
        // HTML's presentational `align` sits under every stylesheet rule.
        if let Some(align) = e.attr("align").and_then(align) {
            matched.push((
                (false, Origin::UserAgent, 0, 0),
                Declaration {
                    property: Property::Align(align),
                    important: false,
                },
            ));
        }
        matched.sort_by_key(|(key, _)| *key);
        let mut style = parent.inherited();
        let (mut margin, mut padding) = (0., 0.);
        for (_, declaration) in matched {
            match declaration.property {
                Property::Display(display) => style.display = display,
                Property::Bold(bold) => style.bold = bold,
                Property::Italic(italic) => style.italic = italic,
                Property::Decoration { underline, strike } => {
                    // `none` cannot take back what an ancestor draws.
                    style.underline = parent.underline || underline;
                    style.strike = parent.strike || strike;
                }
                Property::Align(align) => style.align = align,
                Property::Pre(pre) => style.pre = pre,
                Property::Size(Size::Parent(scale)) => style.size = parent.size * scale,
                Property::Size(Size::Root(scale)) => style.size = scale,
                Property::MarginLeft(ems) => margin = ems,
                Property::PaddingLeft(ems) => padding = ems,
            }
        }
        style.size = style.size.clamp(0.25, 8.);
        style.indent = margin + padding;
        style
    }
}

/// The rules of one stylesheet (or of an `@media` block inside one).
struct Rules {
    origin: Origin,
    out: Vec<Rule>,
    budget: usize,
    /// `@media` blocks inside `@media` blocks, bounded like HTML nesting.
    depth: usize,
}
impl Rules {
    fn sheet(&mut self, input: &mut Parser<'_, '_>) {
        let parsed: Vec<_> = StyleSheetParser::new(input, self).collect();
        // Errors are rules the reader does not know: they are skipped, as a
        // browser skips them.
        drop(parsed);
    }
}

const MEDIA_DEPTH: usize = 4;

/// An at-rule's prelude: which ones have rules worth reading.
enum At {
    Media(bool),
    Other,
}

impl<'i> AtRuleParser<'i> for Rules {
    type Prelude = At;
    type AtRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<At, ParseError<'i, ()>> {
        if name.eq_ignore_ascii_case("media") {
            let start = input.position();
            while input.next().is_ok() {}
            let query = input.slice_from(start);
            // A reading pane is a screen of any size: only an unconditional
            // query for it is certain to apply. Kindle's `amzn-*` blocks and
            // width breakpoints are not.
            let applies = query.split(',').all(|medium| {
                let medium = medium.trim();
                medium.eq_ignore_ascii_case("all") || medium.eq_ignore_ascii_case("screen")
            });
            return Ok(At::Media(applies));
        }
        while input.next().is_ok() {}
        Ok(At::Other)
    }
    fn rule_without_block(&mut self, _: At, _: &ParserState) -> Result<(), ()> {
        Ok(())
    }
    fn parse_block<'t>(
        &mut self,
        prelude: At,
        _: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<(), ParseError<'i, ()>> {
        if let At::Media(true) = prelude
            && self.depth < MEDIA_DEPTH
        {
            self.depth += 1;
            self.sheet(input);
            self.depth -= 1;
        } else {
            while input.next().is_ok() {}
        }
        Ok(())
    }
}

impl<'i> QualifiedRuleParser<'i> for Rules {
    type Prelude = SelectorList<Simple>;
    type QualifiedRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<SelectorList<Simple>, ParseError<'i, ()>> {
        SelectorList::parse(&Selectors, input, ParseRelative::No)
            .map_err(|error| error.location.new_custom_error(()))
    }
    fn parse_block<'t>(
        &mut self,
        selectors: SelectorList<Simple>,
        _: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<(), ParseError<'i, ()>> {
        let declarations: Arc<[Declaration]> = block(input).into();
        if declarations.is_empty() {
            return Ok(());
        }
        for selector in selectors.slice() {
            if self.budget == 0 {
                break;
            }
            self.budget -= 1;
            self.out.push(Rule {
                selector: selector.clone(),
                declarations: declarations.clone(),
                origin: self.origin,
            });
        }
        Ok(())
    }
}

/// Selector syntax without `:has()`, whose matching cost grows with the
/// whole subtree of every element it is asked about.
struct Selectors;
impl<'i> selectors::parser::Parser<'i> for Selectors {
    type Impl = Simple;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_is_and_where(&self) -> bool {
        true
    }
}

/// The declarations of a rule's block or of a `style` attribute.
fn block(input: &mut Parser<'_, '_>) -> Vec<Declaration> {
    let mut parser = Declarations;
    RuleBodyParser::new(input, &mut parser)
        .filter_map(Result::ok)
        .flatten()
        .collect()
}
fn declarations(style: &str) -> Vec<Declaration> {
    let mut input = ParserInput::new(style);
    block(&mut Parser::new(&mut input))
}

struct Declarations;
impl<'i> DeclarationParser<'i> for Declarations {
    /// `None` for a property reading mode does not keep.
    type Declaration = Option<Declaration>;
    type Error = ();

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _: &ParserState,
    ) -> Result<Option<Declaration>, ParseError<'i, ()>> {
        let mut values: Vec<Value> = Vec::new();
        let mut important = false;
        while let Ok(token) = input.next().cloned() {
            match token {
                Token::Delim('!') => {
                    important = matches!(input.next(), Ok(Token::Ident(i)) if i.eq_ignore_ascii_case("important"));
                }
                Token::Ident(ident) => values.push(Value::Ident(ident.to_ascii_lowercase())),
                Token::Number { value, .. } => values.push(Value::Number(value)),
                Token::Dimension { value, unit, .. } => {
                    values.push(Value::Length(value, unit.to_ascii_lowercase()));
                }
                Token::Percentage { unit_value, .. } => {
                    values.push(Value::Length(unit_value * 100., "%".into()));
                }
                _ => values.push(Value::Other),
            }
        }
        Ok(
            property(&name.to_ascii_lowercase(), &values).map(|property| Declaration {
                property,
                important,
            }),
        )
    }
}
impl AtRuleParser<'_> for Declarations {
    type Prelude = ();
    type AtRule = Option<Declaration>;
    type Error = ();
}
impl QualifiedRuleParser<'_> for Declarations {
    type Prelude = ();
    type QualifiedRule = Option<Declaration>;
    type Error = ();
}
impl RuleBodyItemParser<'_, Option<Declaration>, ()> for Declarations {
    fn parse_declarations(&self) -> bool {
        true
    }
    fn parse_qualified(&self) -> bool {
        false
    }
}

#[derive(Debug)]
enum Value {
    Ident(String),
    Number(f32),
    Length(f32, String),
    Other,
}

fn property(name: &str, values: &[Value]) -> Option<Property> {
    let ident = || match values {
        [Value::Ident(ident)] => Some(ident.as_str()),
        _ => None,
    };
    Some(match name {
        "display" => Property::Display(match ident()? {
            "none" => Display::None,
            "block" | "list-item" | "flex" | "grid" | "flow-root" | "table" | "table-row"
            | "table-caption" => Display::Block,
            "inline" | "inline-block" | "inline-flex" | "inline-grid" | "inline-table"
            | "contents" | "table-cell" | "ruby" => Display::Inline,
            _ => return None,
        }),
        "font-weight" => Property::Bold(match values {
            [Value::Ident(weight)] => match weight.as_str() {
                "bold" | "bolder" => true,
                "normal" | "lighter" => false,
                _ => return None,
            },
            [Value::Number(weight)] => *weight >= 600.,
            _ => return None,
        }),
        "font-style" => Property::Italic(match ident()? {
            "italic" | "oblique" => true,
            "normal" => false,
            _ => return None,
        }),
        "text-decoration" | "text-decoration-line" => {
            let mut underline = false;
            let mut strike = false;
            for value in values {
                match value {
                    Value::Ident(line) if line == "underline" => underline = true,
                    Value::Ident(line) if line == "line-through" => strike = true,
                    _ => {}
                }
            }
            Property::Decoration { underline, strike }
        }
        "text-align" => Property::Align(align(ident()?)?),
        "white-space" => Property::Pre(match ident()? {
            "pre" | "pre-wrap" | "pre-line" | "break-spaces" => true,
            "normal" | "nowrap" => false,
            _ => return None,
        }),
        "font-size" => Property::Size(match values {
            [Value::Ident(keyword)] => match keyword.as_str() {
                "xx-small" => Size::Root(0.6),
                "x-small" => Size::Root(0.75),
                "small" => Size::Root(0.89),
                "medium" => Size::Root(1.),
                "large" => Size::Root(1.2),
                "x-large" => Size::Root(1.5),
                "xx-large" => Size::Root(2.),
                "smaller" => Size::Parent(1. / 1.2),
                "larger" => Size::Parent(1.2),
                _ => return None,
            },
            [Value::Length(value, unit)] => match unit.as_str() {
                "em" => Size::Parent(*value),
                "%" => Size::Parent(value / 100.),
                "rem" => Size::Root(*value),
                "px" => Size::Root(value / 16.),
                "pt" => Size::Root(value / 12.),
                _ => return None,
            },
            _ => return None,
        }),
        "margin-left" | "margin-inline-start" => Property::MarginLeft(ems(values.first()?)?),
        "padding-left" | "padding-inline-start" => Property::PaddingLeft(ems(values.first()?)?),
        // The shorthands name the left side last, or repeat an earlier one.
        "margin" => Property::MarginLeft(ems(left(values)?)?),
        "padding" => Property::PaddingLeft(ems(left(values)?)?),
        _ => return None,
    })
}
fn align(value: &str) -> Option<Align> {
    Some(match value.to_ascii_lowercase().as_str() {
        "left" | "start" | "justify" => Align::Start,
        "center" | "-webkit-center" => Align::Center,
        "right" | "end" => Align::End,
        _ => return None,
    })
}
fn left(values: &[Value]) -> Option<&Value> {
    match values {
        [all] | [_, all] | [_, all, _] => Some(all),
        [_, _, _, left] => Some(left),
        _ => None,
    }
}
/// A length as ems of the body. Percentages are of the column, taken as a
/// reading measure of about 40 ems.
fn ems(value: &Value) -> Option<f32> {
    match value {
        Value::Number(zero) if *zero == 0. => Some(0.),
        Value::Ident(auto) if auto == "auto" => Some(0.),
        Value::Length(value, unit) => Some(match unit.as_str() {
            "em" | "rem" => *value,
            "px" => value / 16.,
            "pt" => value / 12.,
            "%" => value * 0.4,
            "ex" | "ch" => value / 2.,
            _ => return None,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Html;

    fn style_of(html: &str, css: &str, id: &str) -> Style {
        let doc = Html::parse_document(html);
        let mut sheets = Sheets::new();
        sheets.add(css);
        // Walk from the root so inheritance is what the page gives.
        let mut styles: HashMap<ego_tree::NodeId, Style> = HashMap::new();
        for node in doc.tree.root().descendants() {
            let Some(element) = ElementRef::wrap(node) else {
                continue;
            };
            let parent = node
                .parent()
                .and_then(|p| styles.get(&p.id()).copied())
                .unwrap_or_default();
            let style = sheets.compute(element, &parent);
            if element.value().id() == Some(id) {
                return style;
            }
            styles.insert(node.id(), style);
        }
        panic!("no element #{id}");
    }

    #[test]
    fn classes_carry_emphasis_and_the_cascade_orders_it() {
        let html = r#"<p class="calibre3"><span class="calibre5" id="it">never</span>
            <span class="calibre5 plain" id="plain">x</span>
            <em class="up" id="em">y</em><b id="b" style="font-weight: normal">z</b></p>"#;
        let css = ".calibre5 { font-style: italic } p .plain { font-style: normal }
            .up { font-style: normal !important } em.up { font-style: italic }";
        assert!(style_of(html, css, "it").italic);
        assert!(
            !style_of(html, css, "plain").italic,
            "a more specific selector wins"
        );
        assert!(
            !style_of(html, css, "em").italic,
            "important beats specificity and the tag's own meaning"
        );
        assert!(!style_of(html, css, "b").bold, "inline style is strongest");
    }

    #[test]
    fn display_alignment_size_and_margins() {
        let html = r#"<body><div class="c" id="c">T<span id="s" class="big">x</span></div>
            <p class="gone" id="gone">x</p><span class="line" id="line">x</span>
            <div class="letter" id="letter"><p id="inner" style="margin: 0 0 0 1em">x</p></div>
            <p align="right" id="right">x</p><pre id="pre">x</pre></body>"#;
        let css = ".c { text-align: center; font-size: 2em } .big { font-size: 50% }
            .gone { display: none } .line { display: block }
            .letter { margin-left: 5% ; padding-left: 16px }
            @media amzn-kf8 { .c { text-align: right } }
            @media screen { .line { font-weight: bold } }";
        let c = style_of(html, css, "c");
        assert_eq!(
            (c.align, c.display, c.size),
            (Align::Center, Display::Block, 2.)
        );
        let s = style_of(html, css, "s");
        assert_eq!(
            (s.align, s.size),
            (Align::Center, 1.),
            "inherited, then relative"
        );
        assert_eq!(style_of(html, css, "gone").display, Display::None);
        let line = style_of(html, css, "line");
        assert!(line.display == Display::Block && line.bold);
        assert!((style_of(html, css, "letter").indent - 3.).abs() < 1e-4);
        assert!((style_of(html, css, "inner").indent - 1.).abs() < 1e-4);
        assert_eq!(style_of(html, css, "right").align, Align::End);
        assert!(style_of(html, css, "pre").pre);
    }

    #[test]
    fn unknown_css_is_skipped_not_fatal() {
        let html = r#"<p id="p" class="a">x</p>"#;
        let css = "@import url(x.css); @font-face { src: url(a;b.ttf) }
            p:has(span) { font-weight: bold } ::selection { color: red }
            .a { color: red; font-style: italic; background: url(data:image/png;base64,AA) }
            .a { garbage }";
        let style = style_of(html, css, "p");
        assert!(style.italic && !style.bold);
    }

    #[test]
    fn rules_are_bounded() {
        let css = (0..limits::STYLE_RULES + 10).fold(String::new(), |mut css, i| {
            use std::fmt::Write;
            let _ = write!(css, ".r{i} {{ font-style: italic }}");
            css
        });
        let mut sheets = Sheets::new();
        sheets.add(&css);
        assert_eq!(sheets.rules.len(), limits::STYLE_RULES);
    }
}
