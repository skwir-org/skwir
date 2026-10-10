//! The template as a program that speaks the protocol on its standard
//! input and output (`[run] program`), for skwir to start.

fn main() -> std::io::Result<()> {
    skwir_plugin_api::guest::stdio::serve(&skwir_outside_template::WordCount)
}
