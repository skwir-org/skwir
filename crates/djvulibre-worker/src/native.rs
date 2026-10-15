//! `DjVuLibre`'s decoding API (`ddjvuapi.h`), what the worker asks of it.
//! Its answers leave here as Rust values: a page's pixels in a buffer we
//! own, s-expressions (text, outline, links) copied into `Sexp` and given
//! back at once.
// SAFETY: every native handle stays on the worker's one thread and is
// released once, by its owner's `Drop` (the context last). Strings and
// lists are copied while their document holds them; a buffer handed to the
// renderer has the exact size of the rows it is told about.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_ulong, c_void};

type Exp = *mut c_void;

/// `ddjvu_status_t`: a job not done yet is below `OK`, one that ended
/// badly above.
const JOB_OK: c_int = 2;
/// `DDJVU_ERROR`, the tag of an error message.
const MESSAGE_ERROR: c_int = 0;
/// `DDJVU_FORMAT_RGBMASK32` and `DDJVU_RENDER_COLOR`.
const FORMAT_RGBMASK32: c_int = 3;
const RENDER_COLOR: c_int = 0;
/// `miniexp_dummy`: what is asked for is not there yet.
const DUMMY: usize = 2;

#[repr(C)]
struct MessageAny {
    tag: c_int,
    context: *mut c_void,
    document: *mut c_void,
    page: *mut c_void,
    job: *mut c_void,
}

#[repr(C)]
struct MessageError {
    any: MessageAny,
    message: *const c_char,
    function: *const c_char,
    filename: *const c_char,
    lineno: c_int,
}

#[repr(C)]
#[derive(Default)]
struct RenderRect {
    x: c_int,
    y: c_int,
    w: c_uint,
    h: c_uint,
}

#[repr(C)]
struct FileInfo {
    /// A C `char`, read as the byte it is: `P` for a page.
    kind: u8,
    pageno: c_int,
    size: c_int,
    id: *const c_char,
    name: *const c_char,
    title: *const c_char,
}

#[link(name = "djvulibre")]
unsafe extern "C" {
    fn ddjvu_context_create(programname: *const c_char) -> *mut c_void;
    fn ddjvu_context_release(context: *mut c_void);
    fn ddjvu_message_wait(context: *mut c_void) -> *const MessageAny;
    fn ddjvu_message_peek(context: *mut c_void) -> *const MessageAny;
    fn ddjvu_message_pop(context: *mut c_void);
    fn ddjvu_job_status(job: *mut c_void) -> c_int;
    fn ddjvu_job_release(job: *mut c_void);
    fn ddjvu_document_create_by_filename_utf8(
        context: *mut c_void,
        filename: *const c_char,
        cache: c_int,
    ) -> *mut c_void;
    fn ddjvu_document_job(document: *mut c_void) -> *mut c_void;
    fn ddjvu_document_get_pagenum(document: *mut c_void) -> c_int;
    fn ddjvu_document_get_filenum(document: *mut c_void) -> c_int;
    fn ddjvu_document_get_fileinfo_imp(
        document: *mut c_void,
        fileno: c_int,
        info: *mut FileInfo,
        infosz: c_uint,
    ) -> c_int;
    fn ddjvu_page_create_by_pageno(document: *mut c_void, pageno: c_int) -> *mut c_void;
    fn ddjvu_page_job(page: *mut c_void) -> *mut c_void;
    fn ddjvu_page_get_width(page: *mut c_void) -> c_int;
    fn ddjvu_page_get_height(page: *mut c_void) -> c_int;
    fn ddjvu_page_get_resolution(page: *mut c_void) -> c_int;
    fn ddjvu_page_get_rotation(page: *mut c_void) -> c_int;
    fn ddjvu_page_render(
        page: *mut c_void,
        mode: c_int,
        pagerect: *const RenderRect,
        renderrect: *const RenderRect,
        pixelformat: *const c_void,
        rowsize: c_ulong,
        imagebuffer: *mut c_char,
    ) -> c_int;
    fn ddjvu_format_create(style: c_int, nargs: c_int, args: *mut c_uint) -> *mut c_void;
    fn ddjvu_format_set_row_order(format: *mut c_void, top_to_bottom: c_int);
    fn ddjvu_format_set_y_direction(format: *mut c_void, top_to_bottom: c_int);
    fn ddjvu_format_release(format: *mut c_void);
    fn ddjvu_miniexp_release(document: *mut c_void, expr: Exp);
    fn ddjvu_document_get_outline(document: *mut c_void) -> Exp;
    fn ddjvu_document_get_pagetext(
        document: *mut c_void,
        pageno: c_int,
        maxdetail: *const c_char,
    ) -> Exp;
    fn ddjvu_document_get_pageanno(document: *mut c_void, pageno: c_int) -> Exp;
    fn ddjvu_anno_get_hyperlinks(annotations: Exp) -> *mut Exp;
    fn miniexp_stringp(p: Exp) -> c_int;
    fn miniexp_to_lstr(p: Exp, sp: *mut *const c_char) -> usize;
    fn miniexp_to_name(p: Exp) -> *const c_char;
}

