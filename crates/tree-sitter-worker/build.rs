// Which build of skwir this program is (`SKWIR_BUILD`).
include!("../../tools/build/identity.rs");

fn main() {
    emit_build();
    emit_tree_sitter_version();
    build_fixture();
}

/// The tree-sitter library's version, which the helper's greeting says
/// (`SKWIR_TREE_SITTER_VERSION`): its crate's, read from the manifest
/// beside the headers it tells the crates linking it of.
fn emit_tree_sitter_version() {
    let version = std::env::var_os("DEP_TREE_SITTER_INCLUDE")
        .and_then(|include| {
            let manifest = std::path::Path::new(&include).parent()?.join("Cargo.toml");
            let text = std::fs::read_to_string(manifest).ok()?;
            let package = text.split("\n[").find(|part| {
                part.starts_with("package]") || part.trim_start().starts_with("[package]")
            })?;
            package.lines().find_map(|line| {
                let value = line
                    .strip_prefix("version")?
                    .trim_start()
                    .strip_prefix('=')?;
                Some(value.trim().trim_matches('"').to_owned())
            })
        })
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo::rustc-env=SKWIR_TREE_SITTER_VERSION={version}");
}

/// The tests' grammar: tree-sitter-json's parser (`tests/fixtures/json`)
/// and a symbol whose language says an ABI no tree-sitter reads
/// (`tests/fixtures/wrong-abi.c`), built into one shared library, on
/// Linux, whose path the tests read in `SKWIR_TEST_GRAMMAR`. Without a C
/// compiler the helper is built all the same, and the tests that need it
/// say why they cannot run.
fn build_fixture() {
    let fixtures = std::path::Path::new("tests/fixtures");
    println!(
        "cargo::rerun-if-changed={}",
        fixtures.join("json/src").display()
    );
    println!(
        "cargo::rerun-if-changed={}",
        fixtures.join("wrong-abi.c").display()
    );
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let Some(out) = std::env::var_os("OUT_DIR").map(std::path::PathBuf::from) else {
        return;
    };
    let library = out.join("libtree-sitter-json.so");
    let compiler = cc::Build::new()
        .pic(true)
        .cargo_metadata(false)
        .cargo_warnings(false)
        .try_get_compiler();
    let built = compiler
        .map_err(|error| error.to_string())
        .and_then(|compiler| {
            let mut command = compiler.to_command();
            command
                .arg("-shared")
                .arg("-o")
                .arg(&library)
                .arg("-I")
                .arg(fixtures.join("json/src"))
                .arg(fixtures.join("json/src/parser.c"))
                .arg(fixtures.join("wrong-abi.c"));
            match command.output() {
                Ok(output) if output.status.success() => Ok(()),
                Ok(output) => Err(String::from_utf8_lossy(&output.stderr).into_owned()),
                Err(error) => Err(error.to_string()),
            }
        });
    match built {
        Ok(()) => println!("cargo::rustc-env=SKWIR_TEST_GRAMMAR={}", library.display()),
        Err(error) => {
            let said = error.lines().next().unwrap_or_default().to_owned();
            println!("cargo::warning=the tests' grammar was not built: {said}");
            println!("cargo::rustc-env=SKWIR_TEST_GRAMMAR_MISSING={said}");
        }
    }
}
