//! Machine files VectorCraft reads but never writes, built here from scratch for tests (VectorCraft
//! Stitch): Husqvarna Viking HUS ([`hus`]), whose streams this module compresses itself ([`lzh`]),
//! and Janome SEW ([`sew`]). The layouts are the ones the stitch crate's readers cite
//! (`format/hus.rs`, `format/lzh.rs`, `format/sew.rs`); no file made by other software is used.
//!
//! The HUS compressor writes what the reader's method reads: tokens in blocks, each block's three
//! tables sent as code lengths. It is a test fixture, not a writer the app offers: it finds copies
//! greedily, takes code lengths from a Huffman tree (or, past 16 bits, a code of near-equal
//! lengths), and uses every form the method has (a table of one value, lengths of 7 and more, the
//! three runs of zero lengths, the zeros after the third length), so the reader's tests reach them.

/// HUS's compressed streams: the tokens' window, the codes' counts (`format/lzh.rs`).
const WINDOW: usize = 1024;
const TOKENS: usize = 511;
const FIRST_COPY: usize = 256;
const END: usize = 510;
const SHORTEST_COPY: usize = 3;
const LONGEST_COPY: usize = 256;
const LENGTH_CODES: usize = 19;
const DISTANCE_CODES: usize = 15;
const LONGEST_CODE: usize = 16;
/// The most tokens one block holds: its count is 16 bits.
const MOST_IN_A_BLOCK: usize = 65_535;
/// How many earlier places with the same three bytes a copy is looked for at: enough for tests.
const CANDIDATES: usize = 64;

/// How [`lzh`] compresses: with copies or bytes only, and the most tokens a block takes (0: as many
/// as a block holds).
#[derive(Clone, Copy, Debug)]
pub struct Lzh {
    pub copies: bool,
    pub block_tokens: usize,
}

impl Default for Lzh {
    fn default() -> Self {
        Self { copies: true, block_tokens: 0 }
    }
}

enum Token {
    Byte(u8),
    Copy { length: usize, distance: usize },
    End,
}

/// `data` as one HUS stream: its tokens, then the end token, in blocks of at most
/// [`Lzh::block_tokens`].
pub fn lzh(data: &[u8], o: &Lzh) -> Vec<u8> {
    let mut tokens = tokens(data, o.copies);
    tokens.push(Token::End);
    let per_block = if o.block_tokens == 0 { MOST_IN_A_BLOCK } else { o.block_tokens.min(MOST_IN_A_BLOCK) };
    let mut w = BitWriter::default();
    for block in tokens.chunks(per_block) {
        let mut token_freq = vec![0u64; TOKENS];
        let mut distance_freq = vec![0u64; DISTANCE_CODES];
        for t in block {
            match t {
                Token::Byte(b) => token_freq[usize::from(*b)] += 1,
                Token::Copy { length, distance } => {
                    token_freq[FIRST_COPY + length - SHORTEST_COPY] += 1;
                    distance_freq[distance_code(*distance).0] += 1;
                }
                Token::End => token_freq[END] += 1,
            }
        }
        let token_lengths = lengths(&token_freq);
        let distance_lengths = lengths(&distance_freq);
        w.put(16, block.len() as u32);
        match only(&token_freq) {
            // One token: the length table is a single value too, and nothing sends lengths.
            Some(t) => {
                w.put(5, 0);
                w.put(5, 0);
                w.put(9, 0);
                w.put(9, t as u32);
            }
            None => {
                let sent = token_lengths_sent(&token_lengths);
                let mut freq = vec![0u64; LENGTH_CODES];
                for (code, _, _) in &sent {
                    freq[*code] += 1;
                }
                let length_lengths = lengths(&freq);
                small_table(&mut w, &freq, &length_lengths, 5, Some(3));
                let length_codes = codes(&length_lengths);
                let n = token_lengths.iter().rposition(|l| *l > 0).map_or(0, |i| i + 1);
                w.put(9, n as u32);
                for (code, extra, extra_bits) in sent {
                    put_code(&mut w, &length_codes, &length_lengths, code);
                    w.put(extra_bits, extra);
                }
            }
        }
        small_table(&mut w, &distance_freq, &distance_lengths, 5, None);
        let (token_codes, distance_codes) = (codes(&token_lengths), codes(&distance_lengths));
        for t in block {
            match t {
                Token::Byte(b) => put_code(&mut w, &token_codes, &token_lengths, usize::from(*b)),
                Token::Copy { length, distance } => {
                    put_code(&mut w, &token_codes, &token_lengths, FIRST_COPY + length - SHORTEST_COPY);
                    let (code, extra, extra_bits) = distance_code(*distance);
                    put_code(&mut w, &distance_codes, &distance_lengths, code);
                    w.put(extra_bits, extra);
                }
                Token::End => put_code(&mut w, &token_codes, &token_lengths, END),
            }
        }
    }
    w.finish()
}

