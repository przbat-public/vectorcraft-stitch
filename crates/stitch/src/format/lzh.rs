//! Expanding the compressed streams of Husqvarna Viking HUS files ([`super::hus`]): LZSS with
//! static Huffman codes, sent in blocks, the method of Haruhiko Okumura's ar002, which LHA's lh5
//! uses and the KDE wiki takes for the ARJ archiver's methods 1 to 3, with HUS's sizes.
//!
//! The method, as this module reads it. A stream is a series of tokens. A token up to 255 is that
//! byte; a token from 256 to 509 copies 3 to 256 bytes from up to a kilobyte back in what the
//! stream has made so far, its distance following as a code of its own; 510 ends the stream.
//! Tokens come in blocks, each a 16-bit count of its tokens and three tables of codes, each sent
//! as the length of each of its codes: a table of 19 codes that sends the token table's lengths,
//! the token table (511 codes), and the distance table (15 codes). Codes are read most significant
//! bit first; the shorter codes come first, and codes of one length go to the values in order (a
//! canonical Huffman code), so a table is known from its lengths alone.
//!
//! - The 19-code table and the distance table: how many lengths follow (5 bits; 0 means a single
//!   value, sent in the next 5 bits, which takes no bits to code), then each length in 3 bits, 7
//!   meaning 7 and one more for each 1 bit after it, up to a 0 bit. In the 19-code table, 2 bits
//!   after the third length say how many of the next lengths are 0.
//! - The token table: how many lengths follow (9 bits; 0 means a single token, sent in the next 9
//!   bits), then each length as a code of the 19-code table: 0 is one length 0, 1 is 3 to 18 of
//!   them (4 more bits), 2 is 20 to 531 (9 more bits), and from 3, code k is the length k − 2.
//! - A distance code d is 0 for d = 0, else 2^(d − 1) plus the d − 1 bits that follow it; the copy
//!   starts that many bytes plus one back.
//!
//! The sources. The KDE Community Wiki (*Husqvarna HUS*,
//! <https://community.kde.org/Projects/Liberty/File_Formats/Husqvarna_HUS>) says the compression
//! "does seem to use" ARJ's decoder for its methods 1 to 3, and gives HUS's sizes: a 1,024-byte
//! window (DICSIZE, 10 bits), 511 token codes (NC), 19 codes for the lengths (NT) and 15 distance
//! codes (NP), their counts in 5 bits (TBIT, PBIT), the token count in 9 (CBIT), codes of up to 16
//! bits (CODE_BIT), copies of 3 to 256 bytes (THRESHOLD, MAXMATCH). EduTechWiki's *Embroidery
//! format HUS* (<https://edutechwiki.unige.ch/en/Embroidery_format_HUS>) has the compression from
//! a library of Greenleaf's, who licensed it from ARJ's author, and its table of 511 where ARJ's
//! is of 510; it describes the stream in prose: 9-bit tokens in blocks, each block a 16-bit token
//! count and three tables sent as code lengths, built shortest first with ties to the lowest value,
//! the first table sending the second's lengths with its small values standing for runs of zeros;
//! a byte for a token up to 255, the end at 510, a copy from the 1,024-byte window for the others,
//! its distance from the third table. Its format overview (*Embroidery format*,
//! <https://edutechwiki.unige.ch/en/Embroidery_format>) gives the library as a belief, and Jason
//! Weiler's page (<http://www.jasonweiler.com/HUSandVIPFileFormatInfo.html>) as a fact: Greenleaf's
//! ArchiveLib, in its mode AL_GREENLEAF_LEVEL_4 (proprietary; nothing of it, nor of any port of it,
//! was read). Neither wiki gives the tables' bit layout, and the HUS page has the first table's
//! values above 3 stand for the length 3 less, where this module reads the value k as the length
//! k − 2, as every stream of the real files below expands.
//!
//! The bit layout (the run of 1 bits for lengths of 7 and more, the 2 bits of zeros after the third
//! length, the three codes for runs of zeros and their 4 and 9 bits, the value k as the length
//! k − 2, the distance's extra bits) is the method's as it was known when this module was written,
//! checked on those files. On 2026-11-26 it was confirmed against the source of ar002, the program
//! in which Haruhiko Okumura published the method in 1990 and which he released to the public
//! domain, as its README says (<https://okumuralab.org/~okumura/compression/ar002/>, a later
//! revision of it on his own site; the LHa for UNIX manual says so too,
//! <https://manpages.opensuse.org/Leap-15.6/lha/lha.1.en.html>): every rule above is ar002's. It
//! was read and is cited by the project owner's leave (FORK.md › Clean-room for embroidery); none
//! of its code is copied, and this module is built its own way (it decodes canonical codes from
//! their counts by length, where ar002 looks codes up in tables). ar002 writes its files as LHA's
//! lh5; that ARJ's methods use the same layout rests on the KDE wiki. HUS's sizes are the wikis',
//! not ar002's, and the files bear them out: a 1,024-byte window (ar002's is 8,192), 511 token
//! codes whose last ends the stream (ar002 has 510 and no end), 15 distance codes counted in 5
//! bits (ar002: 14, counted in 4).
//!
//! Checked on 6,944 real HUS files the project owner holds (used on their computer only, never
//! committed): every stream expands to its stated length with the end token next, but in the two
//! files that end before their last records, where a stream stops short of it; every table is a
//! complete code, and no copy reaches back further than 1,020 bytes.
//!
//! What a hostile stream can do here: it is read bit by bit, a table that isn't a complete code is
//! refused (ar002 checks this more loosely: it passes a table of no codes, some over-full ones and
//! lengths past 16, and doesn't check counts, values or runs of zeros past a table, which this
//! module refuses), every length and count is checked before it is used, and each token makes at
//! least one byte, so the work is bounded by the bytes asked for. A stream whose data runs out
//! keeps the bytes made before it did, wherever in the stream the cut falls (what the zero bits
//! read past the end make, or make no sense of, is the cut): a HUS file cut in its last stream
//! reads as far as it goes, and one cut earlier is refused by its header, whose stream offsets then
//! point past its end.

