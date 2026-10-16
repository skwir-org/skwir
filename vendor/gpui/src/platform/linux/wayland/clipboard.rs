use std::{
    fs::File,
    io::{ErrorKind, Write},
    os::fd::{AsRawFd, BorrowedFd, OwnedFd},
};

use calloop::{LoopHandle, PostAction};
use filedescriptor::Pipe;
use strum::IntoEnumIterator;
use wayland_client::{Connection, protocol::wl_data_offer::WlDataOffer};
use wayland_protocols::wp::primary_selection::zv1::client::zwp_primary_selection_offer_v1::ZwpPrimarySelectionOfferV1;

use crate::{
    ClipboardEntry, ClipboardFiles, ClipboardItem, Image, ImageFormat, WaylandClientStatePtr, hash,
    platform::linux::{clipboard_files, platform::read_fd},
};

/// Text mime types that we'll offer to other programs.
pub(crate) const TEXT_MIME_TYPES: [&str; 3] =
    ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];
pub(crate) const FILE_LIST_MIME_TYPE: &str = "text/uri-list";

/// Text mime types that we'll accept from other programs.
pub(crate) const ALLOWED_TEXT_MIME_TYPES: [&str; 2] = ["text/plain;charset=utf-8", "UTF8_STRING"];

pub(crate) struct Clipboard {
    connection: Connection,
    loop_handle: LoopHandle<'static, WaylandClientStatePtr>,
    self_mime: String,

    // Internal clipboard
    contents: Option<ClipboardItem>,
    primary_contents: Option<ClipboardItem>,
    /// Files this application put on the clipboard, beside `contents`'s
    /// text, until another application's offer takes their place.
    files: Option<ClipboardFiles>,

    // External clipboard
    cached_read: Option<ClipboardItem>,
    current_offer: Option<DataOffer<WlDataOffer>>,
    cached_primary_read: Option<ClipboardItem>,
    current_primary_offer: Option<DataOffer<ZwpPrimarySelectionOfferV1>>,
}

pub(crate) trait ReceiveData {
    fn receive_data(&self, mime_type: String, fd: BorrowedFd<'_>);
}

impl ReceiveData for WlDataOffer {
    fn receive_data(&self, mime_type: String, fd: BorrowedFd<'_>) {
        self.receive(mime_type, fd);
    }
}

impl ReceiveData for ZwpPrimarySelectionOfferV1 {
    fn receive_data(&self, mime_type: String, fd: BorrowedFd<'_>) {
        self.receive(mime_type, fd);
    }
}

#[derive(Clone, Debug)]
/// Wrapper for `WlDataOffer` and `ZwpPrimarySelectionOfferV1`, used to help track mime types.
pub(crate) struct DataOffer<T: ReceiveData> {
    pub inner: T,
    mime_types: Vec<String>,
}

impl<T: ReceiveData> DataOffer<T> {
    pub fn new(offer: T) -> Self {
        Self {
            inner: offer,
            mime_types: Vec::new(),
        }
    }

    pub fn add_mime_type(&mut self, mime_type: String) {
        self.mime_types.push(mime_type)
    }

    fn has_mime_type(&self, mime_type: &str) -> bool {
        self.mime_types.iter().any(|t| t == mime_type)
    }

    /// Ask for `mime_type` through a pipe, read off the main thread once the
    /// connection is flushed.
    fn receive_pipe(&self, mime_type: &str) -> Option<filedescriptor::FileDescriptor> {
        let pipe = Pipe::new().ok()?;
        self.inner.receive_data(mime_type.to_string(), unsafe {
            BorrowedFd::borrow_raw(pipe.write.as_raw_fd())
        });
        Some(pipe.read)
    }

    fn read_bytes(&self, connection: &Connection, mime_type: &str) -> Option<Vec<u8>> {
        let pipe = Pipe::new().unwrap();
        self.inner.receive_data(mime_type.to_string(), unsafe {
            BorrowedFd::borrow_raw(pipe.write.as_raw_fd())
        });
        let fd = pipe.read;
        drop(pipe.write);

        connection.flush().unwrap();

        match unsafe { read_fd(fd) } {
            Ok(bytes) => Some(bytes),
            Err(err) => {
                log::error!("error reading clipboard pipe: {err:?}");
                None
            }
        }
    }

