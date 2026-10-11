//! Deterministic, idempotent tree generation.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::profiles::Profile;

const EXTS: [&str; 8] = ["txt", "rs", "png", "pdf", "md", "jpg", "toml", "log"];

fn marker(dir: &Path) -> PathBuf {
    dir.join(".skwir-bench-complete")
}

/// Generate `profile` under `root` unless a completion marker already exists.
pub fn generate(root: &Path, profile: Profile) -> Result<PathBuf> {
    let dir = root.join(profile.name());
    if !marker(&dir).exists() {
        create(&dir, profile)?;
    }
    tag(&dir, profile)?;
    Ok(dir)
}

fn create(dir: &Path, profile: Profile) -> Result<()> {
    eprintln!(
        "bench: generating {} ({} files)...",
        profile.name(),
        profile.file_count()
    );
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::create_dir_all(dir)?;
    match profile {
        Profile::Flat100k => flat(dir, 100_000)?,
        Profile::Deep1m => deep(dir)?,
        Profile::Images10k => images(dir, 10_000)?,
    }
    fs::write(marker(dir), b"")?;
    Ok(())
}

/// The tag every tagged bench file carries, which the searches look for.
pub const TAG: &str = "bench";

/// Give some files notes, as a user's would have: every tenth image (tags,
/// a rating and a comment) and one file in a thousand of the deep tree (a
/// tag), so the provider and `tag:` are measured over files that mostly
/// have none. Idempotent, with a marker of its own, so trees generated
/// before it are tagged in place. A filesystem without user attributes
/// leaves the trees as they are: the measurements that need them report
/// nothing.
fn tag(dir: &Path, profile: Profile) -> Result<()> {
    let every = match profile {
        Profile::Images10k => 10,
        Profile::Deep1m => 1000,
        Profile::Flat100k => return Ok(()),
    };
    let done = dir.join(".skwir-bench-tagged");
    if done.exists() {
        return Ok(());
    }
    let probe = dir.join(".skwir-bench-probe");
    fs::write(&probe, b"")?;
    let supported = skwir_core::fs::xattr::set(&probe, "user.xdg.tags", TAG.as_bytes()).is_ok();
    fs::remove_file(&probe)?;
    if !supported {
        eprintln!(
            "bench: no user attributes under {}, left untagged",
            dir.display()
        );
        return Ok(());
    }
    eprintln!("bench: tagging {}...", profile.name());
    let files = files(dir);
    files
        .par_iter()
        .enumerate()
        .filter(|(i, _)| i % every == 0)
        .try_for_each(|(i, path)| -> Result<()> {
            let tags = if i % 20 == 0 {
                format!("{TAG},even")
            } else {
                format!("{TAG},odd")
            };
            skwir_core::fs::xattr::set(path, "user.xdg.tags", tags.as_bytes())?;
            if profile == Profile::Images10k {
                let rating = (i / every) % 10 + 1;
                skwir_core::fs::xattr::set(
                    path,
                    "user.baloo.rating",
                    rating.to_string().as_bytes(),
                )?;
                skwir_core::fs::xattr::set(path, "user.xdg.comment", b"a synthetic comment")?;
            }
            Ok(())
        })?;
    fs::write(done, b"")?;
    Ok(())
}

/// Every file of the tree, in a stable order.
pub fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => dirs.push(e.path()),
                Ok(_) if !e.file_name().as_encoded_bytes().starts_with(b".") => {
                    out.push(e.path());
                }
                _ => {}
            }
        }
    }
    out.sort();
    out
}

fn flat(dir: &Path, n: u64) -> Result<()> {
    // Parallel creation is markedly faster on btrfs/ext4 for six-figure counts.
    (0..n).into_par_iter().try_for_each(|i| -> Result<()> {
        let name = format!("file-{i:06}.{}", EXTS[(i % EXTS.len() as u64) as usize]);
        fs::File::create(dir.join(name)).with_context(|| format!("creating file {i}"))?;
        Ok(())
    })
}

fn deep(dir: &Path) -> Result<()> {
    // 10^4 leaf directories x 100 files = 1,000,000 files.
    let leaves: Vec<PathBuf> = (0..10_000u32)
        .map(|i| {
            let (a, b, c, d) = (i / 1000, (i / 100) % 10, (i / 10) % 10, i % 10);
            dir.join(format!("d{a}"))
                .join(format!("d{b}"))
                .join(format!("d{c}"))
                .join(format!("d{d}"))
        })
        .collect();
    leaves.par_iter().try_for_each(|leaf| -> Result<()> {
        fs::create_dir_all(leaf)?;
        for j in 0..100u32 {
            fs::File::create(leaf.join(format!("f{j:03}.{}", EXTS[(j as usize) % EXTS.len()])))?;
        }
        Ok(())
    })
}

fn images(dir: &Path, n: u64) -> Result<()> {
    const SIZE: u32 = 96;
    (0..n).into_par_iter().try_for_each(|i| -> Result<()> {
        // A cheap gradient with per-image hue so thumbnails are visibly distinct.
        let hue = (i * 37 % 360) as f32;
        let img = image::RgbImage::from_fn(SIZE, SIZE, |x, y| {
            let t = (x + y) as f32 / (2.0 * SIZE as f32);
            let (r, g, b) = hsv(hue, 0.7, 0.4 + 0.6 * t);
            image::Rgb([r, g, b])
        });
        let mut buf = Vec::with_capacity(4096);
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)?;
        fs::File::create(dir.join(format!("img-{i:05}.png")))?.write_all(&buf)?;
        Ok(())
    })
}

/// `n` drawings for the thumbnail measurement, written into `dir`, not a
/// profile's tree. Drawn as an icon theme's full-color icons are: a
/// gradient, filled and stroked curves, and on every tenth a blurred
/// shadow, what costs most. No text: its cost is the machine's fonts'.
pub fn drawings(dir: &Path, n: u32) -> Result<()> {
    use std::fmt::Write as _;
    const SIZE: u32 = 96;
    let color = |(r, g, b): (u8, u8, u8)| format!("#{r:02x}{g:02x}{b:02x}");
    for i in 0..n {
        let hue = (i * 37 % 360) as f32;
        let (light, dark) = (color(hsv(hue, 0.7, 0.9)), color(hsv(hue, 0.7, 0.45)));
        let mut svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{SIZE}" height="{SIZE}"><defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{light}"/><stop offset="1" stop-color="{dark}"/></linearGradient><filter id="s" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="3"/></filter></defs>"#
        );
        if i % 10 == 0 {
            svg.push_str(r##"<rect x="10" y="16" width="80" height="72" rx="8" fill="#000" opacity="0.3" filter="url(#s)"/>"##);
        }
        svg.push_str(r##"<rect x="8" y="12" width="80" height="72" rx="8" fill="url(#g)" stroke="#000" stroke-opacity="0.2"/><circle cx="48" cy="44" r="18" fill="#fff" fill-opacity="0.85"/>"##);
        for k in 0..24 {
            let at = |step: u32| 12 + (k * step + i) % 72;
            write!(
                svg,
                r##"<path d="M{} {} C{} {} {} {} {} {}" fill="none" stroke="#fff" stroke-opacity="0.6" stroke-width="{}" stroke-linecap="round"/>"##,
                at(7),
                at(11),
                at(13),
                at(5),
                at(17),
                at(19),
                at(23),
                at(3),
                1 + k % 3
            )?;
        }
        svg.push_str("</svg>");
        fs::write(dir.join(format!("drawing-{i:03}.svg")), svg)?;
    }
    Ok(())
}

fn hsv(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}
