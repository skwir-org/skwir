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
use bibli_core::inspection::{Align, Script};
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
sup { vertical-align: super }
sub { vertical-align: sub }
center { text-align: center }
pre, listing, xmp, plaintext { white-space: pre }
dd { margin-left: 2em }
p, blockquote, ul, ol, dl, figure { margin-top: 1em; margin-bottom: 1em }
h1 { font-size: 2em; margin-top: 0.67em; margin-bottom: 0.67em }
h2 { font-size: 1.5em; margin-top: 0.83em; margin-bottom: 0.83em }
h3 { font-size: 1.17em; margin-top: 1em; margin-bottom: 1em }
h4 { margin-top: 1.33em; margin-bottom: 1.33em }
h5 { font-size: 0.83em; margin-top: 1.67em; margin-bottom: 1.67em }
h6 { font-size: 0.67em; margin-top: 2.33em; margin-bottom: 2.33em }
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
    /// Raised or lowered with its content, like a decoration.
    pub script: Script,
    pub small_caps: bool,
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
            script: Script::Baseline,
            small_caps: false,
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

#[derive(Clone, Debug)]
enum Property {
    /// What only a fixed-layout page uses (`page.rs`); reading mode leaves
    /// boxes and fonts to the reader.
    Page(Page),
    Display(Display),
    Bold(bool),
    Italic(bool),
    Decoration {
        underline: bool,
        strike: bool,
    },
    Script(Script),
    SmallCaps(bool),
    Align(Align),
    Pre(bool),
    Size(Size),
    Margin(Side, Length),
    Padding(Side, Length),
}

#[derive(Clone, Debug)]
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
    /// The fonts the page's stylesheets bring (`@font-face`).
    pub faces: Vec<FontFace>,
}

