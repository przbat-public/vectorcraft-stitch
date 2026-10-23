//! Thread palettes: the colours the machine formats name their threads from.
//!
//! DST and EXP name no colours at all, so a design read from them gets [`default_thread`]'s
//! colours, one per block, which read well on screen and tell the blocks apart. JEF names one of
//! Janome's 78 threads by index, and PEC one of 64 entries: Brother's 61 threads, then three that
//! are not threads ([`Palette`], [`Palette::is_thread`]). Writing a design names each block's
//! chosen thread when it is of the format's palette (the stitch plan chooses them), else the
//! thread matched to the block's colour by the same rule the plan follows ([`Palette::assign`],
//! [`Palette::numbers`]), and reading names that entry back.
//!
//! Where the tables come from. The numbering is the formats' (EduTechWiki's pages on PEC and JEF,
//! edutechwiki.unige.ch/en/Embroidery_format_PEC and …/Embroidery_format_JEF). The colours, the
//! names and their order are those of pyembroidery's thread tables (MIT,
//! github.com/EmbroidePy/pyembroidery, `EmbThreadPec.py` and `EmbThreadJef.py`), taken when the
//! formats were first written, all but PEC 42's. The makers publish their threads' names and
//! catalogue codes, not screen colours: Brother's thread list (support.brother.com, Artspira thread
//! names) has 61 threads, each name once, and EduTechWiki's tables give most entries other colours
//! than pyembroidery's (61 of PEC's 64, 41 of JEF's 78). So the colours stand in for how the
//! threads look; they are not measurements. From Brother's list and EduTechWiki: PEC 42 is Linen
//! (Brother's 307) in EduTechWiki's colour, where pyembroidery repeats Flesh Pink, and PEC 62 to 64
//! are the format's appliqué entries, not threads ([`Palette::is_thread`]). Thread names are data,
//! not creative text.
//!
//! The tables are credited in `NOTICE` with pyembroidery's MIT notice: the project owner's decision
//! (2026-10-21, `FORK.md` › Clean-room for embroidery), since the makers publish no screen colours.
//! EduTechWiki is cited for facts only (the numbering, PEC 42's name and colour): its default
//! licence, CC BY-NC-SA 3.0, doesn't fit this code's, so its tables are no source to take.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::{Thread, color};

/// A palette entry: sRGB and the thread's name.
pub type Entry = ([u8; 3], &'static str);

/// The number the formats leave for "no colour": no thread has it, so a block without a
/// neighbour on one side leaves it out instead ([`Palette::assign`]).
const NO_THREAD: usize = 0;

/// A thread within this colour difference (CIEDE2000, [`crate::color`]) of the art still looks like
/// it; a thread farther looks clearly unlike it. So a block takes another thread than a
/// neighbouring block's only when that other thread is this near ([`Palette::assign`]), and the
/// stitch plan flags a farther thread as a mismatch. An estimate, the project owner's choice
/// (2026-10-21); no embroidery source gives a threshold. Past about 5, two colours side by side are
/// already seen as two different colours (the bands of W. S. Mokrzycki and M. Tatol, "Color
/// difference Delta E — a survey", *Machine Graphics and Vision* 20, 2011), but those bands were
/// drawn up for the older CIE 1976 difference, which is not on CIEDE2000's scale (CIEDE2000 shrinks
/// differences of saturated colours: its 10 can be a CIE 1976 distance of 20 to 30). A palette of
/// 61 or 78 threads lies farther than 5 from about half the colours of real art, so the line is
/// drawn farther out, at 10, by judgement; and at such distances CIEDE2000 itself is outside the
/// range it was made for ([`crate::color`]).
pub const MISMATCH_DELTA_E: f64 = 10.0;

/// A colour difference is shown to two decimals, in hundredths: finer than any difference the eye
/// sees (about 1 is the smallest seen side by side).
pub const DELTA_E_HUNDREDTHS: f64 = 100.0;

/// Colour difference `delta_e` as it is shown: to [`DELTA_E_HUNDREDTHS`].
pub fn shown_delta_e(delta_e: f64) -> f64 {
    (delta_e * DELTA_E_HUNDREDTHS).round() / DELTA_E_HUNDREDTHS
}

/// Whether a thread `delta_e` from the art looks unlike it: its difference as shown is past
/// [`MISMATCH_DELTA_E`]. As shown, so that a thread is never flagged at a difference shown as the
/// threshold itself, and the same line decides which thread a block takes ([`Palette::assign`]).
pub fn mismatched(delta_e: f64) -> bool {
    shown_delta_e(delta_e) > MISMATCH_DELTA_E
}

/// A thread palette a machine format names its threads from, by number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Palette {
    /// Brother's 61 threads, as PEC (and so PES) numbers them, before three entries that are not
    /// threads ([`PEC`]).
    Pec,
    /// Janome's 78 threads, as JEF numbers them ([`JEF`]).
    Jef,
}

