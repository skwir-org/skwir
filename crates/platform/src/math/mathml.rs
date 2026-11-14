//! MathML's presentation markup written as TeX, for a formula that carries
//! no TeX of its own (an EPUB's, a page MathJax drew and saved): what the
//! helper typesets (ADR-129). Unicode passes as it is, as KaTeX and RaTeX
//! read it (`×`, `≤`, `α`); what TeX means otherwise (`{`, `_`, `\`) is
//! escaped. An element this does not know makes the formula unknown
//! (`None`), which then reads as before: never half a formula.
use ego_tree::NodeRef;
use scraper::Node;

/// How deep a formula's markup is read at most.
const DEPTH: usize = 64;

/// The TeX `math` is written as, if every element of it is known.
pub fn tex(math: NodeRef<'_, Node>) -> Option<String> {
    let mut out = String::new();
    row(&elements(math), &mut out, 0)?;
    let tex = out.trim();
    (!tex.is_empty()).then(|| tex.to_owned())
}

/// An element's child elements, in order: the white space between them is
/// the markup's layout.
fn elements(node: NodeRef<'_, Node>) -> Vec<NodeRef<'_, Node>> {
    node.children()
        .filter(|child| child.value().is_element())
        .collect()
}

fn name(node: NodeRef<'_, Node>) -> &str {
    node.value().as_element().map_or("", |e| e.name())
}

fn attr<'a>(node: NodeRef<'a, Node>, key: &str) -> Option<&'a str> {
    node.value().as_element()?.attr(key)
}

/// An element's text, its white space trimmed.
fn text(node: NodeRef<'_, Node>) -> String {
    let text: String = node
        .descendants()
        .filter_map(|child| child.value().as_text().map(|text| &**text))
        .collect();
    text.trim().to_owned()
}

/// A command, set apart from what follows so `\times x` stays two.
fn command(out: &mut String, command: &str) {
    out.push_str(command);
    out.push(' ');
}

/// Text written into a formula: TeX's own characters escaped, the rest as
/// it is.
fn escaped(text: &str, out: &mut String) {
    for letter in text.chars() {
        match letter {
            '{' | '}' | '#' | '$' | '%' | '&' | '_' => {
                out.push('\\');
                out.push(letter);
            }
            '\\' => command(out, "\\backslash"),
            '^' => command(out, "\\wedge"),
            '~' => command(out, "\\sim"),
            // Invisible: function application, times, separator, plus.
            '\u{2061}'..='\u{2064}' => {}
            _ => out.push(letter),
        }
    }
}

/// Text written into `\text{…}`: TeX's own characters escaped as text
/// mode has them, the rest as it is.
fn text_escaped(text: &str, out: &mut String) {
    for letter in text.chars() {
        match letter {
            '{' | '}' | '#' | '$' | '%' | '&' | '_' => {
                out.push('\\');
                out.push(letter);
            }
            '\\' => command(out, "\\textbackslash"),
            '^' => command(out, "\\textasciicircum"),
            '~' => command(out, "\\textasciitilde"),
            '\u{2061}'..='\u{2064}' => {}
            _ => out.push(letter),
        }
    }
}

/// A fence an `mo` is, for `\left` and `\right`.
fn fence(node: NodeRef<'_, Node>) -> Option<&'static str> {
    if name(node) != "mo" {
        return None;
    }
    delimiter(&text(node))
}

/// The delimiter `\left` or `\right` takes for a fence written `text`,
/// if TeX has one.
fn delimiter(text: &str) -> Option<&'static str> {
    Some(match text {
        "(" => "(",
        ")" => ")",
        "[" => "[",
        "]" => "]",
        "{" => "\\{",
        "}" => "\\}",
        "|" => "|",
        "‖" => "\\|",
        "⟨" | "〈" => "\\langle",
        "⟩" | "〉" => "\\rangle",
        "⌈" => "\\lceil",
        "⌉" => "\\rceil",
        "⌊" => "\\lfloor",
        "⌋" => "\\rfloor",
        _ => return None,
    })
}