/// A font a stylesheet brings: its CSS family and where the files are,
/// relative to the page (a stylesheet's own URLs are relative to it).
#[derive(Clone, Debug, Default)]
pub struct FontFace {
    pub family: String,
    pub src: Vec<String>,
    pub bold: bool,
    pub italic: bool,
}
impl Sheets {
    pub fn new() -> Self {
        let mut sheets = Self::default();
        sheets.parse(USER_AGENT, Origin::UserAgent, "");
        sheets
    }
    /// Add one of the document's stylesheets, found at `base` (the folder
    /// of a linked sheet relative to the page, empty for a `<style>`). Past
    /// the bound, the rest of the document's CSS is left out: the page
    /// still reads, with less styling.
    pub fn add(&mut self, css: &str, base: &str) {
        if self.bytes + css.len() > limits::TEXT_INPUT {
            return;
        }
        self.bytes += css.len();
        self.parse(css, Origin::Author, base);
    }
    fn parse(&mut self, css: &str, origin: Origin, base: &str) {
        let mut input = ParserInput::new(css);
        let mut parser = Parser::new(&mut input);
        let mut rules = Rules {
            origin,
            out: Vec::new(),
            budget: limits::STYLE_RULES.saturating_sub(self.rules.len()),
            depth: 0,
            base: base.to_owned(),
            faces: Vec::new(),
        };
        rules.sheet(&mut parser);
        for rule in rules.out {
            self.insert(rule);
        }
        self.faces.append(&mut rules.faces);
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

    /// The declarations that apply to `element`, weakest first: applying
    /// them in turn leaves each property's winner.
    fn matched(&mut self, element: ElementRef<'_>) -> Vec<Declaration> {
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
                        declaration.clone(),
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
        matched
            .into_iter()
            .map(|(_, declaration)| declaration)
            .collect()
    }

    /// The style of `element`, whose parent's style is `parent`.
    pub fn compute(&mut self, element: ElementRef<'_>, parent: &Style) -> Style {
        let matched = self.matched(element);
        let mut style = parent.inherited();
        let (mut margin, mut padding) = (0., 0.);
        let mut script = None;
        for declaration in matched {
            match declaration.property {
                Property::Display(display) => style.display = display,
                Property::Bold(bold) => style.bold = bold,
                Property::Italic(italic) => style.italic = italic,
                Property::Decoration { underline, strike } => {
                    // `none` cannot take back what an ancestor draws.
                    style.underline = parent.underline || underline;
                    style.strike = parent.strike || strike;
                }
                Property::Script(own) => script = Some(own),
                Property::SmallCaps(small_caps) => style.small_caps = small_caps,
                Property::Align(align) => style.align = align,
                Property::Pre(pre) => style.pre = pre,
                Property::Size(Size::Parent(scale)) => style.size = parent.size * scale,
                Property::Size(Size::Root(scale)) => style.size = scale,
                Property::Margin(Side::Left, length) => margin = ems(length),
                Property::Padding(Side::Left, length) => padding = ems(length),
                Property::Page(_) | Property::Margin(..) | Property::Padding(..) => {}
            }
        }
        // A raised element raises its content; its own `baseline` only
        // undoes what a sheet said about it, not its parent's position.
        if let Some(script) = script.filter(|script| *script != Script::Baseline) {
            style.script = script;
        }
        style.size = style.size.clamp(0.25, 8.);
        style.indent = margin + padding;
        style
    }
}

/// The computed style of an element on a fixed-layout page (`page.rs`): its
/// box, its font and how its text is set. Lengths that depend on a box stay
/// lengths; what CSS inherits as a value (sizes, spacing) is in pixels.
// Each flag is a separate CSS property, computed independently.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug)]
pub struct PageStyle {
    pub display: PageDisplay,
    pub position: Position,
    /// `top right bottom left`, as CSS orders them.
    pub offsets: [Length; 4],
    pub width: Length,
    pub height: Length,
    pub margin: [Length; 4],
    pub padding: [Length; 4],
    pub transform: Option<[f32; 6]>,
    pub origin: (Length, Length),
    pub families: Arc<[String]>,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub line_height: LineHeight,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub color: [u8; 4],
    pub background: Option<[u8; 4]>,
    pub visible: bool,
    pub opacity: f32,
    /// Lines break to fit the box (`white-space` other than `pre`, `nowrap`).
    pub wrap: bool,
    /// Spaces and line breaks are kept as written.
    pub pre: bool,
    pub align: Align,
}
impl Default for PageStyle {
    fn default() -> Self {
        Self {
            display: PageDisplay::Inline,
            position: Position::Static,
            offsets: [Length::Auto; 4],
            width: Length::Auto,
            height: Length::Auto,
            margin: [Length::Px(0.); 4],
            padding: [Length::Px(0.); 4],
            transform: None,
            origin: (Length::Percent(50.), Length::Percent(50.)),
            families: Arc::from([]),
            font_size: 16.,
            bold: false,
            italic: false,
            line_height: LineHeight::Normal,
            letter_spacing: 0.,
            word_spacing: 0.,
            color: [0, 0, 0, 255],
            background: None,
            visible: true,
            opacity: 1.,
            wrap: true,
            pre: false,
            align: Align::Start,
        }
    }
}
impl PageStyle {
    /// What a child starts from: CSS's inherited properties.
    fn inherited(&self) -> Self {
        Self {
            families: self.families.clone(),
            font_size: self.font_size,
            bold: self.bold,
            italic: self.italic,
            line_height: self.line_height,
            letter_spacing: self.letter_spacing,
            word_spacing: self.word_spacing,
            color: self.color,
            visible: self.visible,
            wrap: self.wrap,
            pre: self.pre,
            align: self.align,
            ..Self::default()
        }
    }
}
fn side(side: Side) -> usize {
    match side {
        Side::Top => 0,
        Side::Right => 1,
        Side::Bottom => 2,
        Side::Left => 3,
    }
}

impl Sheets {
    /// The page style of `element`, whose parent's is `parent`.
    pub fn compute_page(&mut self, element: ElementRef<'_>, parent: &PageStyle) -> PageStyle {
        let mut style = parent.inherited();
        let (mut letter, mut word) = (None, None);
        for declaration in self.matched(element) {
            match declaration.property {
                Property::Size(Size::Parent(scale)) => style.font_size = parent.font_size * scale,
                Property::Size(Size::Root(scale)) => style.font_size = scale * 16.,
                Property::Bold(bold) => style.bold = bold,
                Property::Italic(italic) => style.italic = italic,
                Property::Align(align) => style.align = align,
                Property::Pre(pre) => style.pre = pre,
                Property::Margin(at, length) => style.margin[side(at)] = length,
                Property::Padding(at, length) => style.padding[side(at)] = length,
                Property::Page(page) => match page {
                    Page::Display(display) => style.display = display,
                    Page::Position(position) => style.position = position,
                    Page::Offset(at, length) => style.offsets[side(at)] = length,
                    Page::Width(length) => style.width = length,
                    Page::Height(length) => style.height = length,
                    Page::Transform(matrix) => {
                        style.transform = (matrix != IDENTITY).then_some(matrix);
                    }
                    Page::Origin(x, y) => style.origin = (x, y),
                    Page::Families(families) => style.families = families,
                    Page::LineHeight(height) => style.line_height = height,
                    Page::LetterSpacing(length) => letter = Some(length),
                    Page::WordSpacing(length) => word = Some(length),
                    Page::Color(color) => style.color = color,
                    Page::Background(color) => style.background = color,
                    Page::Visible(visible) => style.visible = visible,
                    Page::Opacity(opacity) => style.opacity = opacity,
                    Page::Wrap(wrap) => style.wrap = wrap,
                },
                _ => {}
            }
        }
        // Spacing in ems is of this element's size, and inherited as pixels.
        let font = style.font_size;
        if let Some(px) = letter.and_then(|length| length.px(font, 0.)) {
            style.letter_spacing = px;
        }
        if let Some(px) = word.and_then(|length| length.px(font, 0.)) {
            style.word_spacing = px;
        }
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
    /// Where the sheet is, relative to the page, for its URLs.
    base: String,
    faces: Vec<FontFace>,
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
/// Fonts one page's stylesheets may bring.
const FACES: usize = 64;

/// A URL of a stylesheet at `base`, relative to the page.
fn join(base: &str, url: &str) -> String {
    if base.is_empty() || url.starts_with('/') {
        url.to_owned()
    } else {
        format!("{base}/{url}")
    }
}

/// The descriptors of an `@font-face` rule, as names and values.
struct Descriptors;
impl<'i> DeclarationParser<'i> for Descriptors {
    type Declaration = (String, Vec<Value>);
    type Error = ();
    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _: &ParserState,
    ) -> Result<(String, Vec<Value>), ParseError<'i, ()>> {
        Ok((name.to_ascii_lowercase(), values(input).0))
    }
}
impl AtRuleParser<'_> for Descriptors {
    type Prelude = ();
    type AtRule = (String, Vec<Value>);
    type Error = ();
}
impl QualifiedRuleParser<'_> for Descriptors {
    type Prelude = ();
    type QualifiedRule = (String, Vec<Value>);
    type Error = ();
}
impl RuleBodyItemParser<'_, (String, Vec<Value>), ()> for Descriptors {
    fn parse_declarations(&self) -> bool {
        true
    }
    fn parse_qualified(&self) -> bool {
        false
    }
}

