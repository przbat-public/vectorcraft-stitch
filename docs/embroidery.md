# Embroidery

> **Last reviewed:** 2026-10-19 · **Last updated:** 2026-10-19 · **Change:** new (stitch planning and the DST, EXP, PES and JEF machine formats)

VectorCraft turns artwork into machine embroidery and opens embroidery designs as art. The work
lives in the `vectorcraft-stitch` crate (`crates/stitch`): stitch planning from geometry, the
machine formats, and the thread palettes the formats name colours from. The engine adds the
commands: `document.open` reads a design, `document.export` writes one.

Everything is implemented from public format documentation — the formats were reverse-engineered
publicly long before this project (see [References](#references)); no proprietary software was
observed, and no GPL implementation was read or copied.

## Using it

**Writing a design.** Draw the artwork as usual, select nothing (or the art you want) and export:

```json
{"id": 1, "method": "engine.execute", "params": {"command": "document.export", "params": {"format": "pes", "density": 0.4, "stitchAngle": 45}}}
```

`document.export` writes the whole document's visible art unless `selectedOnly` is given. The file
goes to `path`, or comes back as `dataBase64` when no path is given (that is how the tests and the
control channel use it). Every writable format's options are listed by `document.formats`.

| Option | Default | Meaning |
|---|---|---|
| `stitchStyle` | `auto` | `auto` fills fills, satins strokes 1 mm and wider, runs narrower ones; `fill`, `satin` and `running` force one style (`fill` on a stroke fills the area the stroke paints, at its width, caps and joins) |
| `density` | 0.4 mm in fills; satin by width | distance between neighbouring threads: fill rows, satin crossings. Left out, a satin column picks it by its width (*auto spacing*: 0.5 mm under 2 mm wide, 0.45 under 4, 0.4 under 6, 0.37 wider; estimates); given, it is used as it is |
| `stitchLength` | 4 mm in fills, 2.5 mm in running lines | the longest stitch of a running line or a fill row |
| `stitchAngle` | 45° | the direction a fill's rows run |
| `pullCompensation` | 0.2 mm | how much wider a satin column is stitched so the fabric's pull leaves no gap |
| `underlay` | `true` | an edge run under fills, a centre run under satin columns |
| `jumpLength` | 5 mm | a longer move cuts the thread and jumps; a shorter move that can't be sewn (between separate pieces) jumps with the thread on |
| `minStitch` | 0.4 mm | shorter stitches are left out |

An option left out keeps the style's own value. The defaults are the stitch planner's
(`plan::Params::for_style`); `document.formats` lists the same ones, and a test keeps them equal.

**Reading a design.** Open the file as a document; `File → Open` lists the formats. Each colour
block becomes a group named after its thread, and each run of stitches a thin round-capped line in
the thread's colour, so the design looks like it will sew and can be measured, traced over or
recoloured. A DST or EXP file carries no colours: those blocks get the default palette's colours,
one per block. `design.pes` opens by its `#PES` bytes whatever it is called.

**Exporting what you read.** The lines of an opened design are its stitches: each one is marked
as a run of literal stitches (`NodeExtras::embroidery`, kept in `.vectorcraft` files), and the
export sews its points exactly as they are, one needle point per point, with the jumps and trims
the file had between runs. The runs of one thread group stay one colour block, and the export
options (`stitchStyle`, `density`…) don't touch them. So a design opened and written again, in the
same format or another, has every needle point, trim and colour block where the file had it (see
`an_opened_design_is_written_back_unchanged` in `crates/engine/src/cmd/fileio/tests_embroidery.rs`).
Moving or scaling the lines moves or scales the stitches; editing a line's points edits the
stitches. Turning a design back into shapes that are stitched by the rules (what professional
software calls recognising objects) is not done yet.

## How a shape becomes stitches

A *run* is a stretch sewn without lifting the needle. Runs are joined into a block: a short move
inside the shape (`jumpLength` or less) is just the next stitch; a move that can't be sewn (it
would leave the shape or cross a hole) jumps, with the thread on when it is short and cut first
when it is longer than `jumpLength`, as professional software leaves short connectors as jumps.
Each colour block is one thread. Satin and fill tie the thread in after every cut (and at the
start) and off before every cut (and at the end): a short stitch back and forth on the spot. A
stitch shorter than `minStitch` is left out, as professional software removes them: it can break
the thread or cut the fabric.

- **Running** follows a path at `stitchLength` steps, with a lock stitch (a short stitch back
  along the line and forward again, never past its end) at each end so the thread does not pull
  out. Sharp corners (turns over about 30°) get a needle point of their own, so the stitches follow
  the outline instead of cutting across a corner; the same holds for underlay.
- **Satin** sews a column: the path is its centre line, its stroke width the column's width, and
  the needle crosses from rail to rail at the column's spacing, so the threads lie side by side:
  more open in a narrow column, where the thread bunches at the rails, denser in a wide one so it
  covers (or `density`, when given). `pullCompensation` widens the column. The underlay holds the
  column up and follows its width: a centre run under columns narrower than 2 mm, an edge run (along one side and back along
  the other, kept inside the column) from 2 mm, and an edge run then a zigzag between its sides
  from 4.5 mm (estimates from the usual practice). The top stitching starts where the underlay
  ended, so the needle never travels back along the column. A crossing longer than 10 mm (an estimate; professional software makes it a setting) is split
  into shorter stitches (*auto split*), and the cuts move from one crossing to the next so they
  never line up into a seam down the column.
- **Tatami fill** fills a shape with rows `density` apart at `stitchAngle` degrees, alternating
  their direction. A shape drawn without a final close is still filled, as paint fills it; only a
  subpath with no area is sewn as a line. The penetrations sit on a grid every `stitchLength`
  along the rows' direction, shifted a quarter stitch from one row to the next over a four-row
  cycle, so neighbouring rows — in any region — form one stagger pattern and never line up into a
  groove; no stitch is longer than `stitchLength` (an end stitch at most `minStitch` more), and
  only a row's first and last stitches are shorter. A row's last stitch is left out when it is
  short, so the needle turns diagonally into the next row instead of crawling along the edge; a
  long turn (where the edge runs nearly along the rows) is sewn as short travel stitches rather
  than one loose stitch. Rows sit half a spacing inside the shape's extent: no row lies on a
  tangent, and the thread's width covers to the edge. Holes are left unstitched: the shape's fill
  rule decides what is inside, as it does for paint.

  Underneath, an edge run along each outline holds the fabric, and a fill at least 10 mm across
  both ways also gets a second underlay: sparse tatami rows (2 mm apart, 3 mm stitches) across
  the top rows, kept 0.6 mm inside the edge so they never peek out (estimates).

  The rows are grouped into *regions* where the shape does not branch (above a hole, each side of
  it, below it; each arm of a letter), and each region is sewn whole before the next: the one that
  starts nearest where the last one ended, from whichever end and side is closer. Each region also
  takes one row of its neighbour's where they meet, cut to the stretch they share, so the fabric's
  pull opens no gap along the seam. The needle gets from one region (or outline) to the next by a
  *travel run*: 2 mm stitches along the shortest way that stays inside the shape, found on a
  visibility graph of the outlines' corners, each passed 0.5 mm inside (as Ink/Stitch's
  "underpath" does). Travels under the top stitching (in the underlay) may be any length, since the
  top stitching hides them; in the top stitching they may be at most 25 mm (an estimate), and a
  longer or impossible way (two separate shapes) lifts the needle instead. A 30 mm ring with a hole
  and a puzzle piece are sewn without a trim.