/// A row of elements; one opened and closed by fences is set between
/// `\left` and `\right`, which grow with it.
fn row(items: &[NodeRef<'_, Node>], out: &mut String, depth: usize) -> Option<()> {
    if let [first, middle @ .., last] = items
        && let (Some(open), Some(close)) = (fence(*first), fence(*last))
    {
        command(out, &format!("\\left{open}"));
        for item in middle {
            element(*item, out, depth)?;
        }
        command(out, &format!("\\right{close}"));
        return Some(());
    }
    for item in items {
        element(*item, out, depth)?;
    }
    Some(())
}

/// `node` braced: an argument of what holds it.
fn group(node: Option<NodeRef<'_, Node>>, out: &mut String, depth: usize) -> Option<()> {
    out.push('{');
    element(node?, out, depth + 1)?;
    out.push('}');
    Some(())
}

/// The functions TeX has a command of.
const FUNCTIONS: [&str; 30] = [
    "sin", "cos", "tan", "cot", "sec", "csc", "sinh", "cosh", "tanh", "coth", "arcsin", "arccos",
    "arctan", "log", "ln", "lg", "exp", "lim", "limsup", "liminf", "max", "min", "sup", "inf",
    "det", "deg", "dim", "gcd", "ker", "arg",
];

/// The large operators whose limits go under and over them.
fn takes_limits(node: NodeRef<'_, Node>) -> bool {
    let text = text(node);
    match name(node) {
        "mo" => matches!(
            text.as_str(),
            "∑" | "∏"
                | "∐"
                | "∫"
                | "∬"
                | "∭"
                | "∮"
                | "⋃"
                | "⋂"
                | "⨁"
                | "⨂"
                | "⋁"
                | "⋀"
                | "lim"
        ),
        "mi" => matches!(text.as_str(), "lim" | "max" | "min" | "sup" | "inf" | "det"),
        _ => false,
    }
}

/// The accent an `mo` over or under a base is, as TeX names it.
fn accent(node: NodeRef<'_, Node>, over: bool) -> Option<&'static str> {
    if name(node) != "mo" {
        return None;
    }
    Some(match (text(node).as_str(), over) {
        ("^" | "ˆ" | "\u{302}", true) => "\\hat",
        ("~" | "˜" | "\u{303}", true) => "\\tilde",
        ("¯" | "‾" | "_" | "\u{304}" | "\u{305}", true) => "\\overline",
        ("→" | "\u{20d7}", true) => "\\vec",
        ("˙" | "\u{307}", true) => "\\dot",
        ("¨" | "\u{308}", true) => "\\ddot",
        ("⏞", true) => "\\overbrace",
        ("¯" | "‾" | "_" | "\u{332}", false) => "\\underline",
        ("⏟", false) => "\\underbrace",
        _ => return None,
    })
}

/// An identifier in its letters' style: one letter italic, a word (`sin`)
/// upright, as MathML draws them, unless `mathvariant` says otherwise.
fn identifier(node: NodeRef<'_, Node>, out: &mut String) {
    let word = text(node);
    let variant = attr(node, "mathvariant");
    let style = match variant {
        Some("bold") => Some("\\mathbf"),
        Some("double-struck") => Some("\\mathbb"),
        Some("script" | "bold-script") => Some("\\mathcal"),
        Some("fraktur" | "bold-fraktur") => Some("\\mathfrak"),
        Some("sans-serif") => Some("\\mathsf"),
        Some("monospace") => Some("\\mathtt"),
        Some("bold-italic") => Some("\\boldsymbol"),
        Some("normal") => Some("\\mathrm"),
        Some("italic") if word.chars().count() > 1 => Some("\\mathit"),
        // A function TeX names (`\sin`, `\lim`) is its command: upright,
        // spaced as an operator, its limits where TeX sets them.
        None if FUNCTIONS.contains(&word.as_str()) => {
            command(out, &format!("\\{word}"));
            return;
        }
        _ if word.chars().count() > 1 => Some("\\operatorname"),
        _ => None,
    };
    match style {
        Some(style) => {
            out.push_str(style);
            out.push('{');
            escaped(&word, out);
            out.push('}');
        }
        None => escaped(&word, out),
    }
}

fn element(node: NodeRef<'_, Node>, out: &mut String, depth: usize) -> Option<()> {
    if depth > DEPTH {
        return None;
    }
    let args = elements(node);
    match name(node) {
        "math" | "mrow" | "mstyle" | "mpadded" | "merror" | "maction" => {
            row(&args, out, depth + 1)?;
        }
        // Its markup, not what it is annotated with.
        "semantics" => element(*args.first()?, out, depth + 1)?,
        "annotation" | "annotation-xml" | "none" | "mprescripts" => {}
        "mi" => identifier(node, out),
        "mn" | "mo" => escaped(&text(node), out),
        "mtext" | "ms" => {
            let quoted = name(node) == "ms";
            out.push_str("\\text{");
            if quoted {
                out.push('"');
            }
            text_escaped(&text(node), out);
            if quoted {
                out.push('"');
            }
            out.push('}');
        }
        "mspace" => command(out, "\\;"),
        "msup" => {
            group(args.first().copied(), out, depth)?;
            out.push('^');
            group(args.get(1).copied(), out, depth)?;
        }
        "msub" => {
            group(args.first().copied(), out, depth)?;
            out.push('_');
            group(args.get(1).copied(), out, depth)?;
        }
        "msubsup" => {
            group(args.first().copied(), out, depth)?;
            out.push('_');
            group(args.get(1).copied(), out, depth)?;
            out.push('^');
            group(args.get(2).copied(), out, depth)?;
        }
        "mfrac" => {
            let bare = attr(node, "linethickness")
                .is_some_and(|thickness| thickness.trim_end_matches("px") == "0");
            out.push_str(if bare {
                "\\genfrac{}{}{0pt}{}"
            } else {
                "\\frac"
            });
            group(args.first().copied(), out, depth)?;
            group(args.get(1).copied(), out, depth)?;
        }
        "msqrt" => {
            out.push_str("\\sqrt{");
            row(&args, out, depth + 1)?;
            out.push('}');
        }
        "mroot" => {
            // Braced, so a `]` of the index does not end it.
            out.push_str("\\sqrt[");
            group(args.get(1).copied(), out, depth)?;
            out.push(']');
            group(args.first().copied(), out, depth)?;
        }
        "mover" | "munder" => {
            let over = name(node) == "mover";
            let (base, mark) = (*args.first()?, *args.get(1)?);
            if let Some(accent) = accent(mark, over) {
                out.push_str(accent);
                group(Some(base), out, depth)?;
            } else if takes_limits(base) {
                // Unbraced: braced, an operator is a letter, its limits
                // beside it.
                element(base, out, depth + 1)?;
                out.push(if over { '^' } else { '_' });
                group(Some(mark), out, depth)?;
            } else {
                out.push_str(if over { "\\overset" } else { "\\underset" });
                group(Some(mark), out, depth)?;
                group(Some(base), out, depth)?;
            }
        }
        "munderover" => {
            let base = *args.first()?;
            if takes_limits(base) {
                element(base, out, depth + 1)?;
                out.push('_');
                group(args.get(1).copied(), out, depth)?;
                out.push('^');
                group(args.get(2).copied(), out, depth)?;
            } else {
                out.push_str("\\overset");
                group(args.get(2).copied(), out, depth)?;
                out.push_str("{\\underset");
                group(args.get(1).copied(), out, depth)?;
                group(Some(base), out, depth)?;
                out.push('}');
            }
        }
        "mtable" => {
            out.push_str("\\begin{matrix}");
            for (index, line) in args.iter().enumerate() {
                if !matches!(name(*line), "mtr" | "mlabeledtr") {
                    return None;
                }
                if index > 0 {
                    out.push_str(" \\\\ ");
                }
                // A labelled row's first cell is its label.
                let skip = usize::from(name(*line) == "mlabeledtr");
                for (column, cell) in elements(*line).into_iter().skip(skip).enumerate() {
                    if column > 0 {
                        out.push_str(" & ");
                    }
                    row(&elements(cell), out, depth + 1)?;
                }
            }
            out.push_str("\\end{matrix}");
        }
        "mfenced" => {
            let open = attr(node, "open").unwrap_or("(");
            let close = attr(node, "close").unwrap_or(")");
            let separators: Vec<char> = attr(node, "separators")
                .unwrap_or(",")
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            // A fence TeX has, or none (`.`); one it has not makes the
            // formula unknown.
            let side = |fence: &str| match fence.trim() {
                "" => Some("."),
                fence => delimiter(fence),
            };
            command(out, &format!("\\left{}", side(open)?));
            for (index, item) in args.iter().enumerate() {
                if index > 0
                    && let Some(separator) = separators.get(index - 1).or(separators.last())
                {
                    escaped(&separator.to_string(), out);
                }
                element(*item, out, depth + 1)?;
            }
            command(out, &format!("\\right{}", side(close)?));
        }
        "menclose" => {
            let notation = attr(node, "notation").unwrap_or("longdiv");
            let wrap = if notation.contains("box") || notation.contains("roundedbox") {
                Some("\\boxed")
            } else if notation.contains("strike") {
                Some("\\cancel")
            } else {
                None
            };
            if let Some(wrap) = wrap {
                out.push_str(wrap);
                out.push('{');
            }
            row(&args, out, depth + 1)?;
            if wrap.is_some() {
                out.push('}');
            }
        }
        "mphantom" => {
            out.push_str("\\phantom{");
            row(&args, out, depth + 1)?;
            out.push('}');
        }
        _ => return None,
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tex_of(markup: &str) -> Option<String> {
        let html = scraper::Html::parse_fragment(markup);
        let math = html
            .tree
            .nodes()
            .find(|node| name(*node) == "math")
            .expect("a math element");
        tex(math)
    }

    #[test]
    fn mathml_is_written_as_the_tex_it_draws() {
        let cases = [
            ("<math><mi>x</mi><mo>=</mo><mn>2</mn></math>", "x=2"),
            (
                "<math><msup><mi>x</mi><mn>2</mn></msup><mo>+</mo><msub><mi>y</mi><mi>i</mi></msub></math>",
                "{x}^{2}+{y}_{i}",
            ),
            (
                "<math><mfrac><mn>1</mn><msqrt><mi>π</mi></msqrt></mfrac></math>",
                "\\frac{1}{\\sqrt{π}}",
            ),
            (
                "<math><mi>sin</mi><mo>\u{2061}</mo><mi>θ</mi></math>",
                "\\sin θ",
            ),
            (
                "<math><munderover><mo>∑</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow><mi>n</mi></munderover><msub><mi>a</mi><mi>i</mi></msub></math>",
                "∑_{i=1}^{n}{a}_{i}",
            ),
            (
                "<math><mover><mi>v</mi><mo>→</mo></mover><mo>×</mo><mover><mi>x</mi><mo>^</mo></mover></math>",
                "\\vec{v}×\\hat{x}",
            ),
            (
                "<math><mrow><mo>(</mo><mtable><mtr><mtd><mn>1</mn></mtd><mtd><mn>0</mn></mtd></mtr><mtr><mtd><mn>0</mn></mtd><mtd><mn>1</mn></mtd></mtr></mtable><mo>)</mo></mrow></math>",
                "\\left( \\begin{matrix}1 & 0 \\\\ 0 & 1\\end{matrix}\\right)",
            ),
            (
                "<math><mi mathvariant=\"double-struck\">R</mi><mtext>for all {x}</mtext></math>",
                "\\mathbb{R}\\text{for all \\{x\\}}",
            ),
            (
                "<math><mfenced><mi>a</mi><mi>b</mi></mfenced></math>",
                "\\left( a,b\\right)",
            ),
            // What the markup holds is escaped, as text mode or math mode
            // has it, never read as TeX.
            (
                "<math><mtext>a\\b ^c ~d {e}</mtext></math>",
                "\\text{a\\textbackslash b \\textasciicircum c \\textasciitilde d \\{e\\}}",
            ),
            (
                "<math><mroot><mi>x</mi><mrow><mo>]</mo></mrow></mroot></math>",
                "\\sqrt[{]}]{x}",
            ),
            (
                "<math><mfenced open=\"{\" close=\"\" separators=\"&amp;\"><mi>a</mi><mi>b</mi></mfenced></math>",
                "\\left\\{ a\\&b\\right.",
            ),
        ];
        for (markup, expected) in cases {
            assert_eq!(tex_of(markup).as_deref(), Some(expected), "{markup}");
        }
    }

    #[test]
    fn an_element_unknown_leaves_the_formula_as_it_reads() {
        assert_eq!(tex_of("<math><mi>x</mi><mglyph/></math>"), None);
        // A fence TeX has not, written by the page, is not TeX to read.
        assert_eq!(
            tex_of("<math><mfenced open=\"\\input{x}\"><mi>a</mi></mfenced></math>"),
            None
        );
        assert_eq!(tex_of("<math></math>"), None);
        // Its annotation is not its markup.
        assert_eq!(
            tex_of("<math><semantics><mi>y</mi><annotation encoding=\"text/plain\">why</annotation></semantics></math>")
                .as_deref(),
            Some("y")
        );
    }
}