impl Palette {
    /// Every palette.
    pub const ALL: [Self; 2] = [Self::Pec, Self::Jef];

    /// Its name in parameters and files: `pec`, `jef`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Pec => "pec",
            Self::Jef => "jef",
        }
    }

    /// The palette `id` names ([`Palette::id`]).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The format whose threads it numbers, the one a design in it is written as: PES for
    /// Brother's (PEC numbers them too), JEF for Janome's.
    pub fn format(self) -> crate::format::Format {
        match self {
            Self::Pec => crate::format::Format::Pes,
            Self::Jef => crate::format::Format::Jef,
        }
    }

    /// Its entries, each a colour and a name, in the order the format numbers them from 1: its
    /// threads, then, in PEC, three entries that are not ([`Palette::is_thread`]).
    pub fn threads(self) -> &'static [Entry] {
        match self {
            Self::Pec => &PEC,
            Self::Jef => &JEF,
        }
    }

    /// How many of its entries are threads, from the first: Brother's 61 in PEC, all of Janome's
    /// 78.
    fn thread_count(self) -> usize {
        match self {
            Self::Pec => BROTHER_THREADS,
            Self::Jef => JEF.len(),
        }
    }

    /// Whether number `index` (from 1) is one of its threads, which matching may pick: not 0, not
    /// past the palette's end, and not one of PEC's appliqué entries, 62 to 64 ([`PEC`]). A file
    /// that names an entry that isn't a thread still reads in its colour ([`Palette::thread`]).
    pub fn is_thread(self, index: usize) -> bool {
        (1..=self.thread_count()).contains(&index)
    }

    /// Thread `index` (from 1): its colour and name; `None` for 0 (the formats' "no colour") and
    /// past the palette's end.
    pub fn get(self, index: usize) -> Option<Entry> {
        index.checked_sub(1).and_then(|i| self.threads().get(i)).copied()
    }

    /// Entry `index` (from 1) as the thread a block read from a file is in: its colour and name.
    /// `None` when the number names no entry: 0, which the formats leave for "no colour", or a
    /// number past the palette's end, which only a damaged file or a newer chart holds
    /// ([`unknown_thread`]).
    pub fn thread(self, index: usize) -> Option<Thread> {
        self.get(index).map(|(color, name)| Thread::named(color, name))
    }

    /// The thread nearest `color` as the eye sees it, the smallest CIEDE2000 difference
    /// ([`crate::color`]), and how near, leaving out the threads `skip` names. A colour that is a
    /// thread's own is that thread, whatever `skip` says; when `skip` leaves no thread, the
    /// nearest of all. Never an entry that isn't a thread ([`Palette::is_thread`]).
    pub fn nearest(self, color: [u8; 3], skip: &[usize]) -> Match {
        if let Some(own) = self.own(color) {
            return own;
        }
        // A design's colours repeat from plan to plan (a plan after an edit matches every block
        // again): each match is worked out once and remembered, for a skip of up to two threads
        // (the neighbours [`Palette::assign`] avoids).
        let key = match skip {
            [] => Some([NO_THREAD; 2]),
            [a] => Some([*a, NO_THREAD]),
            [a, b] => Some([*a, *b]),
            _ => None,
        }
        .map(|skip| (self, color, skip));
        let memo = || MATCHES.get_or_init(Default::default).lock().ok();
        if let Some(m) = key.and_then(|k| memo()?.get(&k).copied()) {
            return m;
        }
        let m = self.nearest_of_all(color, skip);
        if let (Some(k), Some(mut kept)) = (key, memo()) {
            if kept.len() >= MAX_MATCHES_KEPT {
                kept.clear();
            }
            kept.insert(k, m);
        }
        m
    }

    /// [`Palette::nearest`] for a colour that is no thread's own, worked out: compared with every
    /// thread.
    fn nearest_of_all(self, color: [u8; 3], skip: &[usize]) -> Match {
        let lab = color::lab(color);
        let (mut best, mut any): (Option<Match>, Option<Match>) = (None, None);
        for (i, &thread) in self.labs().iter().enumerate().take(self.thread_count()) {
            let m = Match { index: i + 1, delta_e: color::delta_e(lab, thread) };
            let nearer = |b: Option<Match>| b.is_none_or(|b| m.delta_e < b.delta_e);
            if nearer(any) {
                any = Some(m);
            }
            if !skip.contains(&m.index) && nearer(best) {
                best = Some(m);
            }
        }
        // A palette has threads, so `any` is always one: this stand-in is never taken.
        best.or(any).unwrap_or(Match { index: 1, delta_e: f64::INFINITY })
    }

    /// Its entries' colours in CIE L*a*b* ([`color::lab`]), in its order, worked out once: matching
    /// compares a colour with every thread's.
    fn labs(self) -> &'static [color::Lab] {
        static PEC_LABS: OnceLock<Vec<color::Lab>> = OnceLock::new();
        static JEF_LABS: OnceLock<Vec<color::Lab>> = OnceLock::new();
        let labs = match self {
            Self::Pec => &PEC_LABS,
            Self::Jef => &JEF_LABS,
        };
        labs.get_or_init(|| self.threads().iter().map(|&(rgb, _)| color::lab(rgb)).collect())
    }

    /// The thread whose own colour `color` is, if any (no two threads of a palette share one). An
    /// entry that isn't a thread is no one's: art in its colour is matched like any other.
    fn own(self, color: [u8; 3]) -> Option<Match> {
        self.threads().iter().take(self.thread_count()).position(|&(rgb, _)| rgb == color).map(|i| Match { index: i + 1, delta_e: 0.0 })
    }

    /// The thread each block of a design is sewn in, in sewing order, from each block's colour and
    /// the thread already chosen for it (`None`: to be matched). A block whose thread is chosen
    /// from this palette, or whose colour is a thread's own, has that thread; one chosen from
    /// another palette is matched by its colour. Every other block gets the thread
    /// nearest its colour ([`Palette::nearest`]), unless a neighbouring block has it: the block
    /// before, or the block after when its thread is already known (chosen, or its own colour).
    /// Then it takes the nearest thread neither neighbour has, so that the colour change changes
    /// thread, as long as that one is within [`MISMATCH_DELTA_E`] of its colour; past that, it
    /// keeps its nearest thread, and the machine stops between two blocks of one thread.
    pub fn assign(self, blocks: &[([u8; 3], Option<PaletteThread>)]) -> Vec<Match> {
        let known: Vec<Option<Match>> = blocks
            .iter()
            .map(|&(color, chosen)| match chosen.filter(|c| c.palette == self).and_then(|c| self.get(c.index).map(|entry| (c.index, entry))) {
                Some((index, (rgb, _))) => Some(Match { index, delta_e: color::delta_e_rgb(color, rgb) }),
                None => self.own(color),
            })
            .collect();
        let mut out: Vec<Match> = Vec::with_capacity(blocks.len());
        for (k, &(color, _)) in blocks.iter().enumerate() {
            let m = match known.get(k).copied().flatten() {
                Some(m) => m,
                None => {
                    let nearest = self.nearest(color, &[]);
                    let before = out.last().map_or(NO_THREAD, |m| m.index);
                    let after = known.get(k + 1).copied().flatten().map_or(NO_THREAD, |m| m.index);
                    let neighbours = [before, after];
                    match neighbours.contains(&nearest.index) {
                        false => nearest,
                        true => Some(self.nearest(color, &neighbours)).filter(|other| !mismatched(other.delta_e)).unwrap_or(nearest),
                    }
                }
            };
            out.push(m);
        }
        out
    }

    /// The numbers a format of this palette writes for `threads`, one per block in sewing order
    /// ([`Palette::assign`]): a block's chosen thread when it is one of this palette's
    /// ([`Thread::chosen`]), else the thread matched to its colour.
    pub fn numbers<'a>(self, threads: impl IntoIterator<Item = &'a Thread>) -> Vec<usize> {
        let blocks: Vec<([u8; 3], Option<PaletteThread>)> = threads.into_iter().map(|t| (t.color, t.chosen)).collect();
        self.assign(&blocks).into_iter().map(|m| m.index).collect()
    }
}