// From the C library std already links: what DjVuLibre's arrays are given
// back with.
unsafe extern "C" {
    fn free(pointer: *mut c_void);
}

/// An s-expression as `DjVuLibre` writes text, outlines and links, copied.
#[derive(Clone, Debug, PartialEq)]
pub enum Sexp {
    List(Vec<Sexp>),
    Text(String),
    Symbol(String),
    Number(i32),
    /// Anything else (a float, an object of another class).
    Other,
}

/// The deepest an s-expression is read, and the most nodes: what a page's
/// text or a document's outline holds, and a crafted file cannot exceed.
const DEPTH: usize = 64;
const NODES: usize = 4_000_000;

/// `expr` copied, within `DEPTH` and `NODES`. The pair, number and symbol
/// tests are `miniexp.h`'s own (its low two bits), and a pair is two
/// expressions, its car and its cdr, as its inline functions read them.
///
/// # Safety
/// `expr` is held by a live document.
unsafe fn copy(expr: Exp, depth: usize, nodes: &mut usize) -> Sexp {
    *nodes += 1;
    if *nodes > NODES || depth > DEPTH {
        return Sexp::Other;
    }
    let bits = expr as usize;
    match bits & 3 {
        // `miniexp_to_int`: the low 32 bits as an int, shifted.
        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        3 => Sexp::Number((bits as i32) >> 2),
        2 => {
            // SAFETY: a symbol's name lives as long as the library.
            let name = unsafe { miniexp_to_name(expr) };
            if name.is_null() {
                Sexp::Other
            } else {
                Sexp::Symbol(
                    unsafe { CStr::from_ptr(name) }
                        .to_string_lossy()
                        .into_owned(),
                )
            }
        }
        0 => {
            let mut items = Vec::new();
            let mut cell = expr;
            // SAFETY: a nonzero expression whose low bits are 0 is a pair:
            // two expressions, car then cdr.
            while !cell.is_null() && (cell as usize).trailing_zeros() >= 2 {
                let pair = cell as *const Exp;
                items.push(unsafe { copy(*pair, depth + 1, nodes) });
                cell = unsafe { *pair.add(1) };
                if *nodes > NODES {
                    break;
                }
            }
            Sexp::List(items)
        }
        _ => {
            // SAFETY: an object; a string's bytes and length are its own.
            if unsafe { miniexp_stringp(expr) } != 0 {
                let mut start: *const c_char = std::ptr::null();
                let length = unsafe { miniexp_to_lstr(expr, &raw mut start) };
                if start.is_null() {
                    return Sexp::Text(String::new());
                }
                let bytes = unsafe { std::slice::from_raw_parts(start.cast::<u8>(), length) };
                Sexp::Text(String::from_utf8_lossy(bytes).into_owned())
            } else {
                Sexp::Other
            }
        }
    }
}