Blocks are sewn in stacking order, back to front, because thread sewn later lies on top of thread
sewn earlier, as paint does. An object joins an earlier block of its thread only when it overlaps
none of the blocks sewn between the two, so the machine changes colour as few times as the stacking
allows rather than once per object. Each block starts with the needle up and a jump to its first
stitch.

What is sewn is what would print: hidden objects, guides, template layers and non-printing layers
are left out. A clip group sews its clipped art only inside the clipping path: a filled area is cut
to it exactly before it is planned, and a stroke keeps the stitches whose needle points fall inside
(so a running line can stop up to a stitch short of the edge). The clipping path's own fill is sewn
under the clipped art and its stroke over it, unclipped, as they paint.

A shape that would need more than 4 million stitches, or a design of more than 2 million, is refused
with a message rather than planned (a tiny stitch length or spacing over large art); real designs
run to a few hundred thousand.

## The formats

| Format | Machine | What it holds | What we write |
|---|---|---|---|
| Tajima DST | commercial, the de-facto standard | 512-byte text header, 3-byte records of ±121 tenths of a millimetre (12.1 mm), no colours, trims implied by runs of three or more jumps | header with counts and extents, records, a colour-change record per block, trims as three small jumps that go nowhere |
| Melco EXP | commercial | records only, no header, no colours: 2-byte deltas up to ±12.7 mm, `0x80`-prefixed commands | stitches, jumps, trims, colour changes; the file ends with the last record |
| Brother PES | home machines (Brother, Bernina) | a PES section for PE-Design and a PEC section the machine reads | a minimal version 1 PES header pointing at a complete PEC block: 512-byte header, palette indices, a 20-byte block header with the two origin words real Brother files carry, the stitch stream, blank preview pictures |
| Janome JEF | home machines (Janome) | 116-byte header plus the thread list, then 2-byte records | header with counts, hoop, size and margins, Janome palette indices, records, three zero-length jumps per trim |

