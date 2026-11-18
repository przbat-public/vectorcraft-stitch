//! Machine files VectorCraft reads but never writes, built here from scratch for tests (VectorCraft
//! Stitch): Janome SEW ([`sew`]). The layout is the one the stitch crate's reader cites
//! (`format/sew.rs`); no file made by other software is used.

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