    fn read_text(&self, connection: &Connection) -> Option<ClipboardItem> {
        let mime_type = self.mime_types.iter().find(|&mime_type| {
            ALLOWED_TEXT_MIME_TYPES
                .iter()
                .any(|&allowed| allowed == mime_type)
        })?;
        let bytes = self.read_bytes(connection, mime_type)?;
        let text_content = match String::from_utf8(bytes) {
            Ok(content) => content,
            Err(e) => {
                log::error!("Failed to convert clipboard content to UTF-8: {}", e);
                return None;
            }
        };

        // Normalize the text to unix line endings, otherwise
        // copying from eg: firefox inserts a lot of blank
        // lines, and that is super annoying.
        let result = text_content.replace("\r\n", "\n");
        Some(ClipboardItem::new_string(result))
    }

    fn read_image(&self, connection: &Connection) -> Option<ClipboardItem> {
        for format in ImageFormat::iter() {
            let mime_type = format.mime_type();
            if !self.has_mime_type(mime_type) {
                continue;
            }

            if let Some(bytes) = self.read_bytes(connection, mime_type) {
                let id = hash(&bytes);
                return Some(ClipboardItem {
                    entries: vec![ClipboardEntry::Image(Image { format, bytes, id })],
                });
            }
        }
        None
    }
}

/// How the files on the clipboard are read.
pub(crate) enum FilesRequest {
    /// This application's own.
    Own(ClipboardFiles),
    /// Another application's lists, through pipes.
    Pipes {
        copied: Option<filedescriptor::FileDescriptor>,
        uri_list: Option<filedescriptor::FileDescriptor>,
        kde_cut: Option<filedescriptor::FileDescriptor>,
    },
    /// No files.
    Nothing,
}

impl FilesRequest {
    /// Read off the main thread: another application's pipes until they
    /// close or `clipboard_files::READ_DEADLINE`.
    pub fn read(self) -> Option<ClipboardFiles> {
        match self {
            FilesRequest::Own(files) => Some(files),
            FilesRequest::Nothing => None,
            FilesRequest::Pipes {
                copied,
                uri_list,
                kde_cut,
            } => {
                let deadline = std::time::Instant::now() + clipboard_files::READ_DEADLINE;
                let read = |fd: Option<filedescriptor::FileDescriptor>| {
                    fd.and_then(|fd| clipboard_files::read_until(fd, deadline))
                };
                let copied = read(copied);
                let uri_list = read(uri_list);
                let kde_cut = read(kde_cut);
                clipboard_files::files_from(
                    copied.as_deref(),
                    uri_list.as_deref(),
                    kde_cut.as_deref(),
                )
            }
        }
    }
}

impl Clipboard {
    pub fn new(
        connection: Connection,
        loop_handle: LoopHandle<'static, WaylandClientStatePtr>,
    ) -> Self {
        Self {
            connection,
            loop_handle,
            self_mime: format!("pid/{}", std::process::id()),

            contents: None,
            primary_contents: None,
            files: None,

            cached_read: None,
            current_offer: None,
            cached_primary_read: None,
            current_primary_offer: None,
        }
    }

    /// This application's text on the clipboard; true when it took the
    /// place of files this application had put there.
    pub fn set(&mut self, item: ClipboardItem) -> bool {
        self.contents = Some(item);
        self.files.take().is_some()
    }

    /// This application's files on the clipboard, with a text.
    pub fn set_files(&mut self, files: ClipboardFiles, text: String) {
        self.contents = Some(ClipboardItem::new_string(text));
        self.files = Some(files);
    }

    /// The files this application put on the clipboard, while they are there.
    pub fn files(&self) -> Option<&ClipboardFiles> {
        self.files.as_ref()
    }

    /// Nothing of this application's is on the clipboard any more: it emptied it.
    pub fn clear(&mut self) {
        self.contents = None;
        self.files = None;
    }