/// A stream that doesn't expand: a table that is no complete code, a value past a table's end, a
/// copy from before the stream's start or further back than its window, a block of no tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Damaged;

/// What a stream expanded to: its bytes, and whether they are all it was asked for (`false` when
/// its data ran out, or it ended, first: the bytes made before then).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Expanded {
    pub(crate) bytes: Vec<u8>,
    pub(crate) whole: bool,
}

/// The window copies reach back into, bytes: a kilobyte (the KDE wiki's DICSIZE, 1024, and
/// EduTechWiki's "lookup window").
const WINDOW: usize = 1024;
/// The token codes: the 256 bytes, the copies and the end (the KDE wiki's NC, 511).
const TOKENS: usize = 511;
/// The first token that copies bytes: tokens below it are bytes.
const FIRST_COPY: usize = 256;
/// The token that ends a stream (EduTechWiki: 510, "exit").
const END: usize = 510;
/// The fewest bytes a copy takes (the KDE wiki's THRESHOLD, 3): the first copy token copies 3
/// bytes, the last before the end 256 (its MAXMATCH).
const SHORTEST_COPY: usize = 3;
/// The codes of the table that sends the token table's lengths (the KDE wiki's NT, 19), and the
/// bits its count takes (TBIT, 5).
const LENGTH_CODES: usize = 19;
const LENGTH_COUNT_BITS: u32 = 5;
/// The distance codes (the KDE wiki's NP, 15), and the bits their count takes (PBIT, 5; ar002's,
/// for its 14 distance codes, is 4: the real files take 5).
const DISTANCE_CODES: usize = 15;
const DISTANCE_COUNT_BITS: u32 = 5;
/// The bits the token table's count takes (the KDE wiki's CBIT, 9).
const TOKEN_COUNT_BITS: u32 = 9;
/// The bits a block's token count takes (EduTechWiki: a 16-bit count).
const BLOCK_COUNT_BITS: u32 = 16;
/// The longest code, bits (the KDE wiki's CODE_BIT, 16).
const LONGEST_CODE: usize = 16;
/// A length in the 19-code and the distance tables takes this many bits; at [`LONG_LENGTH`] it
/// grows by one for each 1 bit after them, up to a 0 bit (ar002's layout, as the next three).
const LENGTH_BITS: u32 = 3;
const LONG_LENGTH: u32 = 7;
/// After this many lengths of the 19-code table, [`ZEROS_AFTER_BITS`] bits say how many of the
/// next lengths are 0.
const ZEROS_AFTER: usize = 3;
const ZEROS_AFTER_BITS: u32 = 2;
/// The 19-code table's codes for runs of zero lengths in the token table: one zero; 3 to 18 zeros
/// in [`SOME_ZEROS_BITS`] more bits; 20 to 531 zeros in [`MANY_ZEROS_BITS`] more bits. A code past
/// them is a length, [`LENGTH_CODE_OFFSET`] less than the code (ar002's layout; EduTechWiki's HUS
/// page has 3 less, which the real files refute).
const ONE_ZERO: usize = 0;
const SOME_ZEROS: usize = 1;
const SOME_ZEROS_BITS: u32 = 4;
const SOME_ZEROS_LEAST: usize = 3;
const MANY_ZEROS: usize = 2;
const MANY_ZEROS_BITS: u32 = 9;
const MANY_ZEROS_LEAST: usize = 20;
const LENGTH_CODE_OFFSET: usize = 2;

