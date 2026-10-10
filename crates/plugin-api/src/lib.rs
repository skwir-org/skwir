//! What a plugin from outside skwir is, and what it speaks (ADR-045,
//! ADR-046). Such a plugin lives in a folder of its own with a
//! `plugin.toml` ([`Manifest`]): what it is, what it may do, what it
//! provides, and how it runs, as a program that speaks the [`protocol`]
//! over its standard input and output (`[run] program`), or as a
//! WebAssembly module (`[run] module`) whose interface is
//! `wit/plugin.wit`, the same methods. skwir hands it the files it is
//! asked about by handle, never by path to open; it reads them through
//! skwir, which checks what it may read.
//!
//! A plugin written in Rust implements [`guest::Plugin`] and is served by
//! [`guest::stdio::serve`] (a program) or [`export!`] (a module). This
//! crate depends on nothing of skwir's: it is the contract both sides
//! build on.

pub mod base64;
pub mod guest;
pub mod manifest;
pub mod protocol;

pub use manifest::Manifest;
