//! The embroidery thread libraries (Window › Swatch Libraries › Embroidery Threads): the palettes
//! the machine formats number their threads from, as read-only swatch libraries, built from the
//! stitch crate's palettes (no file of their own). The swatch library commands (`swatchlib`) list
//! and open them beside the built-in libraries, through two hooks.

use std::sync::{Arc, OnceLock};

use vectorcraft_color::{Color, Paint, Swatch, SwatchLibrary};
use vectorcraft_stitch::thread;

use crate::cmd::swatchlib::LibraryInfo;

/// The category `swatch.library.list` gives the embroidery thread libraries.
pub const THREADS: &str = "threads";

/// An embroidery thread library: the threads a machine format numbers its colours from, as
/// swatches. In a document of that palette, art painted with one of them is sewn in that thread
/// exactly and any other colour is matched to the nearest; the format of that palette writes
/// that thread whatever the document's palette.
#[derive(Clone, Copy, Debug)]
pub struct ThreadLibrary {
    pub id: &'static str,
    pub name: &'static str,
    pub palette: thread::Palette,
}

impl ThreadLibrary {
    /// `palette`'s library. Every palette has one: a palette added to the stitch crate doesn't
    /// compile here until it is given its library's id and name.
    pub const fn of(palette: thread::Palette) -> Self {
        match palette {
            thread::Palette::Pec => Self { id: "brother-threads-pec", name: "Brother Threads (PEC)", palette },
            thread::Palette::Jef => Self { id: "janome-threads-jef", name: "Janome Threads (JEF)", palette },
        }
    }
}

/// The embroidery thread libraries, in menu order: one for each palette the PES and JEF writers
/// number their threads from ([`vectorcraft_stitch::thread::Palette::ALL`], in its order).
pub const THREAD_LIBRARIES: [ThreadLibrary; thread::Palette::ALL.len()] = {
    let mut libraries = [ThreadLibrary::of(thread::Palette::Pec); thread::Palette::ALL.len()];
    let mut i = 0;
    while i < libraries.len() {
        libraries[i] = ThreadLibrary::of(thread::Palette::ALL[i]);
        i += 1;
    }
    libraries
};

/// `palette`'s threads as swatches, in its order (PEC's appliqué entries, which are not threads,
/// left out: [`thread::Palette::is_thread`]): each named by the palette, its number and its name
/// ("PEC 5 Red"), as a list of the plan's threads shows them.
pub fn thread_swatches(palette: thread::Palette) -> Vec<Swatch> {
    let code = palette.id().to_ascii_uppercase();
    let swatch = |(i, &([r, g, b], name)): (usize, &([u8; 3], &str))| Swatch {
        name: format!("{code} {} {name}", i + 1),
        paint: Paint::solid(Color::rgb8(r, g, b)),
        global: false,
        spot: false,
    };
    palette.threads().iter().enumerate().filter(|(i, _)| palette.is_thread(i + 1)).map(swatch).collect()
}

/// The thread libraries as `swatch.library.list` lists them, after the built-in ones.
pub(crate) fn infos() -> impl Iterator<Item = LibraryInfo> {
    THREAD_LIBRARIES.iter().map(|t| LibraryInfo { id: t.id.into(), name: t.name.into(), category: THREADS })
}

/// Thread library `id` with its info, built once; `None` for an id that isn't one.
pub(crate) fn library(id: &str) -> Option<(LibraryInfo, Arc<SwatchLibrary>)> {
    static LIBS: OnceLock<Vec<Arc<SwatchLibrary>>> = OnceLock::new();
    let libs = LIBS.get_or_init(|| {
        let make = |t: &ThreadLibrary| Arc::new(SwatchLibrary { name: t.name.into(), swatches: thread_swatches(t.palette), groups: vec![] });
        THREAD_LIBRARIES.iter().map(make).collect()
    });
    let at = THREAD_LIBRARIES.iter().position(|t| t.id == id)?;
    Some((infos().nth(at)?, libs.get(at)?.clone()))
}
