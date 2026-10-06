//! The icons the system draws itself, where bibli reads no icon theme
//! (`icons` is the free desktops'): macOS's through `NSWorkspace`, by a
//! file's type or its path, and Windows' shell's, by extension or path,
//! from the system image list. What an icon is drawn of is a [`SystemIcon`]:
//! a file by its extension (Windows associates icons with extensions, and
//! macOS derives the type from it), a folder, one whose icon is its own (an
//! application, a shortcut, a user folder, a volume) by its path, or one of
//! the system's own (the trash, a drive). [`picture`] draws it at the size
//! it is shown. Compiled on every system; on Linux it answers nothing.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use bibli_core::thumbs::Image;

/// Whether this system draws icons of its own to ask for.
pub const AVAILABLE: bool = cfg!(any(target_os = "macos", windows));

/// What a system icon is drawn of.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SystemIcon {
    /// A file with this extension, lowercase and without its dot; empty
    /// for a file with none.
    Extension(String),
    /// A folder.
    Folder,
    /// The file, folder or volume at this path, whose icon is its own.
    Path(PathBuf),
    /// One of the system's own.
    Stock(Stock),
}

/// The system's own icons that have no file to ask by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stock {
    Trash,
    TrashFull,
    Drive,
    Removable,
    Optical,
    Network,
}

/// The system's icon for one the desktop's icon themes name, as places
/// name theirs (`user-home`, `user-trash-full`, `drive-removable-media`):
/// a user folder by its path, the trash and drives as the system draws
/// them. `None` for a name the system has nothing for.
pub fn for_name(name: &str) -> Option<SystemIcon> {
    if let Some(path_of) = user_folder(name) {
        return path_of().map(SystemIcon::Path);
    }
    let stock = |stock| Some(SystemIcon::Stock(stock));
    match name.strip_suffix("-symbolic").unwrap_or(name) {
        "user-trash" => stock(Stock::Trash),
        "user-trash-full" => stock(Stock::TrashFull),
        "drive-removable-media"
        | "drive-removable-media-usb"
        | "media-removable"
        | "drive-harddisk-usb"
        | "media-flash" => stock(Stock::Removable),
        "media-optical" | "drive-optical" => stock(Stock::Optical),
        "folder-remote" | "network-server" | "network-workgroup" => stock(Stock::Network),
        n if n.starts_with("drive-harddisk") => stock(Stock::Drive),
        n if n == "folder" || n.starts_with("folder-") => Some(SystemIcon::Folder),
        _ => None,
    }
}

/// Where the system keeps the user folder `name` names, if it names one.
#[cfg(any(target_os = "macos", windows))]
fn user_folder(name: &str) -> Option<fn() -> Option<PathBuf>> {
    Some(match name {
        "user-home" => dirs::home_dir,
        "user-desktop" => dirs::desktop_dir,
        "folder-documents" => dirs::document_dir,
        "folder-download" => dirs::download_dir,
        "folder-music" => dirs::audio_dir,
        "folder-pictures" => dirs::picture_dir,
        "folder-videos" => dirs::video_dir,
        "folder-publicshare" => dirs::public_dir,
        "folder-templates" => dirs::template_dir,
        _ => return None,
    })
}

#[cfg(not(any(target_os = "macos", windows)))]
fn user_folder(_name: &str) -> Option<fn() -> Option<PathBuf>> {
    None
}

/// The icon of the file at `path` named `name`: its own where the system
/// draws such files each their own (an application, a shortcut, an icon
/// file), else its extension's, or a folder's.
pub fn for_file(path: &Path, name: &OsStr, is_dir: bool) -> SystemIcon {
    let extension = extension_of(name);
    if its_own(&extension, is_dir) {
        return SystemIcon::Path(path.to_path_buf());
    }
    if is_dir {
        SystemIcon::Folder
    } else {
        SystemIcon::Extension(extension)
    }
}

/// The icon of a file known only by its name (an archive's member, a
/// track): its extension's.
pub fn by_extension(name: &OsStr) -> SystemIcon {
    SystemIcon::Extension(extension_of(name))
}

