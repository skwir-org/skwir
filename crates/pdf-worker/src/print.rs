//! Printing on Windows (ADR-111): every page of the document drawn by
//! `PDFium` on the printer Windows' print dialog chose, with its settings,
//! each page fitted to the printable area. Compiled for Windows from Linux
//! and tried under Wine, which has no printer (ADR-074).
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
            let (width, height) = (GetDeviceCaps(dc, HORZRES), GetDeviceCaps(dc, VERTRES));
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
                let (page_width, page_height) = (
                    bindings.FPDF_GetPageWidthF(sheet),
                    bindings.FPDF_GetPageHeightF(sheet),
                );
                let scale = (width as f32 / page_width).min(height as f32 / page_height);
                let (w, h) = ((page_width * scale) as i32, (page_height * scale) as i32);
                bindings.FPDF_RenderPage(
                    dc,
                    sheet,
                    (width - w) / 2,
                    (height - h) / 2,
                    w,
                    h,
                    0,
                    FLAGS,
                );
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
