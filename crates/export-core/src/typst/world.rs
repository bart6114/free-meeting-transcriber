use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::{Datelike, Timelike};
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

static LIBRARY: OnceLock<LazyHash<Library>> = OnceLock::new();
static FONTS: OnceLock<(Vec<Font>, LazyHash<FontBook>)> = OnceLock::new();

fn library() -> &'static LazyHash<Library> {
    LIBRARY.get_or_init(|| LazyHash::new(Library::default()))
}

fn fonts() -> &'static (Vec<Font>, LazyHash<FontBook>) {
    FONTS.get_or_init(|| {
        let mut fonts: Vec<Font> = typst_assets::fonts()
            .flat_map(|data| Font::iter(Bytes::new(data)))
            .collect();

        const PRETENDARD_REGULAR: &[u8] = include_bytes!("../../fonts/Pretendard-Regular.otf");
        const PRETENDARD_MEDIUM: &[u8] = include_bytes!("../../fonts/Pretendard-Medium.otf");
        const PRETENDARD_SEMIBOLD: &[u8] = include_bytes!("../../fonts/Pretendard-SemiBold.otf");
        const PRETENDARD_BOLD: &[u8] = include_bytes!("../../fonts/Pretendard-Bold.otf");

        fonts.extend(Font::iter(Bytes::new(PRETENDARD_REGULAR)));
        fonts.extend(Font::iter(Bytes::new(PRETENDARD_MEDIUM)));
        fonts.extend(Font::iter(Bytes::new(PRETENDARD_SEMIBOLD)));
        fonts.extend(Font::iter(Bytes::new(PRETENDARD_BOLD)));

        let font_book = FontBook::from_fonts(fonts.iter());
        (fonts, LazyHash::new(font_book))
    })
}

pub struct TypstWorld {
    source: Source,
    // Embedded assets (attachment images) keyed by rooted virtual path,
    // e.g. "/att-0.png". A detached source cannot resolve paths, so the main
    // source gets a real virtual FileId.
    files: HashMap<FileId, Bytes>,
}

impl TypstWorld {
    pub fn new(content: String, files: HashMap<String, Vec<u8>>) -> Self {
        let source = Source::new(FileId::new(None, VirtualPath::new("/main.typ")), content);
        let files = files
            .into_iter()
            .map(|(vpath, bytes)| {
                (
                    FileId::new(None, VirtualPath::new(&vpath)),
                    Bytes::new(bytes),
                )
            })
            .collect();
        Self { source, files }
    }
}

impl World for TypstWorld {
    fn library(&self) -> &LazyHash<Library> {
        library()
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &fonts().1
    }

    fn main(&self) -> FileId {
        self.source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() {
            Ok(self.source.clone())
        } else {
            Err(FileError::NotFound(id.vpath().as_rootless_path().into()))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files
            .get(&id)
            .cloned()
            .ok_or_else(|| FileError::NotFound(id.vpath().as_rootless_path().into()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        fonts().0.get(index).cloned()
    }

    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        let now = chrono::Local::now();
        Datetime::from_ymd_hms(
            now.year(),
            now.month().try_into().ok()?,
            now.day().try_into().ok()?,
            now.hour().try_into().ok()?,
            now.minute().try_into().ok()?,
            now.second().try_into().ok()?,
        )
    }
}
