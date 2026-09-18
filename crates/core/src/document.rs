//! Toolkit-independent reader data and the bounded document-worker protocol.
//! Rectangles use normalized coordinates, with the origin at the displayed
//! page's top left; selection geometry is independent of zoom and resolution.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::CancelToken;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.width && y <= self.y + self.height
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextChar {
    pub text: String,
    pub bounds: Rect,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Destination {
    /// One-based page number.
    Page(u32),
    Uri(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Link {
    pub bounds: Rect,
    pub destination: Destination,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outline {
    pub title: String,
    pub depth: u32,
    pub page: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentInfo {
    pub pages: u32,
    pub outlines: Vec<Outline>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageContent {
    pub page: u32,
    pub width: f32,
    pub height: f32,
    pub chars: Vec<TextChar>,
    pub links: Vec<Link>,
}

#[derive(Clone, Debug)]
pub struct DocumentPage {
    pub width: u32,
    pub height: u32,
    /// A single owned buffer transfers from the manager to the UI worker.
    /// Avoid an Arc conversion and another full-page copy for channel swizzling.
    pub rgba: Vec<u8>,
    pub content: Arc<PageContent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    pub page: u32,
    pub bounds: Vec<Rect>,
}

/// A document is owned by an open preview. Methods block only on workers;
/// dropping the last handle must never wait for a renderer on the UI thread.
pub trait DocumentSession: Send + Sync {
    fn info(&self) -> &DocumentInfo;
    fn render(&self, page: u32, side: u32, cancel: &CancelToken) -> anyhow::Result<DocumentPage>;
    fn search(&self, query: &str, cancel: &CancelToken) -> anyhow::Result<Vec<SearchHit>>;
}

/// Internal protocol: JSON lines for small requests/metadata, a private RGBA
/// file for pixels. Neither filenames nor document text are written to logs.
#[derive(Debug, Serialize, Deserialize)]
pub enum WorkerRequest {
    Render { page: u32, side: u32 },
    Search { query: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum WorkerReply {
    Ready(DocumentInfo),
    Page {
        content: PageContent,
        width: u32,
        height: u32,
    },
    Matches(Vec<SearchHit>),
    Unavailable(String),
    Error(String),
}

pub const MAX_SIDE: u32 = 2048;
pub const MAX_REPLY: usize = 32 * 1024 * 1024;
pub const MAX_HITS: usize = 500;
pub const PIXELS_FILE: &str = "page.rgba";