/// The first `count` bytes `data`, one compressed stream, expands to: all of them
/// ([`Expanded::whole`]), or those made before the stream's data ran out or it ended. Refused with
/// [`Damaged`] when the stream is no stream of this method.
pub(crate) fn expand(data: &[u8], count: usize) -> Result<Expanded, Damaged> {
    let mut bits = Bits::new(data);
    // At most a byte per token is asked for: the reader bounds `count` (a design's stitches).
    let mut out: Vec<u8> = Vec::with_capacity(count);
    let mut made = 0;
    let read = tokens_into(&mut bits, &mut out, &mut made, count);
    // Past the data's end the bits read are zeros, not the file's: what they make, and a table or
    // a copy they make no sense of, is the stream cut short, and the bytes made before them stay.
    if bits.ran_out {
        out.truncate(made);
        return Ok(Expanded { bytes: out, whole: false });
    }
    read.map(|whole| Expanded { bytes: out, whole })
}

/// Expand the tokens of `bits` into `out`, up to `count` bytes: `true` when they are all made,
/// `false` at the end token or where the data ran out. `made` is how many bytes `out` held before
/// the block or token being read, so that the bytes made from bits past the data's end can go.
fn tokens_into(bits: &mut Bits, out: &mut Vec<u8>, made: &mut usize, count: usize) -> Result<bool, Damaged> {
    let mut left_in_block: u32 = 0;
    let mut tokens = Code::Only(0);
    let mut distances = Code::Only(0);
    while out.len() < count {
        if bits.ran_out {
            return Ok(false);
        }
        *made = out.len();
        if left_in_block == 0 {
            left_in_block = bits.read(BLOCK_COUNT_BITS);
            let lengths = small_table(bits, LENGTH_CODES, LENGTH_COUNT_BITS, Some(ZEROS_AFTER))?;
            tokens = token_table(bits, &lengths)?;
            distances = small_table(bits, DISTANCE_CODES, DISTANCE_COUNT_BITS, None)?;
            if left_in_block == 0 && !bits.ran_out {
                return Err(Damaged);
            }
            continue;
        }
        left_in_block -= 1;
        let token = tokens.decode(bits)?;
        if token == END {
            return Ok(false);
        }
        match token.checked_sub(FIRST_COPY) {
            None => out.push(u8::try_from(token).map_err(|_| Damaged)?),
            Some(copy) => {
                let length = copy + SHORTEST_COPY;
                let code = distances.decode(bits)?;
                let distance = match u32::try_from(code).map_err(|_| Damaged)? {
                    0 => 0,
                    d => (1usize << (d - 1)) + bits.read(d - 1) as usize,
                };
                if distance >= WINDOW {
                    return Err(Damaged);
                }
                let from = out.len().checked_sub(distance + 1).ok_or(Damaged)?;
                for k in 0..length {
                    if out.len() >= count {
                        break;
                    }
                    // The copy may run into the bytes it makes: each is there when it is read.
                    let byte = out.get(from + k).copied().ok_or(Damaged)?;
                    out.push(byte);
                }
            }
        }
    }
    Ok(true)
}

/// A stream's bits, most significant first. Past its end it reads 0 bits and says so
/// ([`Bits::ran_out`]): what is made from them isn't the file's.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    byte: u8,
    left: u32,
    ran_out: bool,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0, byte: 0, left: 0, ran_out: false }
    }

    fn bit(&mut self) -> u32 {
        if self.left == 0 {
            self.byte = match self.data.get(self.at) {
                Some(b) => *b,
                None => {
                    self.ran_out = true;
                    0
                }
            };
            self.at = self.at.saturating_add(1);
            self.left = u8::BITS;
        }
        self.left -= 1;
        u32::from(self.byte >> self.left) & 1
    }

    /// The next `n` bits (at most 16 are asked for) as a number.
    fn read(&mut self, n: u32) -> u32 {
        (0..n).fold(0, |v, _| (v << 1) | self.bit())
    }
}