/// A HUS file: the header (its magic, the counts, the extents, where the streams start, `name` and
/// the colour list), then the kinds of record, the x moves and the y moves, each compressed with
/// `o`. A record is (kind, x, y): the kinds are 0x80 a stitch, 0x81 a jump, 0x82 a stop, 0x84 a
/// colour change, 0x88 a cut and 0x90 the end; x and y are moves in tenths of a millimetre, y
/// counting upwards.
pub fn hus(records: &[(u8, i8, i8)], colours: &[u16], name: &[u8; 8], o: &Lzh) -> Vec<u8> {
    let kinds: Vec<u8> = records.iter().map(|r| r.0).collect();
    let xs: Vec<u8> = records.iter().map(|r| r.1 as u8).collect();
    let ys: Vec<u8> = records.iter().map(|r| r.2 as u8).collect();
    let streams = [lzh(&kinds, o), lzh(&xs, o), lzh(&ys, o)];
    // The extents: how far the needle goes right, up, left and down from where it starts.
    let (mut x, mut y, mut extents) = (0i32, 0i32, [0i32; 4]);
    for r in records {
        x += i32::from(r.1);
        y += i32::from(r.2);
        extents = [extents[0].max(x), extents[1].max(y), extents[2].min(x), extents[3].min(y)];
    }
    let first = 42 + 2 * colours.len();
    let offsets = [first, first + streams[0].len(), first + streams[0].len() + streams[1].len()];
    let mut out = vec![0x5B, 0xAF, 0xC8, 0x00];
    out.extend_from_slice(&(records.len() as u32).to_le_bytes());
    out.extend_from_slice(&(colours.len() as u32).to_le_bytes());
    for e in extents {
        out.extend_from_slice(&(e as i16).to_le_bytes());
    }
    for at in offsets {
        out.extend_from_slice(&(at as u32).to_le_bytes());
    }
    out.extend_from_slice(name);
    out.extend_from_slice(&[0, 0]);
    for c in colours {
        out.extend_from_slice(&c.to_le_bytes());
    }
    for s in streams {
        out.extend(s);
    }
    out
}

/// Where a SEW file's stitch records start, after its header and previews (the KDE Community
/// Wiki's *Janome Embroidery Format*: offset 0x1D78).
pub const SEW_STITCHES_AT: usize = 0x1D78;

/// A SEW file: the colour count and `colours` (thread numbers of Janome's JEF palette, as many as
/// given, the table padded with zeros to its 12), blank previews, then `records`, JEF's stitch
/// records, from [`SEW_STITCHES_AT`].
pub fn sew(colours: &[u16], records: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; SEW_STITCHES_AT];
    out[..2].copy_from_slice(&(colours.len() as u16).to_le_bytes());
    for (k, c) in colours.iter().take(12).enumerate() {
        out[2 + 2 * k..4 + 2 * k].copy_from_slice(&c.to_le_bytes());
    }
    out.extend_from_slice(records);
    out
}