/// `DjVuLibre`'s context: its messages, and the errors they said.
pub struct Context {
    raw: *mut c_void,
    errors: RefCell<Vec<String>>,
}

impl Context {
    pub fn new() -> anyhow::Result<Self> {
        let name = CString::new("skwir")?;
        // SAFETY: the name is copied by the library.
        let raw = unsafe { ddjvu_context_create(name.as_ptr()) };
        anyhow::ensure!(!raw.is_null(), "DjVuLibre could not start");
        Ok(Self {
            raw,
            errors: RefCell::default(),
        })
    }

    /// Take the messages waiting, after waiting for one if `wait`; keep
    /// what the errors said.
    fn messages(&self, wait: bool) {
        // SAFETY: a message read is valid until it is popped; its strings
        // are copied before.
        unsafe {
            if wait {
                ddjvu_message_wait(self.raw);
            }
            loop {
                let message = ddjvu_message_peek(self.raw);
                if message.is_null() {
                    break;
                }
                if (*message).tag == MESSAGE_ERROR {
                    let error = message.cast::<MessageError>();
                    if !(*error).message.is_null() {
                        let said = CStr::from_ptr((*error).message).to_string_lossy();
                        let mut errors = self.errors.borrow_mut();
                        if errors.len() < 16 {
                            errors.push(said.into_owned());
                        }
                    }
                }
                ddjvu_message_pop(self.raw);
            }
        }
    }

    /// Wait for `job` to end: an error says what the library said.
    fn finish(&self, job: *mut c_void, what: &str) -> anyhow::Result<()> {
        loop {
            // SAFETY: the job is held by its owner while it is waited for.
            let status = unsafe { ddjvu_job_status(job) };
            if status == JOB_OK {
                return Ok(());
            }
            if status > JOB_OK {
                let said = self.errors.borrow_mut().drain(..).next_back();
                anyhow::bail!(
                    "{what}{}",
                    said.map(|s| format!(": {s}")).unwrap_or_default()
                );
            }
            self.messages(true);
        }
    }

    /// Ask for an s-expression until the document has it, copy it and give
    /// it back. `None` for none, or one the library could not read.
    fn expression(&self, document: *mut c_void, ask: impl Fn() -> Exp) -> Option<Sexp> {
        let expr = loop {
            let expr = ask();
            if expr as usize != DUMMY {
                break expr;
            }
            self.messages(true);
        };
        if expr.is_null() {
            return None;
        }
        let mut nodes = 0;
        // SAFETY: the document holds `expr` until it is given back here.
        let copied = unsafe { copy(expr, 0, &mut nodes) };
        unsafe { ddjvu_miniexp_release(document, expr) };
        // `failed` or `stopped`: it could not be read.
        (!matches!(copied, Sexp::Symbol(_))).then_some(copied)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: every document of the context was released before it.
        unsafe { ddjvu_context_release(self.raw) };
    }
}

/// A page decoded: what it shows, and how.
struct Page {
    number: u32,
    raw: *mut c_void,
}

/// A document open, and the pages it keeps decoded.
pub struct Document<'a> {
    context: &'a Context,
    raw: *mut c_void,
    pages: RefCell<VecDeque<Page>>,
}

/// The pages a document keeps decoded: drawn again at another side (the
/// reader draws a page small, then sharp), one is not decoded again.
const KEPT_PAGES: usize = 3;

/// A page's sides as shown (turned), its resolution and how it turns.
pub struct Shown {
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
    /// Quarter turns counterclockwise from the page as stored.
    pub quarters: u8,
}

/// A component file of a document, as its directory says.
pub struct Component {
    pub page: Option<u32>,
    pub id: String,
    pub title: String,
}