fn extension_of(name: &OsStr) -> String {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Whether the system draws a file with this extension its own icon.
fn its_own(extension: &str, is_dir: bool) -> bool {
    if cfg!(target_os = "macos") {
        // A bundle shown as one file: an application, a preference pane.
        is_dir && matches!(extension, "app" | "prefpane" | "appex")
    } else if cfg!(windows) {
        !is_dir
            && matches!(
                extension,
                "exe" | "lnk" | "ico" | "cur" | "ani" | "url" | "scr" | "appref-ms"
            )
    } else {
        false
    }
}

/// The system's picture of `icon`, its longer side at most `side` pixels,
/// as straight RGBA. `Ok(None)` where the system has none. Asks the system,
/// which may read the disk: call it from a worker.
pub fn picture(icon: &SystemIcon, side: u32) -> anyhow::Result<Option<Image>> {
    let side = side.clamp(1, 1024);
    Ok(imp::picture(icon, side)?.map(|picture| crate::thumb_providers::fit(picture, side)))
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    use super::SystemIcon;

    // The signature every system's shares.
    #[allow(clippy::unnecessary_wraps)]
    pub fn picture(_icon: &SystemIcon, _side: u32) -> anyhow::Result<Option<image::DynamicImage>> {
        Ok(None)
    }
}

/// `NSWorkspace`'s icons, drawn by `NSImage` at the size asked. `AppKit` is
/// asked from a worker: its icons and images are documented as safe off
/// the main thread. Compiled only (5b).
// The system's calls are Objective-C messages and extern statics: each
// `unsafe` block says why it is sound, and nothing unsafe leaves the module.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod imp {
    use objc2::rc::{Retained, autoreleasepool};
    use objc2_app_kit::{
        NSImage, NSImageNameNetwork, NSImageNameTrashEmpty, NSImageNameTrashFull, NSWorkspace,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use objc2_uniform_type_identifiers::{UTType, UTTypeData, UTTypeFolder, UTTypeVolume};

    use super::{Stock, SystemIcon};

    // The signature every system's shares: AppKit has no error to give.
    #[allow(clippy::unnecessary_wraps)]
    pub fn picture(icon: &SystemIcon, side: u32) -> anyhow::Result<Option<image::DynamicImage>> {
        Ok(autoreleasepool(|_| {
            let image = image_of(icon)?;
            let mut rect = NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(f64::from(side), f64::from(side)),
            );
            // SAFETY: a rect on the stack, no context and no hints: the
            // image picks its representation for that size.
            let picture =
                unsafe { image.CGImageForProposedRect_context_hints(&raw mut rect, None, None) }?;
            crate::system_thumbs::pixels(&picture)
        }))
    }

    fn image_of(icon: &SystemIcon) -> Option<Retained<NSImage>> {
        let workspace = NSWorkspace::sharedWorkspace();
        // SAFETY (each static): the frameworks' constants, set at load
        // and never written.
        let of_type = |kind: &UTType| Some(workspace.iconForContentType(kind));
        match icon {
            SystemIcon::Extension(extension) => {
                let kind = (!extension.is_empty())
                    .then(|| UTType::typeWithFilenameExtension(&NSString::from_str(extension)))
                    .flatten();
                match kind {
                    Some(kind) => of_type(&kind),
                    None => of_type(unsafe { UTTypeData }),
                }
            }
            SystemIcon::Folder => of_type(unsafe { UTTypeFolder }),
            SystemIcon::Path(path) => {
                Some(workspace.iconForFile(&NSString::from_str(path.to_str()?)))
            }
            SystemIcon::Stock(Stock::Trash) => {
                NSImage::imageNamed(unsafe { NSImageNameTrashEmpty })
            }
            SystemIcon::Stock(Stock::TrashFull) => {
                NSImage::imageNamed(unsafe { NSImageNameTrashFull })
            }
            SystemIcon::Stock(Stock::Network) => NSImage::imageNamed(unsafe { NSImageNameNetwork }),
            SystemIcon::Stock(Stock::Drive | Stock::Removable | Stock::Optical) => {
                of_type(unsafe { UTTypeVolume })
            }
        }
    }
}

