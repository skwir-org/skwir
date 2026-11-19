# Vendored jpeg-decoder 0.3.2

The `jpeg-decoder` 0.3.2 crate as published on crates.io (MIT OR
Apache-2.0, `LICENSE-MIT`, `LICENSE-APACHE`), plus Skwir's patches, wired
in through `[patch.crates-io]` in the root `Cargo.toml`.

Skwir decodes a JPEG with it only where it asks for a smaller scale:
whatever `picture::decode_fitted` decodes smaller than it is
(`crates/platform/src/picture.rs`): thumbnails, the preview, a comic's
pages, print's contact sheets. Every other JPEG is `image`'s, through
zune-jpeg.

The crate is in maintenance mode: image-rs moves to zune-jpeg, and takes
changes here only for lossless JPEGs, so the patches are likely to stay
here. It is kept because zune-jpeg decodes a JPEG only at its full size:
with zune-jpeg 0.5.15, the thumbnail of a synthetic 5464×8192 photo of
3.1 MB took 173–185 ms (the whole decode, then a resize from 45
megapixels), against 60–64 ms here, and held 134 MB of pixels against
2 MB; a progressive one 353–369 ms, holding as much, where here one of
5.5 MB takes 19–28 ms and holds 7 MB (`0001`, `0003`). Its
`jpeg_set_max_scans` is a limit that fails the decode, not a stop. 0.5.16 (release candidates in 2026) adds an
incremental mode and previews of a progressive file, still at full size.
Remove this copy, and decode every JPEG through zune-jpeg, once it
decodes at a reduced scale.

Left out:
- the registry's `.cargo-ok`;
- `rust-toolchain`, which would have rustup install Rust 1.61 for whoever
  runs cargo here;
- the benchmarks' photographs (`benches/*.jpg`), three of which hold the
  GPS position and the phone of whoever took them;
- the crate's `Cargo.lock`, which its own `.gitignore` keeps out of the
  repository (a copy unpacked again has it, ignored).

Skwir runs neither this crate's tests nor its benchmarks.

## Patches

Diffs in `../patches/jpeg-decoder/`, in the upstream repository's layout
(its `Cargo.toml` is the published `Cargo.toml.orig`), so they apply there
with `git apply -p1`, one after the other, onto the published crate: they
make this copy. The published, normalized `Cargo.toml` here carries the
same changes.

- `0001-an-eighth-from-the-dc-scans.patch`: a progressive JPEG decoded at
  an eighth stops once every component's DC coefficients have come.
  - **Why it holds:** at that scale a block is drawn from its DC
    coefficient alone (a 1×1 IDCT), and what the later scans bring (the AC
    coefficients and the DC's lowest bits) changes nothing but those bits
    (see `0003` for them). What has come is drawn as the decoder already
    draws an unfinished file.
  - **The gain:** the first scans are about a tenth of a photo's file, so
    the rest is neither read nor decoded. A synthetic 5464×8192
    progressive photo of 3.2 MB, decoded at an eighth for its thumbnail,
    went from 256–314 ms to 62–64 ms (release), as long as a standard JPEG
    of its size takes; one whose DC came whole in its first scans from
    104–118 ms to 52 ms.
  - **Tests:** `crates/platform/src/picture.rs` checks it on
    `crates/platform/testdata/synthetic-progressive.jpg`, cut after its
    first scan.
- `0002-asmjs-is-a-target.patch`: `asmjs` is named as a value of
  `target_arch` the crate tests (its worker leaves out threads there as on
  WebAssembly). rustc no longer knows that target, and warned of it 8 times
  in every build of Skwir.
- `0003-an-eighth-keeps-the-dc-alone.patch`: a progressive JPEG decoded at
  an eighth keeps one coefficient a block, its DC, and draws it halfway
  where bits of it are still to come.
  - **What it keeps:** the decoder set aside 64 coefficients a block for
    the whole picture, two bytes a pixel of each component (134 MB for
    a photo of 5464×8192, as much as zune-jpeg's pixels), though an
    eighth reads one. An AC scan that comes before the last DC (the format
    allows a component's AC before another's DC) is passed over to the
    next marker, never decoded. The DC is drawn as the 1×1 IDCT draws
    it, without the worker. A synthetic 5464×8192 progressive photo of
    5.5 MB at an eighth, release, alternated, the machine loaded: 49–68 ms
    and 133 MB with `0001` alone became 19–28 ms and 7 MB (397–454 ms as
    published); in 4:4:4, 122–135 ms and 263 MB became 38–42 ms and
    10 MB.
  - **What changes:** where the first DC scan leaves out low bits
    (successive approximation `Al`, 1 bit in libjpeg's default
    progression), what was kept is the whole DC with them cleared, at or
    below it: `0001` drew it so, every picture darker and its colours
    shifted, by up to `(2^Al − 1)·Q/8` levels of 255 a component, `Q`
    its table's DC step. Each block is now drawn halfway between the
    least and the most its whole DC may draw, half of them rounding up
    (a chessboard): a component's sample is off by at most half as much,
    `(2^Al − 1)·Q/16` rounded up, and by none on average. A plasma at
    quality 30 (`Al` = 1): a pixel moved up to 10 levels, 4 darker on
    average; now up to 6, and less than 0.2 on average either way; at
    quality 80, 3 levels, as before, and none on average where it was
    0.9 darker. A standard JPEG, and a progressive one whose DC came
    whole, decode exactly as before.
  - **Tests:** `crates/platform/src/picture.rs` decodes at an eighth a
    plasma's coefficients written four ways by jpegtran
    (`crates/platform/testdata/synthetic-plasma-*.jpg`): progressive with
    every DC whole first, or the luma's AC before the colours' DC, draws
    exactly the sequential one; libjpeg's progression stays within 6
    levels, and half a level on average.
- `0004-a-decode-starts-no-thread.patch`: a decode runs on the thread
  that asked for it. The crate started a thread for each component for
  every picture wider than 128 pixels once decoded (its threshold read
  the width twice): three or four threads a thumbnail, with one decode a
  scheduler worker already, which leaves room for listings and previews.
  A standard JPEG of 5464×8192, alternated with the crate's threads: at
  an eighth and a quarter as long (99–125 ms against 101–117), 10–20 %
  less processor time; at a half, one decode alone takes 15–20 % longer
  (122–136 ms against 141–163), in 20 % less processor time, holding
  80 MB against 106.
