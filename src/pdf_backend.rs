pub type Response<T> = smol::channel::Receiver<Result<T, PdfError>>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug)]
pub struct PdfInfo {
    pub pages: Vec<PageSize>,
    pub can_copy: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub struct PageImage {
    pub png: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub text: String,
    pub rects: Vec<PageRect>,
}

#[derive(Clone, Debug)]
pub struct SearchMatch {
    pub page: usize,
    pub rects: Vec<PageRect>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfError {
    Unavailable,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Invalid,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Locked,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    TooLarge,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    PageMissing,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    CopyDenied,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Unsupported,
}

#[cfg(target_os = "macos")]
#[path = "pdf_macos.rs"]
mod platform;

#[cfg(target_os = "macos")]
pub use platform::{inspect, render_page, search, select};

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;
    use std::path::PathBuf;

    fn unsupported<T>() -> Response<T> {
        let (send, receive) = smol::channel::bounded(1);
        let _ = send.try_send(Err(PdfError::Unsupported));
        receive
    }

    pub fn inspect(_: PathBuf) -> Response<PdfInfo> {
        unsupported()
    }

    pub fn render_page(_: PathBuf, _: usize, _: u32) -> Response<PageImage> {
        unsupported()
    }

    pub fn select(_: PathBuf, _: usize, _: [f64; 2], _: [f64; 2]) -> Response<Selection> {
        unsupported()
    }

    pub fn search(_: PathBuf, _: String) -> Response<SearchResults> {
        unsupported()
    }
}

#[cfg(not(target_os = "macos"))]
pub use platform::{inspect, render_page, search, select};
