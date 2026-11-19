# Vendored jpeg-decoder 0.3.2

The `jpeg-decoder` 0.3.2 crate as published on crates.io (MIT OR
Apache-2.0, `LICENSE-MIT`, `LICENSE-APACHE`), plus Skwir's patch, wired in
through `[patch.crates-io]` in the root `Cargo.toml`. Remove both once the
patch is upstream (image-rs/jpeg-decoder).

Skwir decodes a JPEG with it only where it asks for a smaller scale (a
thumbnail, a preview smaller than the picture:
`crates/platform/src/picture.rs`). Every other JPEG is `image`'s, through
zune-jpeg.

Left out:
- the registry's `.cargo-ok`;
- `rust-toolchain`, which would have rustup install Rust 1.61 for whoever
  runs cargo here;
- the benchmarks' photographs (`benches/*.jpg`), three of which hold the
  GPS position and the phone of whoever took them.

Skwir runs neither this crate's tests nor its benchmarks.

## Patches

Diffs in `../patches/jpeg-decoder/`, in the upstream repository's layout
(its `Cargo.toml` is the published `Cargo.toml.orig`), so they apply there
with `git apply -p1`. The published, normalized `Cargo.toml` here carries
the same changes.

- `0001-an-eighth-from-the-dc-scans.patch`: a progressive JPEG decoded at
  an eighth stops once every component's DC coefficients have come.
  - **Why it holds:** at that scale a block is drawn from its DC
    coefficient alone (a 1×1 IDCT), and what the later scans bring (the AC
    coefficients and the DC's lowest bits) changes nothing but those bits.
    What has come is drawn as the decoder already draws an unfinished
    file.
  - **The gain:** the first scans are about a tenth of a photo's file, so
    the rest is neither read nor decoded. A synthetic 5464×8192
    progressive photo of 3.2 MB, decoded at an eighth for its thumbnail,
    went from 256–314 ms to 62–64 ms (release), as long as a standard JPEG
    of its size takes; one whose DC came whole in its first scans from
    104–118 ms to 52 ms. What is left is mostly the coefficients the
    decoder sets aside for the whole picture, 64 a block, though an
    eighth reads one.
  - **What changes:** where the first DC scan leaves out low bits
    (successive approximation, 1 bit in libjpeg's default progression),
    a pixel may differ by at most 3 levels of 255 from the whole decode
    at an eighth (0.7 on average). A standard JPEG, and a progressive one
    whose DC came whole, decode exactly as before.
  - **Tests:** `crates/platform/src/picture.rs` checks it on
    `crates/platform/testdata/synthetic-progressive.jpg`, cut after its
    first scan.
- `0002-asmjs-is-a-target.patch`: `asmjs` is named as a value of
  `target_arch` the crate tests (its worker leaves out threads there as on
  WebAssembly). rustc no longer knows that target, and warned of it 8 times
  in every build of Skwir.