/// One table's codes: a single value, sent with no bits, or values by the length of their codes.
enum Code {
    Only(usize),
    Lengths {
        /// How many codes each length has, by length (index 0 unused).
        counts: [u16; LONGEST_CODE + 1],
        /// The values with a code, shortest code first, values of one length in order.
        values: Vec<u16>,
    },
}

impl Code {
    /// The code whose values have these code lengths (0: no code): refused unless every code of
    /// the longest length is taken, as a canonical Huffman code's are.
    fn of(lengths: &[u8]) -> Result<Self, Damaged> {
        let mut counts = [0u16; LONGEST_CODE + 1];
        for &l in lengths.iter().filter(|l| **l > 0) {
            let slot = counts.get_mut(usize::from(l)).ok_or(Damaged)?;
            *slot += 1;
        }
        // The codes still free at each length: 2 at length 1, twice what is left at the next.
        let mut free: i64 = 1;
        for &n in counts.iter().skip(1) {
            free = free * 2 - i64::from(n);
            if free < 0 {
                return Err(Damaged);
            }
        }
        if free != 0 {
            return Err(Damaged);
        }
        let mut values: Vec<u16> =
            (0..lengths.len()).filter(|&v| lengths.get(v).is_some_and(|l| *l > 0)).filter_map(|v| u16::try_from(v).ok()).collect();
        values.sort_by_key(|&v| (lengths.get(usize::from(v)).copied().unwrap_or_default(), v));
        Ok(Self::Lengths { counts, values })
    }

    /// The value the next code in `bits` stands for.
    fn decode(&self, bits: &mut Bits) -> Result<usize, Damaged> {
        let (counts, values) = match self {
            Self::Only(v) => return Ok(*v),
            Self::Lengths { counts, values } => (counts, values),
        };
        // The code read so far, the first code of its length, and where that length's values start.
        let (mut code, mut first, mut index) = (0usize, 0usize, 0usize);
        for &n in counts.iter().skip(1) {
            code |= bits.bit() as usize;
            let n = usize::from(n);
            if code < first + n {
                return values.get(index + code - first).map(|v| usize::from(*v)).ok_or(Damaged);
            }
            index += n;
            first = (first + n) << 1;
            code <<= 1;
        }
        Err(Damaged)
    }
}

/// The 19-code table or the distance table: `codes` codes, their count in `count_bits` bits, and
/// for the first the run of zero lengths after [`ZEROS_AFTER`] of them.
fn small_table(bits: &mut Bits, codes: usize, count_bits: u32, zeros_after: Option<usize>) -> Result<Code, Damaged> {
    let n = bits.read(count_bits) as usize;
    if n == 0 {
        let only = bits.read(count_bits) as usize;
        return if only < codes { Ok(Code::Only(only)) } else { Err(Damaged) };
    }
    if n > codes {
        return Err(Damaged);
    }
    let mut lengths = vec![0u8; codes];
    let mut i = 0;
    while i < n {
        let mut length = bits.read(LENGTH_BITS);
        if length == LONG_LENGTH {
            while bits.bit() == 1 {
                length += 1;
                if length as usize > LONGEST_CODE {
                    return Err(Damaged);
                }
            }
        }
        *lengths.get_mut(i).ok_or(Damaged)? = u8::try_from(length).map_err(|_| Damaged)?;
        i += 1;
        if Some(i) == zeros_after {
            // These lengths stay 0.
            i += bits.read(ZEROS_AFTER_BITS) as usize;
        }
    }
    if i > codes {
        return Err(Damaged);
    }
    Code::of(&lengths)
}