/// An at-rule's prelude: which ones have rules worth reading.
enum At {
    Media(bool),
    FontFace,
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
        if name.eq_ignore_ascii_case("font-face") {
            return Ok(At::FontFace);
        }
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
        } else if let At::FontFace = prelude {
            let mut face = FontFace::default();
            let mut parser = Descriptors;
            for (name, values) in RuleBodyParser::new(input, &mut parser).filter_map(Result::ok) {
                match name.as_str() {
                    "font-family" => {
                        if let Some(Property::Page(Page::Families(families))) =
                            page("font-family", &values).map(Property::Page)
                        {
                            face.family = families.first().cloned().unwrap_or_default();
                        }
                    }
                    "src" => {
                        face.src = values
                            .iter()
                            .filter_map(|value| match value {
                                Value::Url(url) if !url.contains(':') => {
                                    Some(join(&self.base, url))
                                }
                                _ => None,
                            })
                            .collect();
                    }
                    "font-weight" => {
                        face.bold = matches!(property(&name, &values), Some(Property::Bold(true)));
                    }
                    "font-style" => {
                        face.italic =
                            matches!(property(&name, &values), Some(Property::Italic(true)));
                    }
                    _ => {}
                }
            }
            if !face.family.is_empty() && !face.src.is_empty() && self.faces.len() < FACES {
                self.faces.push(face);
            }
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
    /// The properties a declaration sets that anything here keeps: none,
    /// one, or several for a shorthand (`margin`).
    type Declaration = Vec<Declaration>;
    type Error = ();

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _: &ParserState,
    ) -> Result<Vec<Declaration>, ParseError<'i, ()>> {
        let (values, important) = values(input);
        Ok(properties(&name.to_ascii_lowercase(), &values)
            .into_iter()
            .map(|property| Declaration {
                property,
                important,
            })
            .collect())
    }
}
impl AtRuleParser<'_> for Declarations {
    type Prelude = ();
    type AtRule = Vec<Declaration>;
    type Error = ();
}
impl QualifiedRuleParser<'_> for Declarations {
    type Prelude = ();
    type QualifiedRule = Vec<Declaration>;
    type Error = ();
}
impl RuleBodyItemParser<'_, Vec<Declaration>, ()> for Declarations {
    fn parse_declarations(&self) -> bool {
        true
    }
    fn parse_qualified(&self) -> bool {
        false
    }
}

