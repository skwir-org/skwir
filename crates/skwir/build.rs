// Which build of skwir this program is (`SKWIR_BUILD`).
include!("../../tools/build/identity.rs");

fn main() {
    emit_build();
    #[cfg(windows)]
    windows_icon::embed();
}

/// skwir.exe's icon, which Explorer, the Start menu and the taskbar show
/// and gpui gives its windows (resource 1): the UI's own drawing
/// (`crates/ui/assets/org.skwir.skwir.svg`) at each side Windows draws an
/// application's icon at. Embedded when built on Windows, whose resource
/// compiler it needs, as gpui embeds its manifest: a check from another
/// system (`just check-cross`) leaves it out.
#[cfg(windows)]
mod windows_icon {
    const SVG: &str = "../ui/assets/org.skwir.skwir.svg";
    const SIDES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

    pub fn embed() {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
            return;
        }
        println!("cargo::rerun-if-changed={SVG}");
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let ico = out.join("skwir.ico");
        std::fs::write(&ico, icon(&std::fs::read(SVG).unwrap())).unwrap();
        let rc = out.join("skwir.rc");
        // A string of the resource script, its backslashes doubled.
        let path = ico.display().to_string().replace('\\', "\\\\");
        std::fs::write(&rc, format!("1 ICON \"{path}\"\n")).unwrap();
        embed_resource::compile(&rc, embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }

    /// An .ico of `svg` at each of `SIDES`, each picture a PNG.
    fn icon(svg: &[u8]) -> Vec<u8> {
        let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).unwrap();
        let pictures: Vec<(u32, Vec<u8>)> = SIDES
            .iter()
            .map(|&side| {
                let mut pixmap = resvg::tiny_skia::Pixmap::new(side, side).unwrap();
                let scale = side as f32 / tree.size().width();
                resvg::render(
                    &tree,
                    resvg::tiny_skia::Transform::from_scale(scale, scale),
                    &mut pixmap.as_mut(),
                );
                (side, pixmap.encode_png().unwrap())
            })
            .collect();
        // The header, an entry for each picture, then the pictures.
        let mut ico = vec![0, 0, 1, 0];
        ico.extend_from_slice(&(pictures.len() as u16).to_le_bytes());
        let mut offset = 6 + 16 * pictures.len();
        for (side, png) in &pictures {
            // A side of 256 is written 0: it does not fit.
            let side = u8::try_from(*side).unwrap_or(0);
            ico.extend_from_slice(&[side, side, 0, 0]);
            ico.extend_from_slice(&1u16.to_le_bytes());
            ico.extend_from_slice(&32u16.to_le_bytes());
            ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
            ico.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += png.len();
        }
        for (_, png) in &pictures {
            ico.extend_from_slice(png);
        }
        ico
    }
}