/// A thread of a palette: the palette, and the thread's number in it. Made only for a number that
/// names one of the palette's threads ([`PaletteThread::new`]), so a thread chosen for a block is
/// always one; which palette it is from is part of it, so a format of another palette never takes
/// its number for one of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaletteThread {
    palette: Palette,
    /// From 1, as the formats count (their 0 is "no colour").
    index: usize,
}

impl PaletteThread {
    /// Thread `index` (from 1) of `palette`; `None` when the number names none of its threads
    /// ([`Palette::is_thread`]).
    pub fn new(palette: Palette, index: usize) -> Option<Self> {
        palette.is_thread(index).then_some(Self { palette, index })
    }

    /// The palette it is from.
    pub fn palette(self) -> Palette {
        self.palette
    }

    /// Its number in the palette, from 1.
    pub fn index(self) -> usize {
        self.index
    }

    /// The thread `m` matched in `palette` ([`Palette::nearest`], [`Palette::assign`], which only
    /// ever match threads); were `m` no thread of it, the palette's first.
    pub fn matched(palette: Palette, m: Match) -> Self {
        Self::new(palette, m.index).unwrap_or(Self { palette, index: FIRST_THREAD })
    }

    /// Its colour and name as the palette lists them.
    pub fn entry(self) -> Entry {
        self.palette.get(self.index).unwrap_or(NO_ENTRY)
    }
}

