//! The template as a program that speaks the protocol on its standard
//! input and output (`[run] program`), for bibli to start.

fn main() -> std::io::Result<()> {
    bibli_plugin_api::guest::stdio::serve(&bibli_outside_template::WordCount)
}