    /// What reading the files on the clipboard takes: this application's
    /// own, or pipes from the application whose offer it is.
    pub fn request_files(&self) -> FilesRequest {
        if let Some(files) = &self.files {
            return FilesRequest::Own(files.clone());
        }
        let Some(offer) = self.current_offer.as_ref() else {
            return FilesRequest::Nothing;
        };
        let pipe = |mime_type: &str| {
            offer
                .has_mime_type(mime_type)
                .then(|| offer.receive_pipe(mime_type))
                .flatten()
        };
        let copied = pipe(clipboard_files::GNOME_COPIED_FILES)
            .or_else(|| pipe(clipboard_files::MATE_COPIED_FILES));
        let uri_list = pipe(clipboard_files::URI_LIST);
        let kde_cut = pipe(clipboard_files::KDE_CUT_SELECTION);
        if copied.is_none() && uri_list.is_none() {
            return FilesRequest::Nothing;
        }
        self.connection.flush().ok();
        FilesRequest::Pipes {
            copied,
            uri_list,
            kde_cut,
        }
    }

    pub fn set_primary(&mut self, item: ClipboardItem) {
        self.primary_contents = Some(item);
    }

    /// The clipboard's new offer. True when it is another application's
    /// and takes the place of files this one had put there (an offer that
    /// carries this application's own type is still its own, re-offered by
    /// a clipboard manager).
    pub fn set_offer(&mut self, data_offer: Option<DataOffer<WlDataOffer>>) -> bool {
        self.cached_read = None;
        let own = data_offer
            .as_ref()
            .is_some_and(|offer| offer.has_mime_type(&self.self_mime));
        self.current_offer = data_offer;
        !own && self.files.take().is_some()
    }

    pub fn set_primary_offer(&mut self, data_offer: Option<DataOffer<ZwpPrimarySelectionOfferV1>>) {
        self.cached_primary_read = None;
        self.current_primary_offer = data_offer;
    }

    pub fn self_mime(&self) -> String {
        self.self_mime.clone()
    }

    pub fn send(&self, mime_type: String, fd: OwnedFd) {
        if let Some(bytes) = self
            .files
            .as_ref()
            .and_then(|files| clipboard_files::bytes_for(files, &mime_type))
        {
            self.send_internal(fd, bytes);
        } else if let Some(text) = self.contents.as_ref().and_then(|contents| contents.text()) {
            self.send_internal(fd, text.as_bytes().to_owned());
        }
    }

    /// Write arbitrary bytes to a data-source `Send` fd (used by outgoing drags).
    pub fn send_bytes(&self, fd: OwnedFd, bytes: Vec<u8>) {
        self.send_internal(fd, bytes);
    }

    pub fn send_primary(&self, _mime_type: String, fd: OwnedFd) {
        if let Some(text) = self
            .primary_contents
            .as_ref()
            .and_then(|contents| contents.text())
        {
            self.send_internal(fd, text.as_bytes().to_owned());
        }
    }

    pub fn read(&mut self) -> Option<ClipboardItem> {
        let offer = self.current_offer.as_ref()?;
        if let Some(cached) = self.cached_read.clone() {
            return Some(cached);
        }

        if offer.has_mime_type(&self.self_mime) {
            return self.contents.clone();
        }

        let item = offer
            .read_text(&self.connection)
            .or_else(|| offer.read_image(&self.connection))?;

        self.cached_read = Some(item.clone());
        Some(item)
    }

    pub fn read_primary(&mut self) -> Option<ClipboardItem> {
        let offer = self.current_primary_offer.as_ref()?;
        if let Some(cached) = self.cached_primary_read.clone() {
            return Some(cached);
        }

        if offer.has_mime_type(&self.self_mime) {
            return self.primary_contents.clone();
        }

        let item = offer
            .read_text(&self.connection)
            .or_else(|| offer.read_image(&self.connection))?;

        self.cached_primary_read = Some(item.clone());
        Some(item)
    }

    fn send_internal(&self, fd: OwnedFd, bytes: Vec<u8>) {
        let mut written = 0;
        self.loop_handle
            .insert_source(
                calloop::generic::Generic::new(
                    File::from(fd),
                    calloop::Interest::WRITE,
                    calloop::Mode::Level,
                ),
                move |_, file, _| {
                    let mut file = unsafe { file.get_mut() };
                    loop {
                        match file.write(&bytes[written..]) {
                            Ok(n) if written + n == bytes.len() => {
                                written += n;
                                break Ok(PostAction::Remove);
                            }
                            Ok(n) => written += n,
                            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                                break Ok(PostAction::Continue);
                            }
                            Err(_) => break Ok(PostAction::Remove),
                        }
                    }
                },
            )
            .unwrap();
    }
}