/// The number of every palette's first thread (the formats count from 1; 0 is "no colour").
const FIRST_THREAD: usize = 1;
/// What a thread that isn't in its palette would show: never, since a [`PaletteThread`] is always
/// one of its palette's threads.
const NO_ENTRY: Entry = ([0; 3], "");

/// The matches [`Palette::nearest`] has worked out: by palette, colour and the (up to two)
/// threads skipped, [`NO_THREAD`] for none. A lock that a panic elsewhere left poisoned is passed
/// by: matching afresh is always right.
static MATCHES: OnceLock<Mutex<HashMap<MatchKey, Match>>> = OnceLock::new();
/// What a remembered match is of: the palette, the colour and the threads skipped.
type MatchKey = (Palette, [u8; 3], [usize; 2]);
/// The most matches remembered, about 3 MB: past it they are forgotten and remembered again from
/// the next one. Many more than a design's colours. An estimate.
const MAX_MATCHES_KEPT: usize = 65_536;

/// A colour matched to a palette's thread ([`Palette::nearest`], [`Palette::assign`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Match {
    /// The thread's number in the palette, from 1.
    pub index: usize,
    /// The CIEDE2000 difference between the colour and the thread's ([`crate::color`]): 0 when
    /// the colour is the thread's own.
    pub delta_e: f64,
}

/// The colours a design read from a format without colours gets, one per block (cycled).
pub const DEFAULT: [Entry; 8] = [
    ([32, 32, 32], "Charcoal"),
    ([214, 32, 32], "Red"),
    ([28, 84, 168], "Blue"),
    ([30, 132, 60], "Green"),
    ([240, 176, 24], "Gold"),
    ([126, 60, 150], "Purple"),
    ([224, 110, 50], "Orange"),
    ([70, 70, 74], "Slate"),
];

/// How many threads Brother's thread list has (support.brother.com, Artspira thread names): PEC's
/// entries 1 to 61.
const BROTHER_THREADS: usize = 61;

/// PEC's 64 entries, numbered from 1 (0 is no colour): Brother's 61 threads, then three that are
/// not threads but the format's appliqué entries ([`Palette::is_thread`]).
pub const PEC: [Entry; 64] = [
    ([14, 31, 124], "Prussian Blue"),
    ([10, 85, 163], "Blue"),
    ([0, 135, 119], "Teal Green"),
    ([75, 107, 175], "Cornflower Blue"),
    ([237, 23, 31], "Red"),
    ([209, 92, 0], "Reddish Brown"),
    ([145, 54, 151], "Magenta"),
    ([228, 154, 203], "Light Lilac"),
    ([145, 95, 172], "Lilac"),
    ([158, 214, 125], "Mint Green"),
    ([232, 169, 0], "Deep Gold"),
    ([254, 186, 53], "Orange"),
    ([255, 255, 0], "Yellow"),
    ([112, 188, 31], "Lime Green"),
    ([186, 152, 0], "Brass"),
    ([168, 168, 168], "Silver"),
    ([125, 111, 0], "Russet Brown"),
    ([255, 255, 179], "Cream Brown"),
    ([79, 85, 86], "Pewter"),
    ([0, 0, 0], "Black"),
    ([11, 61, 145], "Ultramarine"),
    ([119, 1, 118], "Royal Purple"),
    ([41, 49, 51], "Dark Gray"),
    ([42, 19, 1], "Dark Brown"),
    ([246, 74, 138], "Deep Rose"),
    ([178, 118, 36], "Light Brown"),
    ([252, 187, 197], "Salmon Pink"),
    ([254, 55, 15], "Vermilion"),
    ([240, 240, 240], "White"),
    ([106, 28, 138], "Violet"),
    ([168, 221, 196], "Seacrest"),
    ([37, 132, 187], "Sky Blue"),
    ([254, 179, 67], "Pumpkin"),
    ([255, 243, 107], "Cream Yellow"),
    ([208, 166, 96], "Khaki"),
    ([209, 84, 0], "Clay Brown"),
    ([102, 186, 73], "Leaf Green"),
    ([19, 74, 70], "Peacock Blue"),
    ([135, 135, 135], "Gray"),
    ([216, 204, 198], "Warm Gray"),
    ([67, 86, 7], "Dark Olive"),
    // Brother's Linen (307 in Brother's thread list), in the colour EduTechWiki's PEC page gives it.
    ([255, 227, 198], "Linen"),
    ([249, 147, 188], "Pink"),
    ([0, 56, 34], "Deep Green"),
    ([178, 175, 212], "Lavender"),
    ([104, 106, 176], "Wisteria Violet"),
    ([239, 227, 185], "Beige"),
    ([247, 56, 102], "Carmine"),
    ([181, 75, 100], "Amber Red"),
    ([19, 43, 26], "Olive Green"),
    ([199, 1, 86], "Dark Fuchsia"),
    ([254, 158, 50], "Tangerine"),
    ([168, 222, 235], "Light Blue"),
    ([0, 103, 62], "Emerald Green"),
    ([78, 41, 144], "Purple"),
    ([47, 126, 32], "Moss Green"),
    ([255, 204, 204], "Flesh Pink"),
    ([255, 217, 17], "Harvest Gold"),
    ([9, 91, 166], "Electric Blue"),
    ([240, 249, 112], "Lemon Yellow"),
    ([227, 243, 91], "Fresh Green"),
    // Not threads: EduTechWiki's PEC page names entries 62 to 64 Applique Material, Applique
    // Position and Applique. Files from other open-source tools use 62 and 63 for an orange and a
    // cream yellow (the real designs' fox does), so they read in these colours and names, as they
    // always have; matching never picks them.
    ([255, 153, 0], "Orange"),
    ([255, 240, 141], "Cream Yellow"),
    ([255, 200, 200], "Applique"),
];

