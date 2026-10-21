//! Thread palettes: the colours the machine formats name their threads from.
//!
//! DST and EXP name no colours at all, so a design read from them gets [`default_thread`]'s
//! colours, one per block, which read well on screen and tell the blocks apart. JEF names one of
//! Janome's 78 threads by index, and PEC one of Brother's 64 ([`Palette`]). Writing a design names
//! each block's chosen thread when it is of the format's palette (the stitch plan chooses them),
//! else the thread nearest the block's colour ([`Palette::numbers`]), and reading names that
//! thread back.
//!
//! The tables are the manufacturers' published charts as public reverse-engineering documents
//! list them; the names are theirs (thread names are data, not creative text).

use crate::{Thread, color};

/// A palette entry: sRGB and the thread's name.
pub type Entry = ([u8; 3], &'static str);

/// A thread palette a machine format names its threads from, by number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Palette {
    /// Brother's 64 threads, as PEC (and so PES) numbers them ([`PEC`]).
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

    /// Its threads, each a colour and a name, in the order the format numbers them from 1.
    pub fn threads(self) -> &'static [Entry] {
        match self {
            Self::Pec => &PEC,
            Self::Jef => &JEF,
        }
    }

    /// Thread `index` (from 1): its colour and name; `None` for 0 (the formats' "no colour") and
    /// past the palette's end.
    pub fn get(self, index: usize) -> Option<Entry> {
        index.checked_sub(1).and_then(|i| self.threads().get(i)).copied()
    }

    /// Thread `index` (from 1) as a block's thread; 0, the format's "no colour", and a number past
    /// the palette are black.
    pub fn thread(self, index: usize) -> Thread {
        match self.get(index) {
            Some((color, name)) => Thread::named(color, name),
            None => no_colour(),
        }
    }

    /// The thread nearest `color`, and how near. `skip` names the thread the block before is sewn
    /// in: a block whose nearest thread is that one gets the next nearest instead, so that two
    /// colours the palette would sew alike still change thread.
    pub fn nearest(self, color: [u8; 3], skip: Option<usize>) -> Match {
        let threads = self.threads();
        let mut best = (u32::MAX, Match { index: 1, delta_e: f64::INFINITY });
        for (i, &(rgb, _)) in threads.iter().enumerate() {
            let index = i + 1;
            if Some(index) == skip && threads.len() > 1 {
                continue;
            }
            let d = distance(rgb, color);
            if d < best.0 {
                best = (d, Match { index, delta_e: color::delta_e_rgb(color, rgb) });
            }
        }
        best.1
    }

    /// The numbers a format of this palette writes for `threads`, one per block in sewing order:
    /// a block's chosen thread when it is one of this palette's ([`Thread::chosen`]), else the
    /// thread nearest its colour, not the block before's ([`Palette::nearest`]).
    pub fn numbers<'a>(self, threads: impl IntoIterator<Item = &'a Thread>) -> Vec<usize> {
        let mut last = None;
        threads
            .into_iter()
            .map(|t| {
                let index = match t.chosen {
                    Some(c) if c.palette == self && self.get(c.index).is_some() => c.index,
                    _ => self.nearest(t.color, last).index,
                };
                last = Some(index);
                index
            })
            .collect()
    }
}

/// A thread of a palette: the palette, and the thread's number in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaletteThread {
    pub palette: Palette,
    /// From 1, as the formats count (their 0 is "no colour").
    pub index: usize,
}

/// A colour matched to a palette's thread ([`Palette::nearest`]).
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

/// Brother's 64-thread palette, as PEC counts it (index 1 to 64; 0 is no colour).
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
    ([253, 217, 222], "Flesh Pink"),
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

/// The thread for a palette index that names none: black.
fn no_colour() -> Thread {
    Thread::named([0, 0, 0], "Black")
}

/// The squared RGB distance between two colours.
fn distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    let d = |x: u8, y: u8| {
        let v = i32::from(x) - i32::from(y);
        (v * v) as u32
    };
    d(a[0], b[0]) + d(a[1], b[1]) + d(a[2], b[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A thread chosen for a block is its number when it is the palette's own; another palette's,
    /// or a number the palette doesn't have, is matched by colour; and a block isn't matched to
    /// the thread of the block before, chosen or not.
    #[test]
    fn numbers_take_the_chosen_thread_of_their_palette() {
        let red = [200, 30, 30];
        let chosen = |palette, index| Thread { chosen: Some(PaletteThread { palette, index }), ..Thread::rgb(red) };
        let nearest = Palette::Pec.nearest(red, None).index;
        assert_eq!(Palette::Pec.numbers(&[chosen(Palette::Pec, 40)]), [40]);
        assert_eq!(Palette::Pec.numbers(&[chosen(Palette::Jef, 40)]), [nearest], "Janome's 40");
        assert_eq!(Palette::Pec.numbers(&[chosen(Palette::Pec, 65)]), [nearest], "past the palette");
        assert_eq!(Palette::Pec.numbers(&[chosen(Palette::Pec, 0)]), [nearest], "no colour");
        let next = Palette::Pec.nearest(red, Some(nearest)).index;
        assert_ne!(next, nearest);
        assert_eq!(Palette::Pec.numbers(&[chosen(Palette::Pec, nearest), Thread::rgb(red)]), [nearest, next]);
        assert_eq!(Palette::Pec.numbers(&[Thread::rgb(red), Thread::rgb(red)]), [nearest, next]);
    }

    /// A palette's threads are numbered from 1, as the formats count; 0 and numbers past the end
    /// name none and read as black.
    #[test]
    fn threads_are_numbered_from_one() {
        for palette in Palette::ALL {
            let threads = palette.threads();
            assert_eq!(palette.get(1), threads.first().copied());
            assert_eq!(palette.get(threads.len()), threads.last().copied());
            assert_eq!((palette.get(0), palette.get(threads.len() + 1)), (None, None));
            assert_eq!(palette.thread(0), Thread::named([0, 0, 0], "Black"));
            assert_eq!(Palette::from_id(palette.id()), Some(palette));
            // Each thread is its own colour's nearest, exactly.
            for (i, &(rgb, _)) in threads.iter().enumerate() {
                assert_eq!(palette.nearest(rgb, None), Match { index: i + 1, delta_e: 0.0 });
            }
        }
    }
}
