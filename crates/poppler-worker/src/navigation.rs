//! Small adapter for APIs missing from poppler-rs 0.25's safe bindings.
//! Each allocation is freed with its matching Poppler/GLib function; pointers
//! are borrowed only while their owning document/page/list/action is alive.
#![allow(unsafe_code)] // Native unions, nullable index iterators, and text layout lack safe wrappers.

use std::ffi::CStr;
use std::ptr;

use bibli_core::document::{Destination, Link, Outline, TextChar};
use glib::translate::ToGlibPtr;
use poppler::{Document, Page, ffi};

use crate::rect;

unsafe fn string(pointer: *const std::ffi::c_char) -> String {
    if pointer.is_null() {
        String::new()
    } else {
        // SAFETY: callers pass live, NUL-terminated strings owned by Poppler.
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }
}

unsafe fn destination(
    document: &Document,
    action: *const ffi::PopplerAction,
) -> Option<Destination> {
    // SAFETY: action is borrowed from a live mapping or owned index action;
    // union fields are read only after checking the native discriminant.
    unsafe {
        let action = action.as_ref()?;
        match action.type_ {
            ffi::POPPLER_ACTION_URI => Some(Destination::Uri(string(action.uri.uri))),
            ffi::POPPLER_ACTION_GOTO_DEST => {
                let dest = action.goto_dest.dest.as_ref()?;
                let page = if dest.type_ == ffi::POPPLER_DEST_NAMED {
                    if dest.named_dest.is_null() {
                        return None;
                    }
                    let resolved =
                        ffi::poppler_document_find_dest(document.to_glib_none().0, dest.named_dest);
                    let resolved_ref = resolved.as_ref()?;
                    let page = resolved_ref.page_num;
                    ffi::poppler_dest_free(resolved);
                    page
                } else {
                    dest.page_num
                };
                (page > 0 && page <= document.n_pages()).then_some(Destination::Page(page as u32))
            }
            _ => None, // Never execute launch, JavaScript, or remote-file actions.
        }
    }
}

pub fn outlines(document: &Document) -> Vec<Outline> {
    unsafe fn walk(
        document: &Document,
        iter: *mut ffi::PopplerIndexIter,
        depth: u32,
        result: &mut Vec<Outline>,
    ) {
        if iter.is_null() {
            return;
        }
        // SAFETY: iter is owned by this call. Child iterators and actions are
        // independent allocations, freed even when the traversal hits a limit.
        unsafe {
            loop {
                if result.len() >= 4096 {
                    break;
                }
                let action = ffi::poppler_index_iter_get_action(iter);
                if let Some(value) = action.as_ref() {
                    result.push(Outline {
                        title: string(value.any.title),
                        depth,
                        page: match destination(document, action) {
                            Some(Destination::Page(page)) => Some(page),
                            _ => None,
                        },
                    });
                    ffi::poppler_action_free(action);
                }
                if depth < 32 {
                    walk(
                        document,
                        ffi::poppler_index_iter_get_child(iter),
                        depth + 1,
                        result,
                    );
                }
                if ffi::poppler_index_iter_next(iter) == 0 {
                    break;
                }
            }
            ffi::poppler_index_iter_free(iter);
        }
    }
    let mut result = Vec::new();
    // SAFETY: nullable root is handled by walk; document remains alive.
    unsafe {
        walk(
            document,
            ffi::poppler_index_iter_new(document.to_glib_none().0),
            0,
            &mut result,
        );
    }
    result
}

pub fn links(document: &Document, page: &Page, size: (f64, f64)) -> Vec<Link> {
    let mut result = Vec::new();
    // SAFETY: the list owns its mappings and their actions. All are borrowed
    // until free_link_mapping, which releases the full list once, below.
    unsafe {
        let list = ffi::poppler_page_get_link_mapping(page.to_glib_none().0);
        let mut node = list;
        for _ in 0..4096 {
            let Some(value) = node.as_ref() else {
                break;
            };
            if let Some(mapping) = value.data.cast::<ffi::PopplerLinkMapping>().as_ref()
                && let Some(destination) = destination(document, mapping.action)
            {
                let area = mapping.area;
                result.push(Link {
                    bounds: rect(size, area.x1, area.y1, area.x2, area.y2, true),
                    destination,
                });
            }
            node = value.next;
        }
        ffi::poppler_page_free_link_mapping(list);
    }
    result
}

pub fn chars(page: &Page, size: (f64, f64)) -> anyhow::Result<Vec<TextChar>> {
    let text = page.text().unwrap_or_default();
    anyhow::ensure!(text.chars().count() <= 200_000, "page text exceeds limit");
    // SAFETY: Poppler allocates count contiguous rectangles. Copy the bounded
    // geometry while alive, then free the array even on invalid/oversize layout.
    let bounds = unsafe {
        let mut rectangles = ptr::null_mut();
        let mut count = 0;
        ffi::poppler_page_get_text_layout(
            page.to_glib_none().0,
            &raw mut rectangles,
            &raw mut count,
        );
        let bounds = if rectangles.is_null() || count > 200_000 {
            None
        } else {
            Some(
                std::slice::from_raw_parts(rectangles, count as usize)
                    .iter()
                    .map(|r| rect(size, r.x1, r.y1, r.x2, r.y2, false))
                    .collect::<Vec<_>>(),
            )
        };
        glib::ffi::g_free(rectangles.cast());
        bounds
    };
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let bounds = bounds.ok_or_else(|| anyhow::anyhow!("invalid page text layout"))?;
    anyhow::ensure!(
        bounds.len() == text.chars().count(),
        "page text/layout mismatch"
    );
    Ok(text
        .chars()
        .zip(bounds)
        .map(|(ch, bounds)| TextChar {
            text: ch.to_string(),
            bounds,
        })
        .collect())
}