/// Janome's 78-thread palette, as JEF counts it (index 1 to 78; 0 is no colour).
pub const JEF: [Entry; 78] = [
    ([0, 0, 0], "Black"),
    ([255, 255, 255], "White"),
    ([255, 255, 23], "Yellow"),
    ([255, 102, 0], "Orange"),
    ([47, 89, 51], "Olive Green"),
    ([35, 115, 54], "Green"),
    ([101, 194, 200], "Sky"),
    ([171, 90, 150], "Purple"),
    ([246, 105, 160], "Pink"),
    ([255, 0, 0], "Red"),
    ([177, 112, 78], "Brown"),
    ([11, 47, 132], "Blue"),
    ([228, 195, 93], "Gold"),
    ([72, 26, 5], "Dark Brown"),
    ([172, 156, 199], "Pale Violet"),
    ([252, 242, 148], "Pale Yellow"),
    ([249, 153, 183], "Pale Pink"),
    ([250, 179, 129], "Peach"),
    ([201, 164, 128], "Beige"),
    ([151, 5, 51], "Wine Red"),
    ([160, 184, 204], "Pale Sky"),
    ([127, 194, 28], "Yellow Green"),
    ([229, 229, 229], "Silver Gray"),
    ([136, 155, 155], "Gray"),
    ([152, 214, 189], "Pale Aqua"),
    ([178, 225, 227], "Baby Blue"),
    ([54, 139, 160], "Powder Blue"),
    ([79, 131, 171], "Bright Blue"),
    ([56, 106, 145], "Slate Blue"),
    ([7, 22, 80], "Navy Blue"),
    ([249, 153, 162], "Salmon Pink"),
    ([249, 103, 107], "Coral"),
    ([227, 49, 31], "Burnt Orange"),
    ([226, 161, 136], "Cinnamon"),
    ([181, 148, 116], "Umber"),
    ([228, 207, 153], "Blond"),
    ([255, 203, 0], "Sunflower"),
    ([225, 173, 212], "Orchid Pink"),
    ([195, 0, 126], "Peony Purple"),
    ([128, 0, 75], "Burgundy"),
    ([84, 5, 113], "Royal Purple"),
    ([177, 5, 37], "Cardinal Red"),
    ([202, 224, 192], "Opal Green"),
    ([137, 152, 86], "Moss Green"),
    ([92, 148, 26], "Meadow Green"),
    ([0, 49, 20], "Dark Green"),
    ([93, 174, 148], "Aquamarine"),
    ([76, 191, 143], "Emerald Green"),
    ([0, 119, 114], "Peacock Green"),
    ([89, 91, 97], "Dark Gray"),
    ([255, 255, 242], "Ivory White"),
    ([177, 88, 24], "Hazel"),
    ([203, 138, 7], "Toast"),
    ([152, 108, 128], "Salmon"),
    ([152, 105, 45], "Cocoa Brown"),
    ([77, 52, 25], "Sienna"),
    ([76, 51, 11], "Sepia"),
    ([51, 32, 10], "Dark Sepia"),
    ([82, 58, 151], "Violet Blue"),
    ([13, 33, 126], "Blue Ink"),
    ([30, 119, 172], "Sola Blue"),
    ([178, 221, 83], "Green Dust"),
    ([243, 54, 137], "Crimson"),
    ([222, 100, 158], "Floral Pink"),
    ([152, 65, 97], "Wine"),
    ([76, 86, 18], "Olive Drab"),
    ([76, 136, 31], "Meadow"),
    ([228, 222, 121], "Mustard"),
    ([203, 138, 26], "Yellow Ocher"),
    ([203, 162, 28], "Old Gold"),
    ([255, 152, 5], "Honey Dew"),
    ([252, 178, 87], "Tangerine"),
    ([255, 229, 5], "Canary Yellow"),
    ([240, 51, 31], "Vermilion"),
    ([26, 132, 45], "Bright Green"),
    ([56, 108, 174], "Ocean Blue"),
    ([227, 196, 180], "Beige Gray"),
    ([227, 172, 129], "Bamboo"),
];