impl<'a> Document<'a> {
    pub fn open(context: &'a Context, path: &std::path::Path) -> anyhow::Result<Self> {
        let path = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("the document's path is no UTF-8"))?;
        let name = CString::new(path)?;
        // SAFETY: the name is copied by the library.
        let raw = unsafe { ddjvu_document_create_by_filename_utf8(context.raw, name.as_ptr(), 1) };
        anyhow::ensure!(!raw.is_null(), "DjVuLibre could not open the document");
        let document = Self {
            context,
            raw,
            pages: RefCell::default(),
        };
        // SAFETY: the document is alive.
        context.finish(unsafe { ddjvu_document_job(raw) }, "not a DjVu document")?;
        Ok(document)
    }

    pub fn pages(&self) -> u32 {
        // SAFETY: the document is decoded.
        u32::try_from(unsafe { ddjvu_document_get_pagenum(self.raw) }).unwrap_or(0)
    }

    /// The document's component files, with the page each is.
    pub fn components(&self) -> Vec<Component> {
        // SAFETY: the document is decoded; its strings live as long as it
        // and are copied.
        let count = unsafe { ddjvu_document_get_filenum(self.raw) };
        let string = |pointer: *const c_char| {
            if pointer.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(pointer) }
                    .to_string_lossy()
                    .into_owned()
            }
        };
        (0..count)
            .filter_map(|file| {
                let mut info = FileInfo {
                    kind: 0,
                    pageno: -1,
                    size: -1,
                    id: std::ptr::null(),
                    name: std::ptr::null(),
                    title: std::ptr::null(),
                };
                loop {
                    let status = unsafe {
                        ddjvu_document_get_fileinfo_imp(
                            self.raw,
                            file,
                            &raw mut info,
                            c_uint::try_from(size_of::<FileInfo>()).ok()?,
                        )
                    };
                    if status == JOB_OK {
                        break;
                    }
                    if status > JOB_OK {
                        return None;
                    }
                    self.context.messages(true);
                }
                Some(Component {
                    page: (info.kind == b'P')
                        .then(|| u32::try_from(info.pageno).ok().map(|page| page + 1))
                        .flatten(),
                    id: string(info.id),
                    title: string(info.title),
                })
            })
            .collect()
    }

    /// Page `number` (from 1) decoded, kept or decoded now.
    fn page(&self, number: u32) -> anyhow::Result<*mut c_void> {
        if let Some(page) = self
            .pages
            .borrow()
            .iter()
            .find(|page| page.number == number)
        {
            return Ok(page.raw);
        }
        // SAFETY: the document is alive; the page is released by
        // `Document::drop` or when another takes its place.
        let raw = unsafe { ddjvu_page_create_by_pageno(self.raw, c_int::try_from(number - 1)?) };
        anyhow::ensure!(!raw.is_null(), "page {number} could not be read");
        let decoded = self
            .context
            .finish(unsafe { ddjvu_page_job(raw) }, &format!("page {number}"));
        if let Err(error) = decoded {
            unsafe { ddjvu_job_release(ddjvu_page_job(raw)) };
            return Err(error);
        }
        let mut pages = self.pages.borrow_mut();
        if pages.len() == KEPT_PAGES
            && let Some(old) = pages.pop_front()
        {
            unsafe { ddjvu_job_release(ddjvu_page_job(old.raw)) };
        }
        pages.push_back(Page { number, raw });
        Ok(raw)
    }

    /// How page `number` is shown.
    pub fn shown(&self, number: u32) -> anyhow::Result<Shown> {
        let page = self.page(number)?;
        // SAFETY: the page is decoded and kept.
        let (width, height, dpi, rotation) = unsafe {
            (
                ddjvu_page_get_width(page),
                ddjvu_page_get_height(page),
                ddjvu_page_get_resolution(page),
                ddjvu_page_get_rotation(page),
            )
        };
        Ok(Shown {
            width: u32::try_from(width)?.max(1),
            height: u32::try_from(height)?.max(1),
            dpi: u32::try_from(dpi).unwrap_or(0),
            quarters: u8::try_from(rotation.rem_euclid(4))?,
        })
    }

    /// Page `number` drawn whole at `width` × `height`, four bytes a pixel
    /// in the window's order (BGRA).
    pub fn render(&self, number: u32, width: u32, height: u32) -> anyhow::Result<Vec<u8>> {
        let page = self.page(number)?;
        let rect = RenderRect {
            w: width,
            h: height,
            ..RenderRect::default()
        };
        let row = width as usize * 4;
        let mut pixels = vec![0u8; row * height as usize];
        // Red, green and blue in a 32-bit word, opaque: on a little-endian
        // machine, the bytes BGRA.
        let mut masks: [c_uint; 4] = [0xff_0000, 0xff00, 0xff, 0xff00_0000];
        // SAFETY: the format is released below; the buffer holds `height`
        // rows of `row` bytes, what the library is told.
        let drawn = unsafe {
            let format = ddjvu_format_create(FORMAT_RGBMASK32, 4, masks.as_mut_ptr());
            anyhow::ensure!(!format.is_null(), "no pixel format");
            ddjvu_format_set_row_order(format, 1);
            ddjvu_format_set_y_direction(format, 1);
            let drawn = ddjvu_page_render(
                page,
                RENDER_COLOR,
                &raw const rect,
                &raw const rect,
                format,
                c_ulong::try_from(row)?,
                pixels.as_mut_ptr().cast(),
            );
            ddjvu_format_release(format);
            drawn
        };
        anyhow::ensure!(drawn != 0, "page {number} could not be drawn");
        if cfg!(target_endian = "big") {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                *pixel = u32::from_ne_bytes(*pixel).to_le_bytes();
            }
        }
        Ok(pixels)
    }

    /// Page `number`'s text, to its words.
    pub fn text(&self, number: u32) -> Option<Sexp> {
        let detail = c"word";
        let page = c_int::try_from(number.checked_sub(1)?).ok()?;
        // SAFETY: the document is alive.
        self.context.expression(self.raw, || unsafe {
            ddjvu_document_get_pagetext(self.raw, page, detail.as_ptr())
        })
    }

    pub fn outline(&self) -> Option<Sexp> {
        // SAFETY: the document is alive.
        self.context
            .expression(self.raw, || unsafe { ddjvu_document_get_outline(self.raw) })
    }

    /// Page `number`'s links: each `(maparea …)`.
    pub fn links(&self, number: u32) -> Vec<Sexp> {
        let Some(page) = number.checked_sub(1).and_then(|n| c_int::try_from(n).ok()) else {
            return Vec::new();
        };
        let annotations = loop {
            // SAFETY: the document is alive.
            let expr = unsafe { ddjvu_document_get_pageanno(self.raw, page) };
            if expr as usize != DUMMY {
                break expr;
            }
            self.context.messages(true);
        };
        if annotations.is_null() || (annotations as usize) & 3 != 0 {
            return Vec::new();
        }
        let mut links = Vec::new();
        // SAFETY: the array ends with a null and is freed here; its
        // expressions are the annotations', copied before they are given
        // back.
        unsafe {
            let areas = ddjvu_anno_get_hyperlinks(annotations);
            if !areas.is_null() {
                let mut at = areas;
                let mut nodes = 0;
                while !(*at).is_null() && links.len() < 10_000 {
                    links.push(copy(*at, 0, &mut nodes));
                    at = at.add(1);
                }
                free(areas.cast());
            }
            ddjvu_miniexp_release(self.raw, annotations);
        }
        links
    }
}

impl Drop for Document<'_> {
    fn drop(&mut self) {
        // SAFETY: pages first, then the document, each once.
        unsafe {
            for page in self.pages.borrow_mut().drain(..) {
                ddjvu_job_release(ddjvu_page_job(page.raw));
            }
            ddjvu_job_release(ddjvu_document_job(self.raw));
        }
    }
}
