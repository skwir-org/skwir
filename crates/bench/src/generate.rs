//! Deterministic, idempotent tree generation.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::profiles::Profile;

const EXTS: [&str; 8] = ["txt", "rs", "png", "pdf", "md", "jpg", "toml", "log"];

fn marker(dir: &Path) -> PathBuf {
    dir.join(".bibli-bench-complete")
}

/// Generate `profile` under `root` unless a completion marker already exists.
pub fn generate(root: &Path, profile: Profile) -> Result<PathBuf> {
    let dir = root.join(profile.name());
    if marker(&dir).exists() {
        return Ok(dir);
    }
    eprintln!(
        "bench: generating {} ({} files)...",
        profile.name(),
        profile.file_count()
    );
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    match profile {
        Profile::Flat100k => flat(&dir, 100_000)?,
        Profile::Deep1m => deep(&dir)?,
        Profile::Images10k => images(&dir, 10_000)?,
    }
    fs::write(marker(&dir), b"")?;
    Ok(dir)
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
