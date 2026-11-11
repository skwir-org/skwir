# ADR-129: Formulas are typeset by a helper and drawn in their lines

Status: accepted 2026-11-10 with the user. Built on Linux; the helper is
in Linux's and Nix's packages, apart, and in Windows' and macOS's since
2026-11-11 (amendments).
Amends ADR-041 (a package of its own, `skwir-math`), ADR-064 (gpui patch
0060) and ADR-124 (`withMath`).

## Context

Notebooks, READMEs and books write formulas in TeX (`$…$`, `$$…$$`, a
MathML element's TeX annotation); previews showed the TeX, or MathML's
markup as loose letters. Drawing them takes a typesetter with math fonts,
and a line of text that can hold a drawing: the reading view's text is
gpui's, whose lines hold only glyphs.

Measured in a scratch program on this machine, 2026-11-10:

| Typesetter | What | Size added (stripped) | Per formula |
| --- | --- | --- | --- |
| RaTeX 0.1.15 (MIT, Rust) | KaTeX's TeX and layout, mhchem, KaTeX's fonts (OFL, 0.5 MB) | 3.9 MB parser and layout, 4.9 MB with SVG out | about 1 ms, 6 ms with `\ce` |
| latex-rust 3.0 | STIX Two Math, math mode only | not measured | 4 stars, Rust 1.99 |
| Typst through MiTeX | a whole typesetter | far larger | not measured |
| LaTeX to MathML (pulldown-latex, math-core) | MathML, which still needs drawing | | |

RaTeX refuses broken TeX and a macro that expands for ever with a reason,
and bounds its depth and its output. ADR-128 measured that each MiB of
code costs about 0.73 MiB idle, used or not.

## Decision

1. **A helper of its own typesets** (`skwir-math-worker`, pure Rust:
   RaTeX, its fonts embedded), optional as Mermaid's is: Linux's packages
   ship it in `skwir-math`, Nix's under `withMath` (on). It reads a page's
   formulas as JSON on its standard input and answers each with an SVG of
   outlines and its box in ems (width, height above the baseline, depth
   below, and the padding the SVG reaches past it), or why it cannot;
   run isolated as every helper reading a file's content is.
2. **One batch a page.** A page's formulas wait in it as drawings to come
   (`math::Pending`) and are typeset together once it is read, after the
   blocks past its bound are cut: one call for a notebook, its cells and
   all. At most `math::FORMULAS` a page; each refused, past the bound, or
   without the helper, stays as it is written, as code (a formula on a
   line of its own, the code block of its TeX).
3. **A formula is a character of its line.** It stands in its block's
   text as the object replacement character, its run holding the drawing
   (`inspection::Drawing`); it reads as its TeX where the text is copied
   or read aloud (`Block::said`), and moves with the text it sits in. A
   formula set apart is a centered block of its own.
4. **Lines hold boxes** (gpui patch 0060): a run in `RunPosition::Inline`
   is boxes the layout places, wraps and selects as characters, paints no
   glyph for, and reports (`TextLayout::inline_boxes`); a text holding one
   taller than its line grows its lines. The view paints each drawing
   there as a mask in the text's ink: the text's color, a link's, the
   selection's, a ground's, at the text's size, at any scale. Formulas
   are set at 1.21 em of their text, as KaTeX sets them.
5. **Where formulas are:** Markdown's `$…$` and `$$…$$` (pulldown-cmark's
   rules: a price is not one); a notebook's cells by Jupyter's rules
   (JupyterLab's `removeMath`: `$`, `$$`, `\\(`, `\\[`, environments, a
   `$` in code no delimiter), taken out before its Markdown is read, and
   its results' LaTeX; HTML's MathML that carries its TeX (pandoc's,
   LaTeXML's, KaTeX's), else its `alttext`.

## Consequences

- Formulas read as Jupyter, GitHub and KaTeX show them, with no browser,
  no network and no font installed; the main program carries no
  typesetter.
- Without the helper every formula is its TeX, as before, set as code.
- A formula is drawn in one ink: `\color` is not kept.
- A formula is not searched by its TeX: Find reads the text drawn.
- In a debug build, rust-embed reads the fonts from the crate's folder,
  which the helper, isolated, cannot see: it embeds them there too
  (`debug-embed`); without them RaTeX had fallen back to the system's
  faces, at their sizes.

## Amendments

### 2026-11-11: numbers by `\tag` alone, at the line's end; MathJax's pages

Typeset one at a time, every numbered equation of a page read "(1)":
RaTeX numbers `equation`, `align`, `gather` and `alignat` itself, from 1
each formula. JupyterLab sets no `tags`, so MathJax's default (`'none'`)
holds there: such an environment is not numbered, and a number is the
one a `\tag` writes; GitHub cannot be configured otherwise.