/// The shell's icons, from the system image list: an extension's and a
/// folder's without touching the disk (`SHGFI_USEFILEATTRIBUTES`), a file's
/// own from the file, the stock ones by their id. Run under Wine, whose
/// shell has icons of its own; compiled for Windows (5b).
// The system's calls take raw pointers: each `unsafe` block says why its
// call is sound, and nothing unsafe leaves the module.
#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use ::windows::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ};
    use ::windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES,
    };
    use ::windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    use ::windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
    use ::windows::Win32::UI::Shell::{
        SHFILEINFOW, SHGFI_FLAGS, SHGFI_SYSICONINDEX, SHGFI_USEFILEATTRIBUTES, SHGSI_SYSICONINDEX,
        SHGetFileInfoW, SHGetImageList, SHGetStockIconInfo, SHIL_EXTRALARGE, SHIL_JUMBO,
        SHIL_LARGE, SHIL_SMALL, SHSTOCKICONID, SHSTOCKICONINFO, SIID_DRIVECD, SIID_DRIVEFIXED,
        SIID_DRIVENET, SIID_DRIVEREMOVE, SIID_RECYCLER, SIID_RECYCLERFULL,
    };
    use ::windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};
    use ::windows::core::HSTRING;

    use super::{Stock, SystemIcon};

    // The signature every system's shares: the shell has no error to give.
    #[allow(clippy::unnecessary_wraps)]
    pub fn picture(icon: &SystemIcon, side: u32) -> anyhow::Result<Option<image::DynamicImage>> {
        // SAFETY: COM for this thread, as the shell needs it; a thread
        // already in it gets S_FALSE, which is no failure.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let Some(index) = index_of(icon) else {
            return Ok(None);
        };
        let list = match side {
            0..=16 => SHIL_SMALL,
            17..=32 => SHIL_LARGE,
            33..=48 => SHIL_EXTRALARGE,
            _ => SHIL_JUMBO,
        };
        let picture = from_list(index, list);
        // The jumbo list draws an icon that has no large image small, in
        // its corner: take the extra large one, whole, instead.
        if list == SHIL_JUMBO && picture.as_ref().is_none_or(drawn_small) {
            return Ok(from_list(index, SHIL_EXTRALARGE).or(picture));
        }
        Ok(picture)
    }

    /// Where the icon is in the system image list.
    fn index_of(icon: &SystemIcon) -> Option<i32> {
        let by_attributes = |name: &str, attributes: FILE_FLAGS_AND_ATTRIBUTES| {
            file_index(name, attributes, SHGFI_USEFILEATTRIBUTES)
        };
        match icon {
            SystemIcon::Extension(extension) if extension.is_empty() => {
                by_attributes("file", FILE_ATTRIBUTE_NORMAL)
            }
            SystemIcon::Extension(extension) => {
                by_attributes(&format!("file.{extension}"), FILE_ATTRIBUTE_NORMAL)
            }
            SystemIcon::Folder => by_attributes("folder", FILE_ATTRIBUTE_DIRECTORY),
            SystemIcon::Path(path) => {
                file_index(path.to_str()?, FILE_FLAGS_AND_ATTRIBUTES(0), SHGFI_FLAGS(0))
            }
            SystemIcon::Stock(stock) => stock_index(stock_id(*stock)),
        }
    }

    fn stock_id(stock: Stock) -> SHSTOCKICONID {
        match stock {
            Stock::Trash => SIID_RECYCLER,
            Stock::TrashFull => SIID_RECYCLERFULL,
            Stock::Drive => SIID_DRIVEFIXED,
            Stock::Removable => SIID_DRIVEREMOVE,
            Stock::Optical => SIID_DRIVECD,
            Stock::Network => SIID_DRIVENET,
        }
    }

    fn file_index(
        name: &str,
        attributes: FILE_FLAGS_AND_ATTRIBUTES,
        flags: SHGFI_FLAGS,
    ) -> Option<i32> {
        let mut info = SHFILEINFOW::default();
        let name = HSTRING::from(name);
        // SAFETY: a NUL-terminated name, and a SHFILEINFOW of the size
        // given, which the shell fills.
        let found = unsafe {
            SHGetFileInfoW(
                &name,
                attributes,
                Some(&raw mut info),
                u32::try_from(size_of::<SHFILEINFOW>()).ok()?,
                SHGFI_SYSICONINDEX | flags,
            )
        };
        (found != 0).then_some(info.iIcon)
    }

    fn stock_index(id: SHSTOCKICONID) -> Option<i32> {
        let mut info = SHSTOCKICONINFO {
            cbSize: u32::try_from(size_of::<SHSTOCKICONINFO>()).ok()?,
            ..Default::default()
        };
        // SAFETY: a SHSTOCKICONINFO whose size it states, filled by the shell.
        unsafe { SHGetStockIconInfo(id, SHGSI_SYSICONINDEX, &raw mut info) }.ok()?;
        // Wine's is a stub that gives no index (and the file icon's place).
        (info.iSysImageIndex >= 0).then_some(info.iSysImageIndex)
    }

    /// The icon at `index` in the system image list `list`, as pixels.
    fn from_list(index: i32, list: u32) -> Option<image::DynamicImage> {
        // SAFETY: the system's own image list of that kind; the icon it
        // gives is ours to destroy.
        let icon = unsafe {
            let list: IImageList = SHGetImageList(i32::try_from(list).ok()?).ok()?;
            list.GetIcon(index, ILD_TRANSPARENT.0).ok()?
        };
        let picture = icon_pixels(icon);
        // SAFETY: the icon `GetIcon` made for us.
        let _ = unsafe { DestroyIcon(icon) };
        picture
    }

    /// An icon's colour bitmap as straight RGBA; where it has no alpha, its
    /// mask says which pixels are transparent.
    fn icon_pixels(icon: HICON) -> Option<image::DynamicImage> {
        let mut info = ICONINFO::default();
        // SAFETY: an ICONINFO for the shell to fill; the two bitmaps it
        // makes are ours to delete, below.
        unsafe { GetIconInfo(icon, &raw mut info) }.ok()?;
        let colour = (!info.hbmColor.is_invalid())
            .then(|| crate::system_thumbs::pixels(info.hbmColor))
            .flatten();
        let picture = match colour {
            Some((picture, true)) => Some(picture),
            Some((picture, false)) => {
                let mut picture = picture.into_rgba8();
                if let Some((mask, _)) = crate::system_thumbs::pixels(info.hbmMask) {
                    let mask = mask.into_rgba8();
                    if mask.dimensions() == picture.dimensions() {
                        for (pixel, masked) in picture.pixels_mut().zip(mask.pixels()) {
                            // A set bit of the mask (white) is transparent.
                            if masked[0] > 127 {
                                pixel[3] = 0;
                            }
                        }
                    }
                }
                Some(image::DynamicImage::ImageRgba8(picture))
            }
            None => None,
        };
        // SAFETY: the bitmaps GetIconInfo made for us.
        unsafe {
            let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
            let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        }
        picture
    }

    /// Whether what is drawn fills no more than a corner of the bitmap: a
    /// small icon the jumbo list put in a large one.
    fn drawn_small(picture: &image::DynamicImage) -> bool {
        let rgba = picture.to_rgba8();
        let (mut right, mut bottom) = (0, 0);
        for (x, y, pixel) in rgba.enumerate_pixels() {
            if pixel[3] > 0 {
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
        right.max(bottom) <= rgba.width() / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_names_say_what_the_system_draws() {
        assert_eq!(
            for_name("user-trash"),
            Some(SystemIcon::Stock(Stock::Trash))
        );
        assert_eq!(
            for_name("user-trash-full-symbolic"),
            Some(SystemIcon::Stock(Stock::TrashFull))
        );
        assert_eq!(
            for_name("drive-harddisk"),
            Some(SystemIcon::Stock(Stock::Drive))
        );
        assert_eq!(
            for_name("drive-harddisk-encrypted"),
            Some(SystemIcon::Stock(Stock::Drive))
        );
        assert_eq!(
            for_name("drive-removable-media"),
            Some(SystemIcon::Stock(Stock::Removable))
        );
        assert_eq!(for_name("folder"), Some(SystemIcon::Folder));
        assert_eq!(for_name("folder-saved-search"), Some(SystemIcon::Folder));
        assert_eq!(for_name("document-open-recent"), None);
        if AVAILABLE {
            assert_eq!(
                for_name("user-home"),
                dirs::home_dir().map(SystemIcon::Path)
            );
        }
    }

    #[test]
    fn a_file_is_drawn_by_its_extension_unless_its_icon_is_its_own() {
        let path = Path::new("/files/Report.PDF");
        assert_eq!(
            for_file(path, OsStr::new("Report.PDF"), false),
            SystemIcon::Extension("pdf".into())
        );
        assert_eq!(
            for_file(Path::new("/files/README"), OsStr::new("README"), false),
            SystemIcon::Extension(String::new())
        );
        assert_eq!(
            for_file(Path::new("/files/sub"), OsStr::new("sub"), true),
            SystemIcon::Folder
        );
        let setup = Path::new("/files/setup.exe");
        let own = for_file(setup, OsStr::new("setup.exe"), false);
        assert_eq!(
            own == SystemIcon::Path(setup.to_path_buf()),
            cfg!(windows),
            "{own:?}"
        );
        let app = Path::new("/Applications/Mail.app");
        let own = for_file(app, OsStr::new("Mail.app"), true);
        assert_eq!(
            own == SystemIcon::Path(app.to_path_buf()),
            cfg!(target_os = "macos"),
            "{own:?}"
        );
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn the_free_desktops_have_no_system_icons() {
        assert!(picture(&SystemIcon::Folder, 32).unwrap().is_none());
    }

    // Wine's shell has icons of its own: what runs here is the asking and
    // the reading of the pixels, not what Windows' icons look like.
    #[cfg(windows)]
    #[test]
    fn the_shell_draws_a_types_icon_a_folders_and_the_trash() {
        for (icon, side) in [
            (SystemIcon::Extension("txt".into()), 32),
            (SystemIcon::Extension(String::new()), 16),
            (SystemIcon::Folder, 48),
            (SystemIcon::Path(std::env::current_exe().unwrap()), 32),
            (SystemIcon::Stock(Stock::Trash), 32),
        ] {
            let found = picture(&icon, side).unwrap();
            // Without an icon bibli draws its own mark instead.
            if found.is_none()
                && matches!(icon, SystemIcon::Stock(_))
                && bibli_core::system::under_wine(
                    "the shell's stock icons (its SHGetStockIconInfo names no icon)",
                )
            {
                continue;
            }
            let picture = found.unwrap_or_else(|| panic!("{icon:?}: no icon"));
            assert!(picture.width.max(picture.height) <= side, "{icon:?}");
            assert!(
                picture
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] > 0),
                "{icon:?}: nothing drawn"
            );
        }
    }
}