/// The token table, its lengths sent as codes of `lengths`, the 19-code table.
fn token_table(bits: &mut Bits, lengths: &Code) -> Result<Code, Damaged> {
    let n = bits.read(TOKEN_COUNT_BITS) as usize;
    if n == 0 {
        let only = bits.read(TOKEN_COUNT_BITS) as usize;
        return if only < TOKENS { Ok(Code::Only(only)) } else { Err(Damaged) };
    }
    if n > TOKENS {
        return Err(Damaged);
    }
    let mut table = vec![0u8; TOKENS];
    let mut i = 0;
    while i < n {
        let zeros = match lengths.decode(bits)? {
            ONE_ZERO => 1,
            SOME_ZEROS => bits.read(SOME_ZEROS_BITS) as usize + SOME_ZEROS_LEAST,
            MANY_ZEROS => bits.read(MANY_ZEROS_BITS) as usize + MANY_ZEROS_LEAST,
            code => {
                let length = code.checked_sub(LENGTH_CODE_OFFSET).ok_or(Damaged)?;
                *table.get_mut(i).ok_or(Damaged)? = u8::try_from(length).map_err(|_| Damaged)?;
                i += 1;
                continue;
            }
        };
        i += zeros;
        if i > TOKENS {
            return Err(Damaged);
        }
    }
    Code::of(&table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_testkit::machine_files::{Lzh, lzh, lzh_blocks};

    /// Text and stitch-like bytes, and their extremes: nothing, one byte, a long run of one byte.
    /// The third is skewed so that its tokens' code lengths run from 3 to 10 bits, their counts
    /// doubling: the 19-code table then sends lengths of 7 and more, in their long form.
    fn samples() -> Vec<Vec<u8>> {
        let text = b"a stitch, a stitch, a stitch and a jump; a stitch, a stitch and a trim".repeat(9);
        let deltas: Vec<u8> = (0..3000u32).map(|i| ((i * 37 % 23) as u8).wrapping_sub(11)).collect();
        let mut skewed: Vec<u8> = Vec::new();
        let mut value = 0u8;
        for length in 3..=10u32 {
            for _ in 0..1u32 << (length - 3) {
                skewed.extend(std::iter::repeat_n(value, 1 << (10 - length)));
                value = value.wrapping_add(1);
            }
        }
        let kinds: Vec<u8> = (0..2500u32).map(|i| if i % 97 == 0 { 0x81 } else { 0x80 }).collect();
        vec![text, deltas, skewed, kinds, vec![], vec![0x7F], vec![0x80; 5000], (0..=255u8).collect()]
    }

    /// Every stream written with or without copies, in one block or many, expands to its bytes,
    /// whole, and to fewer when fewer are asked for.
    #[test]
    fn streams_expand_to_their_bytes() {
        for data in samples() {
            for o in [Lzh { copies: true, block_tokens: 0 }, Lzh { copies: false, block_tokens: 0 }, Lzh { copies: true, block_tokens: 7 }] {
                let stream = lzh(&data, &o);
                assert_eq!(expand(&stream, data.len()), Ok(Expanded { bytes: data.clone(), whole: true }), "{o:?}, {} bytes", data.len());
                let half = data.len() / 2;
                assert_eq!(expand(&stream, half).map(|e| e.bytes), Ok(data[..half].to_vec()));
            }
        }
    }

    /// A stream that ends before the bytes asked for gives the bytes made, not whole; cut anywhere,
    /// inside a block's tables or a copy too, it gives a start of the stream's bytes, never fewer
    /// for a later cut and never a byte the stream doesn't hold, and cut in a later block's count
    /// or tables, exactly the bytes of the blocks before it.
    #[test]
    fn a_stream_cut_short_keeps_what_it_made() {
        let data = samples().remove(1);
        let ended = expand(&lzh(&data, &Lzh { copies: true, block_tokens: 300 }), data.len() + 10).unwrap();
        assert_eq!((ended.bytes.as_slice(), ended.whole), (data.as_slice(), false));
        let mut checked = 0;
        for o in [Lzh { copies: true, block_tokens: 300 }, Lzh { copies: false, block_tokens: 50 }, Lzh { copies: true, block_tokens: 7 }] {
            let (stream, starts) = lzh_blocks(&data, &o);
            // Cut anywhere from a later block's count to the end of its tables, its tokens not begun:
            // exactly the bytes of the blocks before it.
            let bits = u8::BITS as usize;
            for b in starts.iter().skip(1) {
                for cut in b.at.div_ceil(bits)..=b.tokens_at / bits {
                    let e = expand(&stream[..cut], data.len()).unwrap();
                    assert_eq!(e.bytes, data[..b.made], "{o:?}: cut at {cut}, in the block from bit {}", b.at);
                    assert_eq!(e.whole, b.made == data.len(), "{o:?}: cut at {cut}");
                    checked += 1;
                }
            }
            let mut before = 0;
            for cut in 0..=stream.len() {
                let e = expand(&stream[..cut], data.len()).unwrap_or_else(|_| panic!("{o:?}: cut at {cut} refused"));
                assert!(data.starts_with(&e.bytes), "{o:?}: cut at {cut}: only bytes the stream holds");
                assert!(e.bytes.len() >= before, "{o:?}: cut at {cut}: {} bytes, {before} before", e.bytes.len());
                assert_eq!(e.whole, e.bytes.len() == data.len(), "{o:?}: cut at {cut}");
                before = e.bytes.len();
            }
            assert_eq!(before, data.len(), "{o:?}: whole at the end");
        }
        assert!(checked > 0, "streams of several blocks");
    }

    /// A table that counts more codes than it has, one that is no complete code (too few codes or
    /// too many), a block of no tokens, a copy from before the start and one further back than the
    /// window are refused; random bytes give a stream, a cut one or a refusal, never a panic,
    /// quickly.
    #[test]
    fn damaged_streams_are_refused() {
        // A block of 1 token whose 19-code table says it has 20 codes.
        assert_eq!(expand(&[0x00, 0x01, 0b1010_0000, 0, 0, 0], 4), Err(Damaged));
        // A block of no tokens, its tables single values of no bits.
        assert_eq!(expand(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], 4), Err(Damaged));
        // A block of one token, the byte 'A' (tables of a single value, no bits a token), whose
        // distance table, never used, has one code of 1 bit (too few) or three (too many).
        assert_eq!(expand(&[0x00, 0x01, 0x00, 0x00, 0x04, 0x10, 0x90], 1), Err(Damaged));
        assert_eq!(expand(&[0x00, 0x01, 0x00, 0x00, 0x04, 0x11, 0x92, 0x40], 1), Err(Damaged));
        // The byte 'A', then a block of one copy of 3 bytes: from one byte back it expands, from
        // two bytes back it would start before the stream.
        let copy = |code: u8| [0x00, 0x01, 0x00, 0x00, 0x04, 0x10, 0x00, 0x00, 0x04, 0x00, 0x00, 0x40, 0x00, code << 4];
        assert_eq!(expand(&copy(0), 4), Ok(Expanded { bytes: b"AAAA".to_vec(), whole: true }));
        assert_eq!(expand(&copy(1), 4), Err(Damaged));
        // 1,025 bytes 'A', then a copy 1,025 bytes back: further than the window.
        assert_eq!(expand(&[0x04, 0x01, 0x00, 0x00, 0x04, 0x10, 0x00, 0x00, 0x04, 0x00, 0x00, 0x40, 0x00, 0xB0, 0x00], 1030), Err(Damaged));
        let started = std::time::Instant::now();
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for len in [1usize, 2, 7, 40, 300, 3000] {
            for _ in 0..200 {
                let bytes: Vec<u8> = (0..len)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        (state >> 24) as u8
                    })
                    .collect();
                let _ = expand(&bytes, 100_000);
            }
        }
        assert!(started.elapsed().as_secs() < 20);
    }

    /// A stream written by hand as `0` and `1` (spaces only part its fields), most significant bit
    /// first, the last byte padded with 0 bits: its bits are the layout as the module's doc gives
    /// it, written apart from the module's constants and the test kit's compressor.
    fn bits(s: &str) -> Vec<u8> {
        let ones: Vec<bool> = s
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| match c {
                '0' => false,
                '1' => true,
                other => panic!("not a bit: {other}"),
            })
            .collect();
        let width = u8::BITS as usize;
        ones.chunks(width).map(|c| c.iter().enumerate().fold(0u8, |b, (k, &one)| b | (u8::from(one) << (width - 1 - k)))).collect()
    }

    /// Streams written by hand expand as the layout says: the 19-code table's lengths and the
    /// 2-bit count after its third (no zeros in the first stream, one in the second, which leaves
    /// code 3 out: read its value wrong, or not at all, and the stream is refused), the token
    /// table's lengths sent as its codes (runs of 20 and more zeros, and code k for the length
    /// k − 2), a distance table that counts all 15 of its lengths, bytes, a copy from one byte back
    /// and the end.
    #[test]
    fn a_stream_written_by_hand_expands() {
        let stream = bits(concat!(
            "0000000000000011 ",                                                  // a block of 3 tokens
            "00101 000 000 001 00 010 010 ", // 19-code table: 5 lengths, 0 0 1, no zeros, 2 2: '0' 2, '10' 3, '11' 4
            "111111111 ",                    // token table: 511 lengths
            "0 000101101 ",                  // 20 + 45 zeros: tokens 0 to 64
            "10 ",                           // code 3, length 1: token 65, 'A'
            "0 010101010 ",                  // 20 + 170 zeros: 66 to 255
            "11 ",                           // code 4, length 2: token 256, a copy of 3 bytes
            "0 011101001 ",                  // 20 + 233 zeros: 257 to 509
            "11 ",                           // code 4, length 2: token 510, the end
            "01111 001 001 000 000 000 000 000 000 000 000 000 000 000 000 000 ", // distance table: 15 lengths
            "0 ",                            // 'A'
            "10 0 ",                         // a copy of 3, distance code 0: from one byte back
            "11",                            // the end
        ));
        assert_eq!(expand(&stream, 4), Ok(Expanded { bytes: b"AAAA".to_vec(), whole: true }));
        assert_eq!(expand(&stream, 10), Ok(Expanded { bytes: b"AAAA".to_vec(), whole: false }), "the end token");
        let stream = bits(concat!(
            "0000000000000100 ",                                                  // a block of 4 tokens
            "00101 000 000 001 01 001 ", // 19-code table: 5 lengths, 0 0 1, one zero (code 3), 1: '0' 2, '1' 4
            "111111111 ",                // token table: 511 lengths
            "0 000101101 ",              // 20 + 45 zeros: tokens 0 to 64
            "1 1 ",                      // code 4, length 2: tokens 65 and 66, 'A' and 'B'
            "0 010101001 ",              // 20 + 169 zeros: 67 to 255
            "1 ",                        // code 4, length 2: token 256, a copy of 3 bytes
            "0 011101001 ",              // 20 + 233 zeros: 257 to 509
            "1 ",                        // code 4, length 2: token 510, the end
            "01111 001 001 000 000 000 000 000 000 000 000 000 000 000 000 000 ", // distance table: 15 lengths
            "00 ",                       // 'A'
            "10 0 ",                     // a copy of 3, distance code 0: from one byte back
            "01 ",                       // 'B'
            "11",                        // the end
        ));
        assert_eq!(expand(&stream, 5), Ok(Expanded { bytes: b"AAAAB".to_vec(), whole: true }));
        assert_eq!(expand(&stream, 10), Ok(Expanded { bytes: b"AAAAB".to_vec(), whole: false }), "the end token");
    }

    /// A length of 16 bits, the longest code, is read in a table: a 19-code table of 17 codes, one
    /// of each length from 1 to 15 and two of 16, the long ones in their long form.
    #[test]
    fn a_code_of_sixteen_bits_is_read() {
        let stream = bits(concat!(
            "0000000000000010 ",                         // a block of 2 tokens
            "10001 ",                                    // 19-code table: 17 lengths
            "011 100 001 00 010 101 110 ",               // 3 4 1, no zeros after the third, 2 5 6: '0' 2, '10' 3
            "1110 11110 111110 1111110 11111110 ",       // 7 to 11
            "111111110 1111111110 11111111110 ",         // 12 to 14
            "111111111110 1111111111110 1111111111110 ", // 15, 16 and 16
            "111111111 0 000101101 10 0 110101000 10 ",  // token table: 65 zeros, 1, 444 zeros, 1: 'A' '0', the end '1'
            "00000 00000 ",                              // distance table: a single value
            "0 1",                                       // 'A', the end
        ));
        assert_eq!(expand(&stream, 4), Ok(Expanded { bytes: b"A".to_vec(), whole: false }));
    }

    /// The token table's single value, and a run of zero lengths, past its 511 codes are refused.
    /// Each stream ends there, so that a reader taking them would read on into no data and keep
    /// nothing rather than refuse the stream.
    #[test]
    fn a_table_past_its_values_is_refused() {
        // A block of 1 token; the 19-code table the single value 0; the token table token 511.
        assert_eq!(expand(&bits("0000000000000001 00000 00000 000000000 111111111"), 10), Err(Damaged));
        // A block of 1 token; the 19-code table: '0' 2, '1' 3; the token table: two lengths of 1,
        // then 20 + 511 zeros, past its 511 lengths.
        let zeros = bits("0000000000000001 00100 000 000 001 00 001 000000011 1 1 0 111111111");
        assert_eq!(expand(&zeros, 10), Err(Damaged));
    }
}