- The helper typesets those environments as their starred forms: no
  equation is numbered by itself, in Markdown, a notebook or HTML.
- A number comes apart from its formula: the helper lays the formula out
  again without its numbers (a `\tag`, an environment's), splits its
  drawing where that ends, and answers the number as a `tag` of its own
  (`inspection::Tag`) on the formula's baseline. The view sets it at the
  end of the line the formula is set apart on, or an em past the
  formula where that line has no room, as KaTeX's stylesheet sets one.

An HTML page that loads MathJax, or KaTeX's auto-render, writes its TeX
in its text for them to typeset in the browser (an exported notebook,
Sphinx's and MkDocs' pages). Its text is read by MathJax's rules
(`math::delimited::mathjax`): `\(…\)`, `\[…\]`, `$$…$$` and environments,
`$…$` only where the page's configuration lists it (nbconvert's does),
never in `pre` or `code`. The finder is the one a notebook's cells use
(`math::delimited`, `JUPYTER`), moved into the platform for both.

The tag of a hand-written `\tag{A}` is set in math italic by RaTeX where
KaTeX sets it upright; numbers read the same either way.

### 2026-11-11: a formula beside a drawing of itself, once

A page that drew its formulas keeps two copies of each: MathML that
carries its TeX, for the ear, and a drawing marked `aria-hidden`, for the
eye (KaTeX's spans, which its stylesheet lays out; MathJax 3's glyphs;
Wikipedia's picture, its MathML `display: none`). The reader showed both,
the spans as loose letters. Where the two stand just beside each other,
the TeX is typeset in the drawing's place, whichever copy the page hid,
as the reader draws neither the spans nor a picture it does not fetch
(`rich_preview::pairs`). A drawing with no formula beside it (KaTeX's
`html` output) stays, shown and not said, as KaTeX marks it.

### 2026-11-11: MathML without its TeX

An EPUB, or a page MathJax drew and saved, often carries MathML alone,
which read as loose letters ("x2+y2"). Its presentation markup is now
written as TeX (`math::mathml`) and typeset like any formula: identifiers
in their letters' style (a function TeX names as its command), numbers
and operators as their Unicode, which RaTeX reads, TeX's own characters
escaped; groups, fences (`\left`, `\right`), scripts, fractions, roots,
accents, limits (a large operator unbraced, so TeX places them), tables,
enclosures. An element it does not know leaves the formula as it read:
never half a formula. A TeX annotation, where there is one, comes first.

### 2026-11-11: a formula grows its own row; GitHub's `math` blocks

A formula taller than its text made every line of its paragraph as tall
as it (decision 4): a paragraph holding one integral read double-spaced.
A row is now one line high, and a row holding a box taller than its text
grows by what the box needs above the text and below it, in whole pixels,
its text keeping its place one line high in it, as a browser grows a line
box (gpui 0060 amended: `LineLayout::rows`, `LineRows`). A background,
the selection's included, fills its row. Where something is set by a row
(a found match's spot, a note opened under its reference, page mode's
cut between two rows), it asks the text's rows
(`TextLayout::row_bounds_for_index`, `row_edges`), not its line height.

Markdown's fenced `math` block (GitHub's) is a formula set apart, as
`$$…$$` is; in a notebook's cell it stays code, as JupyterLab shows it.

### 2026-11-11: what a notebook's cells return; GitHub's inline form; Windows and macOS

A notebook's outputs were read by the rules of what they are, not of
JupyterLab, which renders them: an HTML result's `$x$` stayed text (the
result loads no MathJax, JupyterLab's own typesets it), a Markdown
result's math was Markdown's. Now an HTML result is read by MathJax's
rules as JupyterLab sets them (`$` included; `\$` a dollar, its
`processEscapes`; nothing marked `mathjax_ignore` or `tex2jax_ignore`,
as pandas marks a frame with `display.html.use_mathjax` off), and a
Markdown result as a cell is. The HTML reader is told so by its caller
(`rich_preview::html_with`, `Math`), and its formulas join the caller's
batch: a notebook is still typeset in one call, its outputs and the HTML
in a Markdown file included.

Markdown's `` $`…`$ ``, GitHub's other inline form, was typeset with its
backticks as TeX; it is the formula between them, copied as written. A
notebook's cells have no such form, as JupyterLab has not.

Windows' and macOS's packages carry the helper: Linux's keep it apart
so that its weight is the user's choice, and these have one package
each, with no way to add it apart, so formulas there would never be
drawn. It links nothing and runs only for a page that holds formulas:
it weighs about 5 MB and costs nothing idle. Mermaid's stays out.

