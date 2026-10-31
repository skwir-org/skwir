# tree-sitter-json, for the helper's tests

From [tree-sitter-json](https://github.com/tree-sitter/tree-sitter-json)
at tag `v0.24.8` (commit `ee35a6ebefcef0c5c416c0d1ccec7370cfca5a24`),
MIT (`LICENSE`), unchanged:

- `src/parser.c` and `src/tree_sitter/parser.h`: the generated parser
  (language ABI 14), built into a shared library by the helper's
  `build.rs` on Linux, with `../wrong-abi.c`.
- `queries/highlights.scm`.
- `tree-sitter-json.wasm`: the release's WebAssembly build of the same
  grammar (`releases/download/v0.24.8/tree-sitter-json.wasm`).

The folds, outline and injection queries the tests use are written by
the tests.