/// A declaration's value, token by token, and whether it is `!important`.
/// Functions keep their arguments (`rgb()`, `matrix()`, `url()`).
fn values(input: &mut Parser<'_, '_>) -> (Vec<Value>, bool) {
    let mut out = Vec::new();
    let mut important = false;
    while let Ok(token) = input.next().cloned() {
        out.push(match token {
            Token::Delim('!') => {
                important = matches!(input.next(), Ok(Token::Ident(i)) if i.eq_ignore_ascii_case("important"));
                continue;
            }
            Token::Ident(ident) => Value::Ident(ident.to_ascii_lowercase()),
            Token::Number { value, .. } => Value::Number(value),
            Token::Dimension { value, unit, .. } => {
                Value::Length(value, unit.to_ascii_lowercase())
            }
            Token::Percentage { unit_value, .. } => Value::Length(unit_value * 100., "%".into()),
            Token::QuotedString(text) => Value::String(text.to_string()),
            Token::UnquotedUrl(url) => Value::Url(url.to_string()),
            Token::Comma => Value::Comma,
            Token::Hash(hex) | Token::IDHash(hex) => Value::Hash(hex.to_ascii_lowercase()),
            Token::Function(name) => {
                let name = name.to_ascii_lowercase();
                let inner = input
                    .parse_nested_block(|input| Ok::<_, ParseError<'_, ()>>(values(input).0))
                    .unwrap_or_default();
                match (name.as_str(), inner.as_slice()) {
                    ("url", [Value::String(url)]) => Value::Url(url.clone()),
                    _ => Value::Function(name, inner),
                }
            }
            _ => Value::Other,
        });
    }
    (out, important)
}

#[derive(Clone, Debug)]
enum Value {
    Ident(String),
    Number(f32),
    Length(f32, String),
    String(String),
    Url(String),
    Hash(String),
    Function(String, Vec<Value>),
    Comma,
    Other,
}

/// A CSS length, resolved where it is used: ems need the element's font
/// size, percentages the box they are of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
    Auto,
}
impl Length {
    fn parse(value: &Value) -> Option<Self> {
        Some(match value {
            Value::Number(zero) if *zero == 0. => Self::Px(0.),
            Value::Ident(auto) if auto == "auto" => Self::Auto,
            Value::Length(value, unit) => match unit.as_str() {
                "px" => Self::Px(*value),
                "pt" => Self::Px(value * 4. / 3.),
                "pc" => Self::Px(value * 16.),
                "in" => Self::Px(value * 96.),
                "cm" => Self::Px(value * 96. / 2.54),
                "mm" => Self::Px(value * 96. / 25.4),
                "em" => Self::Em(*value),
                "rem" => Self::Rem(*value),
                "ex" | "ch" => Self::Em(value / 2.),
                "%" | "vw" | "vh" => Self::Percent(*value),
                _ => return None,
            },
            _ => return None,
        })
    }
    /// In pixels, for an element whose font is `font` pixels, of a box of
    /// `base` pixels; `None` for `auto`.
    pub fn px(self, font: f32, base: f32) -> Option<f32> {
        match self {
            Self::Px(px) => Some(px),
            Self::Em(em) => Some(em * font),
            Self::Rem(rem) => Some(rem * 16.),
            Self::Percent(percent) => Some(percent / 100. * base),
            Self::Auto => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}
const SIDES: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

/// The four values of a box shorthand (`margin: 0 auto`), in CSS order.
fn sides(values: &[Value]) -> Option<[Length; 4]> {
    let lengths: Vec<Length> = values.iter().map(Length::parse).collect::<Option<_>>()?;
    Some(match lengths.as_slice() {
        [all] => [*all; 4],
        [vertical, horizontal] => [*vertical, *horizontal, *vertical, *horizontal],
        [top, horizontal, bottom] => [*top, *horizontal, *bottom, *horizontal],
        [top, right, bottom, left] => [*top, *right, *bottom, *left],
        _ => return None,
    })
}

/// What a fixed-layout page's boxes and text are laid out with.
#[derive(Clone, Debug)]
enum Page {
    Display(PageDisplay),
    Position(Position),
    Offset(Side, Length),
    Width(Length),
    Height(Length),
    /// `transform`: a 2D matrix `a b c d e f` (`none` is the identity).
    Transform([f32; 6]),
    Origin(Length, Length),
    Families(Arc<[String]>),
    LineHeight(LineHeight),
    LetterSpacing(Length),
    WordSpacing(Length),
    Color([u8; 4]),
    Background(Option<[u8; 4]>),
    Visible(bool),
    Opacity(f32),
    Wrap(bool),
}

/// How a box is laid out on a fixed page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageDisplay {
    Block,
    #[default]
    Inline,
    InlineBlock,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LineHeight {
    #[default]
    Normal,
    Number(f32),
    Length(Length),
}

fn properties(name: &str, values: &[Value]) -> Vec<Property> {
    let one = |property: Option<Property>| property.into_iter().collect();
    match name {
        "margin" | "padding" => {
            let Some(sides) = sides(values) else {
                return Vec::new();
            };
            SIDES
                .iter()
                .zip(sides)
                .map(|(side, length)| {
                    if name == "margin" {
                        Property::Margin(*side, length)
                    } else {
                        Property::Padding(*side, length)
                    }
                })
                .collect()
        }
        "display" => [
            property(name, values),
            page(name, values).map(Property::Page),
        ]
        .into_iter()
        .flatten()
        .collect(),
        _ => one(property(name, values).or_else(|| page(name, values).map(Property::Page))),
    }
}

/// A page property, or `None` for one no layout here keeps.
fn page(name: &str, values: &[Value]) -> Option<Page> {
    let ident = || match values {
        [Value::Ident(ident)] => Some(ident.as_str()),
        _ => None,
    };
    let length = || match values {
        [value] => Length::parse(value),
        _ => None,
    };
    Some(match name {
        "display" => Page::Display(match ident()? {
            "none" => PageDisplay::None,
            "inline" | "contents" | "ruby" => PageDisplay::Inline,
            "inline-block" | "inline-flex" | "inline-grid" | "inline-table" => {
                PageDisplay::InlineBlock
            }
            _ => PageDisplay::Block,
        }),
        "position" => Page::Position(match ident()? {
            "absolute" | "fixed" => Position::Absolute,
            "relative" | "sticky" => Position::Relative,
            _ => Position::Static,
        }),
        "left" => Page::Offset(Side::Left, length()?),
        "top" => Page::Offset(Side::Top, length()?),
        "right" => Page::Offset(Side::Right, length()?),
        "bottom" => Page::Offset(Side::Bottom, length()?),
        "width" => Page::Width(length()?),
        "height" => Page::Height(length()?),
        "transform" | "-webkit-transform" => Page::Transform(transform(values)?),
        "transform-origin" | "-webkit-transform-origin" => {
            let mut lengths = values.iter().filter_map(|value| match value {
                Value::Ident(keyword) => match keyword.as_str() {
                    "left" | "top" => Some(Length::Percent(0.)),
                    "center" => Some(Length::Percent(50.)),
                    "right" | "bottom" => Some(Length::Percent(100.)),
                    _ => None,
                },
                value => Length::parse(value),
            });
            let x = lengths.next()?;
            Page::Origin(x, lengths.next().unwrap_or(Length::Percent(50.)))
        }
        "font-family" => {
            let mut families = Vec::new();
            let mut current = String::new();
            for value in values.iter().chain([&Value::Comma]) {
                match value {
                    Value::String(name) => current.push_str(name),
                    Value::Ident(word) => {
                        if !current.is_empty() {
                            current.push(' ');
                        }
                        current.push_str(word);
                    }
                    Value::Comma if !current.is_empty() => {
                        families.push(std::mem::take(&mut current));
                    }
                    _ => {}
                }
            }
            Page::Families(families.into())
        }
        "line-height" => Page::LineHeight(match values {
            [Value::Ident(normal)] if normal == "normal" => LineHeight::Normal,
            [Value::Number(number)] => LineHeight::Number(*number),
            [value] => LineHeight::Length(Length::parse(value)?),
            _ => return None,
        }),
        "letter-spacing" => Page::LetterSpacing(match ident() {
            Some("normal") => Length::Px(0.),
            _ => length()?,
        }),
        "word-spacing" => Page::WordSpacing(match ident() {
            Some("normal") => Length::Px(0.),
            _ => length()?,
        }),
        "color" => Page::Color(color(values)?),
        "background-color" | "background" => Page::Background(
            values
                .iter()
                .find_map(|value| color(std::slice::from_ref(value))),
        ),
        "visibility" => Page::Visible(ident()? != "hidden"),
        "opacity" => Page::Opacity(match values {
            [Value::Number(opacity)] => opacity.clamp(0., 1.),
            [Value::Length(percent, unit)] if unit == "%" => (percent / 100.).clamp(0., 1.),
            _ => return None,
        }),
        "white-space" => Page::Wrap(!matches!(ident()?, "pre" | "nowrap")),
        _ => return None,
    })
}

/// A 2D transform as the matrix `a b c d e f`: `matrix()`, `scale()`,
/// `translate()` and `rotate()`, composed left to right; `none` is the
/// identity, and `None` a transform this layout does not know.
fn transform(values: &[Value]) -> Option<[f32; 6]> {
    if let [Value::Ident(none)] = values
        && none == "none"
    {
        return Some(IDENTITY);
    }
    let mut matrix = IDENTITY;
    for value in values {
        let Value::Function(name, arguments) = value else {
            continue;
        };
        let numbers: Vec<f32> = arguments
            .iter()
            .filter_map(|value| match value {
                Value::Number(number) => Some(*number),
                Value::Length(number, unit) if unit == "px" => Some(*number),
                Value::Length(degrees, unit) if unit == "deg" => Some(degrees.to_radians()),
                Value::Length(radians, unit) if unit == "rad" => Some(*radians),
                Value::Length(turns, unit) if unit == "turn" => Some(turns * std::f32::consts::TAU),
                _ => None,
            })
            .collect();
        let step = match (name.as_str(), numbers.as_slice()) {
            ("matrix", [a, b, c, d, e, f]) => [*a, *b, *c, *d, *e, *f],
            ("scale", [s]) => [*s, 0., 0., *s, 0., 0.],
            ("scale", [x, y]) => [*x, 0., 0., *y, 0., 0.],
            ("scalex", [x]) => [*x, 0., 0., 1., 0., 0.],
            ("scaley", [y]) => [1., 0., 0., *y, 0., 0.],
            ("translate" | "translatex", [x]) => [1., 0., 0., 1., *x, 0.],
            ("translate", [x, y]) => [1., 0., 0., 1., *x, *y],
            ("translatey", [y]) => [1., 0., 0., 1., 0., *y],
            ("rotate", [angle]) => {
                let (sin, cos) = angle.sin_cos();
                [cos, sin, -sin, cos, 0., 0.]
            }
            _ => return None,
        };
        matrix = multiply(matrix, step);
    }
    Some(matrix)
}
pub const IDENTITY: [f32; 6] = [1., 0., 0., 1., 0., 0.];
/// `first` then `second`, as CSS composes a transform list.
pub fn multiply(first: [f32; 6], second: [f32; 6]) -> [f32; 6] {
    let [a, b, c, d, e, f] = first;
    let [g, h, i, j, k, l] = second;
    [
        a * g + c * h,
        b * g + d * h,
        a * i + c * j,
        b * i + d * j,
        a * k + c * l + e,
        b * k + d * l + f,
    ]
}

/// A colour as RGBA: `#rgb`, `#rrggbb`, `rgb()`, `rgba()`, and the few names
/// books use.
fn color(values: &[Value]) -> Option<[u8; 4]> {
    let channel = |value: &Value| match value {
        Value::Number(number) => Some(number.clamp(0., 255.) as u8),
        Value::Length(percent, unit) if unit == "%" => {
            Some((percent / 100. * 255.).clamp(0., 255.) as u8)
        }
        _ => None,
    };
    match values {
        [Value::Hash(hex)] => {
            let digit = |i: usize| u8::from_str_radix(hex.get(i..=i)?, 16).ok();
            match hex.len() {
                3 => Some([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17, 255]),
                6 | 8 => {
                    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
                    Some([byte(0)?, byte(2)?, byte(4)?, byte(6).unwrap_or(255)])
                }
                _ => None,
            }
        }
        [Value::Function(name, arguments)] if name == "rgb" || name == "rgba" => {
            let numbers: Vec<&Value> = arguments
                .iter()
                .filter(|value| !matches!(value, Value::Comma | Value::Other))
                .collect();
            let alpha = match numbers.get(3) {
                Some(Value::Number(alpha)) => (alpha.clamp(0., 1.) * 255.) as u8,
                Some(value) => channel(value)?,
                None => 255,
            };
            Some([
                channel(numbers.first()?)?,
                channel(numbers.get(1)?)?,
                channel(numbers.get(2)?)?,
                alpha,
            ])
        }
        [Value::Ident(name)] => Some(match name.as_str() {
            "black" => [0, 0, 0, 255],
            "white" => [255, 255, 255, 255],
            "red" => [255, 0, 0, 255],
            "green" => [0, 128, 0, 255],
            "blue" => [0, 0, 255, 255],
            "gray" | "grey" => [128, 128, 128, 255],
            "transparent" => [0, 0, 0, 0],
            _ => return None,
        }),
        _ => None,
    }
}

/// A reading-mode property, or `None` for one reading mode does not keep.
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
        "vertical-align" => Property::Script(match ident()? {
            "super" | "top" | "text-top" => Script::Superscript,
            "sub" | "bottom" | "text-bottom" => Script::Subscript,
            "baseline" => Script::Baseline,
            _ => return None,
        }),
        "font-variant" | "font-variant-caps" => Property::SmallCaps(match ident()? {
            "small-caps" | "all-small-caps" | "petite-caps" | "all-petite-caps" => true,
            "normal" => false,
            _ => return None,
        }),
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
        "margin-left" | "margin-inline-start" => {
            Property::Margin(Side::Left, Length::parse(values.first()?)?)
        }
        "margin-top" => Property::Margin(Side::Top, Length::parse(values.first()?)?),
        "margin-right" => Property::Margin(Side::Right, Length::parse(values.first()?)?),
        "margin-bottom" => Property::Margin(Side::Bottom, Length::parse(values.first()?)?),
        "padding-left" | "padding-inline-start" => {
            Property::Padding(Side::Left, Length::parse(values.first()?)?)
        }
        "padding-top" => Property::Padding(Side::Top, Length::parse(values.first()?)?),
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
/// A reading margin as ems of the body. Percentages are of the column, taken
/// as a reading measure of about 40 ems.
fn ems(length: Length) -> f32 {
    match length {
        Length::Px(px) => px / 16.,
        Length::Em(em) | Length::Rem(em) => em,
        Length::Percent(percent) => percent * 0.4,
        Length::Auto => 0.,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Html;

    fn style_of(html: &str, css: &str, id: &str) -> Style {
        let doc = Html::parse_document(html);
        let mut sheets = Sheets::new();
        sheets.add(css, "");
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
        sheets.add(&css, "");
        assert_eq!(sheets.rules.len(), limits::STYLE_RULES);
    }
}
