//! Mermaid diagrams laid out as SVG, in a process of its own: the layout
//! engine is large, and bibli pays for it on disk only where this helper
//! is installed. Reads a diagram's source on standard input and writes the
//! SVG on standard output, made for resvg (labels as SVG text, nothing
//! that could read a file or a URL). Anything else is an exit status of 1
//! and a reason on standard error. Pure Rust: no browser, nothing written.
use std::io::{Read, Write};
use std::time::Duration;

/// A diagram's source read at most: a diagram is a few lines.
const SOURCE: u64 = 64 * 1024;
/// How long one layout may take; the caller has a deadline of its own.
const DEADLINE: Duration = Duration::from_secs(5);

fn main() {
    if let Err(reason) = run() {
        eprintln!("{reason}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    use merman::{OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};
    let mut source = String::new();
    std::io::stdin()
        .take(SOURCE + 1)
        .read_to_string(&mut source)
        .map_err(|error| format!("reading the diagram: {error}"))?;
    if source.len() as u64 > SOURCE {
        return Err(format!("the diagram is longer than {SOURCE} bytes"));
    }
    let request = SvgRequest {
        pipeline: Some(merman::svg::SvgPipeline::resvg_safe()),
        ..SvgRequest::default()
    };
    let control = OperationControl::new().with_deadline(DEADLINE);
    match Renderer::new().render(RenderRequest::svg(&source, control, request)) {
        Ok(RenderOutput::Svg(Some(svg))) => std::io::stdout()
            .lock()
            .write_all(svg.svg().as_bytes())
            .map_err(|error| format!("writing the drawing: {error}")),
        Ok(_) => Err("no Mermaid diagram found".into()),
        Err(error) => Err(error.to_string()),
    }
}