Coordinate conventions: files count tenths of a millimetre. DST, EXP and JEF count y upwards and
PEC downwards; the readers and writers flip accordingly, so a design written and read back keeps
its shape. Our DST, EXP and PEC files start the needle at a corner of the design; JEF files
start it at the design's centre, which Janome machines put at the hoop's centre, and the JEF
header's extents and hoop margins are measured from there.

Known limits of the formats themselves, and so of us: DST holds no colour, PEC holds no vector
shapes, JEF's stitches jump at most 12.7 mm per record, and every format names threads by palette
index, whose RGB values differ between published charts — preview colour is not truth.

## What is not stitched

- Type, images and live art (blends, envelopes, meshes, repeats) are left out: they would need
  their outlines. An export says so in its `warnings`, one line per kind.
- Gradients and patterns sew in one colour (a gradient in its first stop's; a pattern is left out).
- Wide satin columns are split stitch by stitch (auto split), not into several narrower columns.
- Underlay is chosen by width and size only, not by fabric (no fabric presets yet).
- The PES section of a `.pes` is the minimal version 1 header: machines sew the design correctly,
  but PE-Design shows it as a blank document because the editable shapes (CEmbOne/CSewSeg) are not
  written yet. Reading a PES reads its PEC block, which is what machines and every other app use.
- No stitch preview layer and no stitch simulator yet; the imported design is the preview.
- Separate pieces of one colour (letters, seeds) farther apart than `jumpLength` are trimmed
  between; a person digitizing the same artwork often leaves them as jumps (see Real designs).
- The order of a fill's regions only looks at the next start, so a branched shape can need a
  travel over stitching already sewn, longer than a visible travel may be, and is then cut (a
  letter E with vertical rows).
- On a satin's inner side, a tight curve bunches the penetrations (no short stitches yet).
- The default spacing and stitch length are more open than Ink/Stitch's: VectorCraft sews about
  40–65 % as many stitches for the same artwork.

## Validation

- `crates/stitch`: 36 unit tests — the fill covers a rectangle and leaves its hole empty, an
  unclosed shape is filled, a ring is sewn in regions with no thread across its hole and,
  travelling inside, without a trim, regions overlap by a row, a narrow gap and a letter E's gaps
  are never sewn across, separate pieces jump when close and trim when far, large fills get a
  tatami underlay and small ones don't, no stitch is longer than asked or shorter than the
  shortest, the angle turns the rows, running lines keep their corners and lock without passing
  their ends, fills and satins tie in and off, satin underlay follows the width with no trim
  between it and the top, satin spacing follows the width unless given, wide satins are split
  without a seam, satin crossings stay within the column, pull compensation widens it, an absurd
  plan is refused before it is made, the DST record round-trips every delta from -121 to 121,
  only a run of three jumps reads as a DST trim, a DST colour change can carry a move, each format
  writes a design it reads back with the same blocks and size, a JEF's header extents are where
  its stitches go, an absurd JEF colour count is not believed, and no reader panics on random,
  truncated or damaged bytes.
- `crates/engine`: 21 tests (20 command-level) — export writes a readable design in every format,
  options change the plan, a written PES opens as art (also by its magic bytes), an opened design
  is written back unchanged in every format (and again after a second round), with the cuts at
  its blocks' ends, converts between formats with the same needle points, ignores the style
  options and survives the native format, `document.formats` lists the four, empty documents,
  bad styles and absurd densities are refused, type is left out with a warning, the stacking
  order survives grouping by thread, clipping masks cut fills and strokes, template layers are not
  sewn, a forced fill fills a stroke's area rather than its path's inside, and the defaults
  `document.formats` lists (the satin spacing table included) are the planner's.
- `crates/engine/tests/import_fuzz.rs`: damaged designs and random records behind each format's
  header, opened and placed, never panic.

**Real designs** (`crates/engine/tests/embroidery_corpus.rs`): `cargo xtask corpus --embroidery`
fetches eleven designs of the [Emoji Embroidery Project](https://edutechwiki.unige.ch/en/Emoji_Embroidery_Project)
(University of Geneva and Arbores Tech; machine files CC BY-SA 4.0, artwork from Twemoji, CC BY
4.0, and Fluent Emoji, MIT) into the gitignored `corpus/embroidery/`, sha256-verified: for each, the
flat artwork and the PES file a person digitized from it with Ink/Stitch (the fox also as JEF). They
are used locally, never committed. The test checks that:

- every machine file reads at the size its own header states, and the fox reads the same from PES
  and JEF;
- every machine file, opened and exported again, keeps every needle point, trim and colour block
  (this found that the cuts Ink/Stitch writes at a block's start and end were lost);
- each artwork, digitized at the size of the person's design, comes out the same size, with no
  stitch longer than 10 mm, at most two shorter than 0.3 mm, no more trims than its ceiling and
  30–80 % of the person's stitch count.

When the test landed (VectorCraft's defaults; the person's design in brackets):

| Design | Stitches | Trims |
|---|---|---|
| bullseye | 3574 (8133) | 1 (6) |
| fox | 2176 (5268) | 5 (6) |
| heart suit | 1338 (3429) | 0 (0) |
| joystick | 1657 (3148) | 5 (4) |
| pool 8 ball | 2322 (5214) | 0 (2) |
| puzzle piece | 1352 (2988) | 1 (0) |
| smiling face | 2580 (5733) | 3 (3) |
| SOS button | 2596 (5849) | 8 (3) |
| video game | 2162 (4496) | 8 (11) |
| watermelon | 1522 (2598) | 13 (2) |
| zebra | 3455 (8861) | 11 (21) |

The comparison also found and fixed: a shape drawn without a final close sewn as an outline (the
heart: 42 stitches instead of a fill), fill stitches up to 1.5 times `stitchLength` (the old
stagger), tiny stitches at the ends of resampled lines, long loose row turns, and trims where a
travel along the outline was too long but a way through the shape was short (the puzzle piece:
6 trims, now 1).

**Cross-checked with pyembroidery** (MIT), used as an independent implementation in a throwaway
environment, never in the repo:

- A three-colour demo design written by us (a 60 × 40 mm fill, a satin curve and a 15 mm circle)
  reads in pyembroidery in all four formats with the same 2320 stitches per thread and the same
  size (100 × 56.6 mm). DST and EXP carry no colours there, as they should; PES and JEF name three
  threads.
- Designs written by pyembroidery read in our readers with pyembroidery's own stitch positions
  and bounds: DST, JEF and PES. That check found and fixed a real bug — PEC's short form is a
  7-bit two's-complement number, so a delta of -1 to -64 (bytes 64–127) decoded as +64 to +127 —
  and added the regression test `negative_short_deltas_keep_their_sign`.
- pyembroidery writes PEC stitches from byte 528 (no origin words) and real Brother files from
  532; the reader decides by the `0x90` origin high byte and checks that the stream ends exactly
  where the graphics offset says the preview pictures begin.
- pyembroidery does not parse the PES block at all, so it validates our PEC block and not the PES
  header; the PES section is the minimal version 1 header (see above).

Still unverified, and listed in the local research notes (`plan/research/formats-pes-jef.md`
§Appendix B): behaviour on real hardware, blank preview pictures, JEF's version letter, and how
machines treat our trim encodings.

## References

Public documentation, cited per claim in the crate and in the local research notes:

- EduTechWiki, [Embroidery format DST](https://edutechwiki.unige.ch/en/Embroidery_format_DST),
  [EXP](https://edutechwiki.unige.ch/en/Embroidery_format_EXP),
  [PES](https://edutechwiki.unige.ch/en/Embroidery_format_PES) and
  [PEC](https://edutechwiki.unige.ch/en/Embroidery_format_PEC) — the header and record layouts,
  the DST bit decoding and the PEC stitch encoding.
- [pyembroidery](https://github.com/EmbroidePy/pyembroidery) (MIT), consulted for terminology and
  the read/write conventions (y direction, trim encoding); no code copied.
- Ink/Stitch documentation (GPL-3.0 project, documentation read only) and vendor manuals
  (Wilcom/Hatch, ZSK, Madeira, Brother, Janome) for densities, stitch lengths, pull compensation
  and underlay — summarised in the local research notes with a source per number.
- Thread charts: Brother's 64-thread PEC palette and Janome's 78-thread JEF palette, as published
  by those sources; the tables are in `crates/stitch/src/thread.rs`.

## Files

- `crates/stitch/src/lib.rs` — the model: units, commands, stitches, threads, blocks, patterns.
- `crates/stitch/src/plan/` — stitch planning: `mod.rs` (styles, parameters, entry points),
  `geometry.rs` (polylines, spacing points, the rows' axes), `area.rs` (inside a filled shape, row
  spans, travel runs), `runs.rs` (joining runs: jumps, trims, ties, short stitches), `running.rs`,
  `satin.rs`, `fill.rs`, `tests.rs`.
- `crates/stitch/src/format/` — `mod.rs` (shared frames, readers, block building), `dst.rs`,
  `exp.rs`, `pes.rs`, `jef.rs`.
- `crates/stitch/src/thread.rs` — the colour palettes and nearest-colour matching.
- `crates/engine/src/cmd/fileio/embroidery/` — the commands' side: `mod.rs` (export options and
  entry point), `import.rs` (a design opened as art), `digitize.rs` (the document's art planned as
  stitches), `sequence.rs` (the order blocks are sewn in).
- `crates/engine/tests/embroidery_corpus.rs` and `xtask/src/embroidery_corpus.rs` — the real
  designs and the test that uses them.