/// A SEW file holding a JEF file's records and its first 12 thread numbers (`None` for bytes that
/// aren't a JEF header).
pub fn sew_from_jef(jef: &[u8]) -> Option<Vec<u8>> {
    let word = |at: usize| jef.get(at..at + 4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
    let (start, count) = (word(0)? as usize, word(24)? as usize);
    let colours: Vec<u16> = (0..count.min(12)).map(|k| word(116 + 4 * k).map(|n| n as u16)).collect::<Option<_>>()?;
    Some(sew(&colours, jef.get(start..)?))
}

/// `data` as bytes and copies: at each place, the longest copy of at least three bytes the window
/// holds (when `copies`), else the byte.
fn tokens(data: &[u8], copies: bool) -> Vec<Token> {
    let mut out = Vec::new();
    let mut seen: std::collections::HashMap<[u8; 3], Vec<usize>> = std::collections::HashMap::new();
    let mut i = 0;
    while i < data.len() {
        let mut best: Option<(usize, usize)> = None;
        if copies && i + SHORTEST_COPY <= data.len() {
            let key = [data[i], data[i + 1], data[i + 2]];
            for &from in seen.get(&key).map(|v| v.as_slice()).unwrap_or_default().iter().rev().take(CANDIDATES) {
                if i - from > WINDOW {
                    break;
                }
                let length = (0..LONGEST_COPY.min(data.len() - i)).take_while(|&k| data[from + k] == data[i + k]).count();
                if length >= SHORTEST_COPY && best.is_none_or(|(l, _)| length > l) {
                    best = Some((length, i - from - 1));
                }
            }
        }
        let step = match best {
            Some((length, distance)) => {
                out.push(Token::Copy { length, distance });
                length
            }
            None => {
                out.push(Token::Byte(data[i]));
                1
            }
        };
        for k in i..(i + step) {
            if k + SHORTEST_COPY <= data.len() {
                seen.entry([data[k], data[k + 1], data[k + 2]]).or_default().push(k);
            }
        }
        i += step;
    }
    out
}

/// A copy's distance (bytes back, less one) as its code and extra bits: (code, extra, how many).
fn distance_code(distance: usize) -> (usize, u32, u32) {
    if distance == 0 {
        return (0, 0, 0);
    }
    let code = (usize::BITS - distance.leading_zeros()) as usize;
    let extra_bits = code as u32 - 1;
    (code, (distance - (1 << extra_bits)) as u32, extra_bits)
}

/// The one value `freq` counts, if it counts one.
fn only(freq: &[u64]) -> Option<usize> {
    let mut used = freq.iter().enumerate().filter(|(_, f)| **f > 0);
    let first = used.next()?.0;
    used.next().is_none().then_some(first)
}

/// Code lengths for `freq`: a Huffman tree's, or, when one would pass [`LONGEST_CODE`], lengths as
/// near equal as a complete code allows. A single value used gets length 1 (sent as a table of one
/// value instead); none used, no lengths.
fn lengths(freq: &[u64]) -> Vec<u8> {
    let used: Vec<usize> = (0..freq.len()).filter(|&v| freq[v] > 0).collect();
    let mut out = vec![0u8; freq.len()];
    match used.len() {
        0 => return out,
        1 => {
            out[used[0]] = 1;
            return out;
        }
        _ => {}
    }
    // A Huffman tree: each node's weight and children; leaves are values.
    let mut nodes: Vec<(u64, Option<(usize, usize)>)> = used.iter().map(|&v| (freq[v], None)).collect();
    let mut live: Vec<usize> = (0..nodes.len()).collect();
    while live.len() > 1 {
        live.sort_by_key(|&n| std::cmp::Reverse((nodes[n].0, n)));
        let (a, b) = (live.pop().unwrap(), live.pop().unwrap());
        nodes.push((nodes[a].0 + nodes[b].0, Some((a, b))));
        live.push(nodes.len() - 1);
    }
    let mut depth = vec![0usize; nodes.len()];
    for n in (0..nodes.len()).rev() {
        if let Some((a, b)) = nodes[n].1 {
            depth[a] = depth[n] + 1;
            depth[b] = depth[n] + 1;
        }
    }
    if depth[..used.len()].iter().any(|d| *d > LONGEST_CODE) {
        // Near-equal lengths: k values one shorter, the rest at the full length, a complete code.
        let full = (usize::BITS - (used.len() - 1).leading_zeros()) as usize;
        let shorter = (1 << full) - used.len();
        for (k, &v) in used.iter().enumerate() {
            out[v] = if k < shorter { full - 1 } else { full } as u8;
        }
        return out;
    }
    for (k, &v) in used.iter().enumerate() {
        out[v] = depth[k] as u8;
    }
    out
}

/// The canonical codes of `lengths`: shorter first, values of one length in order.
fn codes(lengths: &[u8]) -> Vec<u32> {
    let mut out = vec![0u32; lengths.len()];
    let mut code = 0u32;
    for len in 1..=LONGEST_CODE as u8 {
        for v in 0..lengths.len() {
            if lengths[v] == len {
                out[v] = code;
                code += 1;
            }
        }
        code <<= 1;
    }
    out
}

fn put_code(w: &mut BitWriter, codes: &[u32], lengths: &[u8], v: usize) {
    if lengths.iter().filter(|l| **l > 0).count() > 1 {
        w.put(u32::from(lengths[v]), codes[v]);
    }
}

/// The 19-code table or the distance table (`zeros_after`: the 19-code table's run of zeros after
/// its third length): a single value when one is used, else its lengths.
fn small_table(w: &mut BitWriter, freq: &[u64], lengths: &[u8], count_bits: u32, zeros_after: Option<usize>) {
    if let Some(v) = only(freq).or(freq.iter().all(|f| *f == 0).then_some(0)) {
        w.put(count_bits, 0);
        w.put(count_bits, v as u32);
        return;
    }
    let n = lengths.iter().rposition(|l| *l > 0).map_or(0, |i| i + 1);
    w.put(count_bits, n as u32);
    let mut i = 0;
    while i < n {
        let k = u32::from(lengths[i]);
        if k <= 6 {
            w.put(3, k);
        } else {
            w.put(k - 3, (1 << (k - 3)) - 2);
        }
        i += 1;
        if Some(i) == zeros_after {
            let mut zeros = 0;
            while zeros < 3 && i + zeros < n && lengths[i + zeros] == 0 {
                zeros += 1;
            }
            w.put(2, zeros as u32);
            i += zeros;
        }
    }
}

/// The token table's lengths as codes of the 19-code table, with their extra bits: (code, extra,
/// how many extra bits).
fn token_lengths_sent(lengths: &[u8]) -> Vec<(usize, u32, u32)> {
    let n = lengths.iter().rposition(|l| *l > 0).map_or(0, |i| i + 1);
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if lengths[i] > 0 {
            out.push((usize::from(lengths[i]) + 2, 0, 0));
            i += 1;
            continue;
        }
        let run = lengths[i..n].iter().take_while(|l| **l == 0).count();
        match run {
            1 | 2 => out.extend(std::iter::repeat_n((0, 0, 0), run)),
            3..=18 => out.push((1, run as u32 - 3, 4)),
            19 => {
                out.push((0, 0, 0));
                out.push((1, 15, 4));
            }
            _ => out.push((2, run as u32 - 20, 9)),
        }
        i += run;
    }
    out
}

/// Bits written most significant first, the last byte padded with zeros.
#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    byte: u8,
    used: u32,
}

impl BitWriter {
    fn put(&mut self, n: u32, v: u32) {
        for k in (0..n).rev() {
            self.byte = (self.byte << 1) | ((v >> k) & 1) as u8;
            self.used += 1;
            if self.used == 8 {
                self.out.push(self.byte);
                (self.byte, self.used) = (0, 0);
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.out.push(self.byte << (8 - self.used));
        }
        self.out
    }
}
