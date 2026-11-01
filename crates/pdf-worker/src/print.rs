//! Printing on Windows (ADR-111): every page of the document drawn by
//! `PDFium` on the printer Windows' print dialog chose, with its settings,
//! each page fitted to the printable area, turned to fit it (`placed`).
//! Compiled for Windows from Linux and tried under Wine, which has no
//! printer (ADR-074).
use pdfium_render::prelude::PdfiumLibraryBindings;
use std::path::Path;

/// Print the PDF at `pdf` on `printer` with `devmode` (a `DEVMODEW` as the
/// dialog left it), the job named `title`: how many pages were sent.
pub(crate) fn print(
    bindings: &dyn PdfiumLibraryBindings,
    pdf: &Path,
    printer: &str,
    devmode: &[u8],
    title: &str,
) -> anyhow::Result<u32> {
    #[cfg(windows)]
    {
        windows::print(bindings, pdf, printer, devmode, title)
    }
    #[cfg(not(windows))]
    {
        let _ = (bindings, pdf, printer, devmode, title);
        anyhow::bail!("printing on a printer from here is Windows' own")
    }
}

/// Where a page of `page` points (width, height) is drawn in a printable
/// area of `area` pixels: as large as it fits, centred, turned a quarter
/// turn when it is wide and the area tall or the other way round (a
/// booklet's landscape sheet on paper fed upright), as the system's
/// printing turns a page to fit its paper elsewhere. `FPDF_RenderPage`'s
/// left, top, width, height and turn (1: a quarter turn clockwise).
#[cfg(any(windows, test))]
fn placed(page: (f32, f32), area: (i32, i32)) -> (i32, i32, i32, i32, i32) {
    let (page_width, page_height) = (page.0.max(0.01), page.1.max(0.01));
    let turn = (page_width > page_height) != (area.0 > area.1);
    // The page as it lands, turned or not.
    let (width, height) = if turn {
        (page_height, page_width)
    } else {
        (page_width, page_height)
    };
    let scale = (area.0 as f32 / width).min(area.1 as f32 / height);
    let (w, h) = ((width * scale) as i32, (height * scale) as i32);
    ((area.0 - w) / 2, (area.1 - h) / 2, w, h, i32::from(turn))
}

#[cfg(windows)]
mod windows {
    use ::windows::Win32::Graphics::Gdi::{
        CreateDCW, DEVMODEW, DeleteDC, GetDeviceCaps, HORZRES, VERTRES,
    };
    use ::windows::Win32::Storage::Xps::{
        AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage,
    };
    use ::windows::core::PCWSTR;
    use pdfium_render::prelude::PdfiumLibraryBindings;
    use std::path::Path;

    /// `FPDF_ANNOT | FPDF_PRINTING`: annotations drawn, as for a printer.
    const FLAGS: i32 = 0x01 | 0x800;

    #[allow(unsafe_code)]
    pub(super) fn print(
        bindings: &dyn PdfiumLibraryBindings,
        pdf: &Path,
        printer: &str,
        devmode: &[u8],
        title: &str,
    ) -> anyhow::Result<u32> {
        anyhow::ensure!(
            devmode.len() >= std::mem::size_of::<DEVMODEW>(),
            "the printer's settings are not whole"
        );
        let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (printer, title) = (wide(printer), wide(title));
        // Held as words, so the settings are aligned as DEVMODEW is.
        let mut mode = vec![0u64; devmode.len().div_ceil(8)];
        // SAFETY: `mode` holds at least `devmode.len()` bytes; the copy
        // writes no more.
        unsafe {
            std::ptr::copy_nonoverlapping(
                devmode.as_ptr(),
                mode.as_mut_ptr().cast::<u8>(),
                devmode.len(),
            );
        }
        // SAFETY: the strings are NUL-terminated and outlive the calls;
        // `mode` is a whole DEVMODEW (its size checked) that outlives the
        // device context; every handle made here is closed here, the
        // document and pages through PDFium's own calls on its handles.
        unsafe {
            let dc = CreateDCW(
                PCWSTR::null(),
                PCWSTR(printer.as_ptr()),
                PCWSTR::null(),
                Some(mode.as_ptr().cast::<DEVMODEW>()),
            );
            anyhow::ensure!(!dc.is_invalid(), "the printer cannot be opened");
            let document = DOCINFOW {
                cbSize: i32::try_from(std::mem::size_of::<DOCINFOW>()).unwrap_or(i32::MAX),
                lpszDocName: PCWSTR(title.as_ptr()),
                ..Default::default()
            };
            if StartDocW(dc, std::ptr::from_ref(&document)) <= 0 {
                let _ = DeleteDC(dc);
                anyhow::bail!("the printer refused the document");
            }
            let (width, height) = (
                GetDeviceCaps(Some(dc), HORZRES),
                GetDeviceCaps(Some(dc), VERTRES),
            );
            let handle = bindings.FPDF_LoadDocument(&pdf.to_string_lossy(), None);
            if handle.is_null() {
                AbortDoc(dc);
                let _ = DeleteDC(dc);
                anyhow::bail!("the document cannot be opened to print");
            }
            let mut sent = 0;
            for index in 0..bindings.FPDF_GetPageCount(handle) {
                let sheet = bindings.FPDF_LoadPage(handle, index);
                if sheet.is_null() {
                    continue;
                }
                StartPage(dc);
                let (x, y, w, h, turn) = super::placed(
                    (
                        bindings.FPDF_GetPageWidthF(sheet),
                        bindings.FPDF_GetPageHeightF(sheet),
                    ),
                    (width, height),
                );
                bindings.FPDF_RenderPage(dc, sheet, x, y, w, h, turn, FLAGS);
                EndPage(dc);
                bindings.FPDF_ClosePage(sheet);
                sent += 1;
            }
            bindings.FPDF_CloseDocument(handle);
            EndDoc(dc);
            let _ = DeleteDC(dc);
            Ok(sent)
        }
    }
}

#[cfg(test)]
mod tests {
    /// A page fits the area as large as it can, centred; a wide page on an
    /// upright area (a booklet's sheet on A4 fed upright) is turned a
    /// quarter turn, so it fills the paper rather than its top half.
    #[test]
    fn a_page_is_turned_to_fit_paper_of_the_other_orientation() {
        // A4 at 600 dpi, upright: 4960 × 7016 pixels.
        let area = (4960, 7016);
        let (x, y, w, h, turn) = super::placed((595.0, 842.0), area);
        assert_eq!(turn, 0);
        assert!((w - 4960).abs() <= 5 && (h - 7016).abs() <= 5, "{w} × {h}");
        assert!(x.abs() <= 3 && y.abs() <= 3, "{x}, {y}");
        // Landscape A4: turned, it fills the paper too.
        let (x, y, w, h, turn) = super::placed((842.0, 595.0), area);
        assert_eq!(turn, 1);
        assert!((w - 4960).abs() <= 5 && (h - 7016).abs() <= 5, "{w} × {h}");
        assert!(x.abs() <= 3 && y.abs() <= 3, "{x}, {y}");
        // A wide page on wide paper is not turned; a square never is.
        assert_eq!(super::placed((842.0, 595.0), (7016, 4960)).4, 0);
        assert_eq!(super::placed((500.0, 500.0), area).4, 0);
        // Smaller than the paper's shape, centred along its free side.
        let (x, y, w, h, _) = super::placed((400.0, 400.0), area);
        assert_eq!((x, w), (0, 4960));
        assert_eq!(y, (7016 - h) / 2);
    }
}