/// The default thread of block `index` (the palette cycled).
pub fn default_thread(index: usize) -> Thread {
    let (color, name) = DEFAULT.get(index % DEFAULT.len()).copied().unwrap_or(DEFAULT[0]);
    Thread::named(color, name)
}

/// The thread a block read from a file is in when the file names thread `number`, which names no
/// entry of its palette ([`Palette::thread`]): block `block`'s default thread's colour
/// ([`default_thread`]), so that the blocks stay told apart, as in a design from a format without
/// colours; named after the number ("Unknown thread 85"), so that it is never taken for one of the
/// palette's threads (a 0 read as Black would be).
pub fn unknown_thread(block: usize, number: u64) -> Thread {
    Thread { name: format!("Unknown thread {number}"), ..Thread::rgb(default_thread(block).color) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A match is always a thread of its palette; the stand-in for one that isn't, the first
    /// thread, is one in every palette.
    #[test]
    fn a_matched_thread_is_always_one_of_its_palette() {
        for palette in Palette::ALL {
            assert!(palette.is_thread(FIRST_THREAD), "{palette:?}");
            let m = palette.nearest([10, 200, 90], &[]);
            let t = PaletteThread::matched(palette, m);
            assert_eq!((t.palette(), t.index()), (palette, m.index));
            assert_eq!(t.entry(), palette.get(m.index).unwrap());
            assert_eq!(PaletteThread::matched(palette, Match { index: 0, delta_e: 0.0 }).index(), FIRST_THREAD);
        }
    }

    /// A thread chosen for a block is its number when it is the palette's own; another palette's
    /// is matched by colour; a number the palette doesn't have is no thread; and a block isn't
    /// matched to the thread of the block before, chosen or not.
    #[test]
    fn numbers_take_the_chosen_thread_of_their_palette() {
        let red = [200, 30, 30];
        let chosen = |palette, index, color| Thread { chosen: PaletteThread::new(palette, index), ..Thread::rgb(color) };
        let nearest = Palette::Pec.nearest(red, &[]).index;
        assert_eq!(Palette::Pec.numbers(&[chosen(Palette::Pec, 40, red)]), [40]);
        assert_eq!(Palette::Pec.numbers(&[chosen(Palette::Jef, 40, red)]), [nearest], "Janome's 40");
        assert_eq!((PaletteThread::new(Palette::Pec, 65), PaletteThread::new(Palette::Pec, 0)), (None, None), "past the palette, no colour");
        let jef = PaletteThread::new(Palette::Jef, 78).unwrap();
        assert_eq!((jef.palette(), jef.index(), PaletteThread::new(Palette::Jef, 79)), (Palette::Jef, 78, None));
        // An orange whose next nearest thread is similar takes it next to its nearest.
        let orange = [254, 180, 58];
        let first = Palette::Pec.nearest(orange, &[]).index;
        let next = Palette::Pec.nearest(orange, &[first]);
        assert!(next.index != first && next.delta_e <= MISMATCH_DELTA_E, "{next:?}");
        let next = next.index;
        assert_eq!(Palette::Pec.numbers(&[chosen(Palette::Pec, first, orange), Thread::rgb(orange)]), [first, next]);
        assert_eq!(Palette::Pec.numbers(&[Thread::rgb(orange), Thread::rgb(orange)]), [first, next]);
        assert_eq!(Palette::Pec.numbers(&[Thread::rgb(orange), chosen(Palette::Pec, first, orange)]), [next, first], "the block after's");
        // A red with no similar second thread keeps its nearest next to it.
        assert!(Palette::Pec.nearest(red, &[nearest]).delta_e > MISMATCH_DELTA_E);
        assert_eq!(Palette::Pec.numbers(&[Thread::rgb(red), Thread::rgb(red)]), [nearest, nearest]);
    }

    /// A palette's threads are numbered from 1, as the formats count; 0 and numbers past the end
    /// name none, and a block a file gives one of them is in a stand-in thread named after it.
    #[test]
    fn threads_are_numbered_from_one() {
        for palette in Palette::ALL {
            let threads = palette.threads();
            assert_eq!(palette.get(1), threads.first().copied());
            assert_eq!(palette.get(threads.len()), threads.last().copied());
            assert_eq!((palette.get(0), palette.get(threads.len() + 1)), (None, None));
            assert_eq!((palette.thread(0), palette.thread(threads.len() + 1)), (None, None));
            assert_eq!(palette.thread(1), threads.first().map(|&(rgb, name)| Thread::named(rgb, name)));
            assert_eq!(Palette::from_id(palette.id()), Some(palette));
            // Each thread is its own colour's nearest, exactly, even when it is left out.
            for (i, &(rgb, _)) in threads.iter().enumerate().filter(|(i, _)| palette.is_thread(i + 1)) {
                assert_eq!(palette.nearest(rgb, &[]), Match { index: i + 1, delta_e: 0.0 });
                assert_eq!(palette.nearest(rgb, &[i + 1]), Match { index: i + 1, delta_e: 0.0 });
            }
        }
    }

    /// A block whose file names no thread of the palette is in its block's default colour, never
    /// in one of the palette's threads, and named after the number it was given.
    #[test]
    fn a_number_that_names_no_thread_reads_as_a_stand_in() {
        for (block, number) in [(0, 0), (2, 79), (9, u64::from(u32::MAX))] {
            let t = unknown_thread(block, number);
            assert_eq!((t.color, t.name.as_str(), t.chosen), (default_thread(block).color, format!("Unknown thread {number}").as_str(), None));
        }
    }

    /// PEC's entries 62 to 64 are not threads: a file that names them reads in their colours, but
    /// no colour is ever matched to them, not even their own, and no thread is chosen from them,
    /// so neither a plan nor a PES file ever names them.
    #[test]
    fn pecs_applique_entries_are_never_matched() {
        let pec = Palette::Pec;
        assert!(pec.is_thread(1) && pec.is_thread(BROTHER_THREADS) && !pec.is_thread(0) && !pec.is_thread(65));
        for index in [62, 63, 64] {
            assert!(!pec.is_thread(index) && PaletteThread::new(pec, index).is_none(), "{index}");
            let (rgb, name) = pec.get(index).unwrap();
            assert_eq!(pec.thread(index), Some(Thread::named(rgb, name)), "read as before");
            let m = pec.nearest(rgb, &[]);
            assert!(pec.is_thread(m.index) && m.delta_e > 0.0, "{index}: {m:?}");
        }
        // Every entry's colour, in every order of neighbours, gets a thread.
        let colours: Vec<([u8; 3], Option<PaletteThread>)> = PEC.iter().map(|&(rgb, _)| (rgb, None)).collect();
        for m in pec.assign(&colours).iter().chain(&pec.assign(&colours.iter().rev().copied().collect::<Vec<_>>())) {
            assert!(pec.is_thread(m.index), "{m:?}");
        }
        assert!(Palette::Jef.threads().iter().enumerate().all(|(i, _)| Palette::Jef.is_thread(i + 1)), "all Janome's entries are threads");
    }

    /// Brother's threads, PEC's first 61 entries, each have a name of their own, as Brother's
    /// thread list gives them (support.brother.com, Artspira thread names: 61 threads, each name
    /// once), and 42 is Linen.
    #[test]
    fn brothers_threads_each_have_their_own_name() {
        let names: Vec<&str> = PEC.iter().take(BROTHER_THREADS).map(|(_, name)| *name).collect();
        let mut distinct = names.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), BROTHER_THREADS, "{names:?}");
        assert_eq!(Palette::Pec.get(42), Some(([255, 227, 198], "Linen")));
    }

    /// The nearest thread is the one that looks nearest, not the one nearest in RGB: a teal is
    /// sewn in Brother's Teal Green, which RGB distance passes over for Sky Blue, about twice as
    /// far by eye.
    #[test]
    fn the_nearest_thread_is_the_one_that_looks_nearest() {
        let teal = [0x33, 0x99, 0x99];
        let rgb = |x: [u8; 3]| (0..3).map(|i| (i32::from(x[i]) - i32::from(teal[i])).pow(2)).sum::<i32>();
        let by_rgb = Palette::Pec.threads().iter().enumerate().min_by_key(|(_, (c, _))| rgb(*c)).map(|(i, _)| i + 1);
        assert_eq!(Palette::Pec.get(by_rgb.unwrap()).map(|(_, name)| name), Some("Sky Blue"));
        let m = Palette::Pec.nearest(teal, &[]);
        assert_eq!(Palette::Pec.get(m.index).map(|(_, name)| name), Some("Teal Green"));
        let sky = color::delta_e_rgb(teal, Palette::Pec.get(by_rgb.unwrap()).unwrap().0);
        assert!(sky > 1.5 * m.delta_e, "Teal Green {} against Sky Blue {sky}", m.delta_e);
        // Every colour's thread is the smallest difference in the palette.
        for c in [[0u8, 0, 0], [255, 255, 255], [200, 30, 30], [64, 128, 255], [250, 230, 10], [90, 60, 30]] {
            let m = Palette::Jef.nearest(c, &[]);
            let smallest = Palette::Jef.threads().iter().map(|(t, _)| color::delta_e_rgb(c, *t)).fold(f64::INFINITY, f64::min);
            assert_eq!(m.delta_e, smallest, "{c:?}");
        }
    }

    /// The threads of `palette` nearest `color`, nearest first.
    fn ranked(palette: Palette, color: [u8; 3]) -> Vec<usize> {
        let mut all: Vec<(f64, usize)> = palette.threads().iter().enumerate().map(|(i, (t, _))| (color::delta_e_rgb(color, *t), i + 1)).collect();
        all.sort_by(|a, b| a.0.total_cmp(&b.0));
        all.into_iter().map(|(_, index)| index).collect()
    }

    /// A block takes another thread than a neighbour's only for one still similar to its colour
    /// (within MISMATCH_DELTA_E): next to a block of its nearest thread's own colour, before or
    /// after it, it takes its next nearest when that is similar, and its nearest otherwise; a block
    /// between two such blocks leaves out both their threads under the same condition. Two blocks
    /// of one thread's own colour keep it both.
    #[test]
    fn a_block_avoids_a_neighbours_thread_only_for_a_similar_one() {
        let indices =
            |palette: Palette, blocks: &[([u8; 3], Option<PaletteThread>)]| palette.assign(blocks).iter().map(|m| m.index).collect::<Vec<_>>();
        let own = |palette: Palette, index: usize| palette.get(index).map(|(rgb, _)| rgb).unwrap();
        // An orange with three similar Brother threads.
        let (pec, orange) = (Palette::Pec, [254, 180, 58]);
        let order = ranked(pec, orange);
        let &[first, second, third, ..] = order.as_slice() else { panic!("a palette of threads") };
        assert!(color::delta_e_rgb(orange, own(pec, third)) <= MISMATCH_DELTA_E, "three similar threads");
        assert_eq!(indices(pec, &[(orange, None), (own(pec, first), None)]), [second, first]);
        assert_eq!(indices(pec, &[(own(pec, first), None), (orange, None)]), [first, second]);
        assert_eq!(indices(pec, &[(own(pec, first), None), (orange, None), (own(pec, second), None)]), [first, third, second]);
        assert_eq!(indices(pec, &[(orange, None), (orange, PaletteThread::new(pec, first))]), [second, first], "a chosen thread is known too");
        let janome = PaletteThread::new(Palette::Jef, first);
        assert_eq!(indices(pec, &[(orange, None), (orange, janome)]), [first, second], "Janome's thread of that number is not");
        let m = pec.assign(&[(orange, None), (own(pec, first), None)]);
        assert_eq!((m[0].delta_e, m[1].delta_e), (color::delta_e_rgb(orange, own(pec, second)), 0.0));
        // The zebra's dark grey: nothing in Janome's palette is similar but Black, which it keeps
        // between a Dark Gray block and a Black one.
        let (jef, grey) = (Palette::Jef, [0x29, 0x2f, 0x33]);
        let order = ranked(jef, grey);
        let &[black, dark_gray, ..] = order.as_slice() else { panic!("a palette of threads") };
        assert!(color::delta_e_rgb(grey, own(jef, dark_gray)) > MISMATCH_DELTA_E);
        assert_eq!(indices(jef, &[(own(jef, dark_gray), None), (grey, None), (own(jef, black), None)]), [dark_gray, black, black]);
        assert_eq!(indices(jef, &[(grey, None), (own(jef, black), None)]), [black, black]);
        // Two blocks of one thread's own colour keep it both.
        assert_eq!(indices(pec, &[(own(pec, first), None), (own(pec, first), None)]), [first, first]);
    }
}
