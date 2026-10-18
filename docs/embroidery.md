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
| `density` | 0.4 mm | distance between neighbouring threads: fill rows, satin crossings |
| `stitchLength` | 4 mm in fills, 2.5 mm in running lines | the longest stitch of a running line or a fill row |
| `stitchAngle` | 45° | the direction a fill's rows run |
| `pullCompensation` | 0.2 mm | how much wider a satin column is stitched so the fabric's pull leaves no gap |
| `underlay` | `true` | an edge run under fills, a centre run under satin columns |
| `jumpLength` | 5 mm | a longer move cuts the thread and jumps |
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
stays down (`jumpLength` or less) and is just the next stitch; a longer one becomes a trim and a
jump. Each colour block is one thread. Satin and fill tie the thread in after every cut (and at the
start) and off before every cut (and at the end): a short stitch back and forth on the spot.

- **Running** follows a path at `stitchLength` steps, with a lock stitch (a short stitch back
  along the line and forward again, never past its end) at each end so the thread does not pull
  out. Sharp corners (turns over about 30°) get a needle point of their own, so the stitches follow
  the outline instead of cutting across a corner; the same holds for underlay.
- **Satin** sews a column: the path is its centre line, its stroke width the column's width, and
  the needle crosses from rail to rail every `density` millimetres, so the threads lie side by
  side. `pullCompensation` widens the column. The underlay holds the column up and follows its
  width: a centre run under columns narrower than 2 mm, an edge run (along one side and back along
  the other, kept inside the column) from 2 mm, and an edge run then a zigzag between its sides
  from 4.5 mm (estimates from the usual practice). The top stitching starts where the underlay
  ended, so the needle never travels back along the column. A
  crossing longer than 10 mm (an estimate; professional software makes it a setting) is split
  into shorter stitches (*auto split*), and the cuts move from one crossing to the next so they
  never line up into a seam down the column.
- **Tatami fill** fills a shape with rows `density` apart at `stitchAngle` degrees, alternating
  their direction and staggering their penetrations a quarter stitch at a time over a four-row
  cycle (so no two rows line up into a groove). Each row is split into stitches no longer than
  `stitchLength`, and its last stitch is left out when it is short, so the needle turns
  diagonally into the next row instead of crawling along the edge. Rows sit half a spacing inside
  the shape's extent: no row lies on a tangent, and the thread's width covers to the edge. An edge
  run underneath holds the fabric, and a fill at least 10 mm across both ways also gets a second
  underlay: sparse tatami rows (2 mm apart, 3 mm stitches) across the top rows, kept 0.6 mm inside
  the edge so they never peek out (estimates). Holes are left unstitched: the shape's fill rule decides what
  is inside, as it does for paint. The rows are grouped into *regions* where the shape does not
  branch (above a hole, each side of it, below it; each arm of a letter), and each region is sewn
  whole before the next. The next region is the one that starts nearest where the last one ended,
  from whichever end and side is closer. The needle gets there by a *travel run*: stitches
  2 mm long inside the shape, straight when that stays inside, else along the edge of the outline
  or hole the two ends lie on, 0.5 mm inside it, the shorter way round. Only when no such way exists
  (two separate shapes) or it is longer than 25 mm (an estimate) is the thread trimmed, and a move
  that would leave the shape or cross a hole is never sewn straight, so no thread floats over the
  fabric. Each region also takes one row of its neighbour's where they meet, cut to the stretch
  they share, so the fabric's pull opens no gap along the seam. A 30 mm ring with a hole is sewn
  in one go, without a trim.

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

## Validation

- `crates/stitch`: 27 unit tests — the fill covers a rectangle and leaves its hole empty, a ring
  is sewn in regions with a handful of trims and no thread across its hole, a narrow gap is not
  bridged, no stitch is longer than asked, the angle turns the rows, running lines keep their
  corners and lock without passing their ends, fills and satins tie in and off, satin crossings
  stay within the column, pull compensation widens it, an absurd plan is refused before it is
  made, the DST record round-trips every delta from -121 to 121, only a run of three jumps reads as
  a DST trim, a DST colour change can carry a move, each format writes a design it reads back with
  the same blocks and size, a JEF's header extents are where its stitches go, an absurd JEF colour
  count is not believed, and no reader panics on random, truncated or damaged bytes.
- `crates/engine`: 17 tests (16 command-level) — export writes a readable design in every format,
  options change the plan, a written PES opens as art (also by its magic bytes), a design read back
  sews again with the same size and blocks, `document.formats` lists the four, empty documents,
  bad styles and absurd densities are refused, type is left out with a warning, the stacking order
  survives grouping by thread, clipping masks cut fills and strokes, template layers are not
  sewn, a forced fill fills a stroke's area rather than its path's inside, and the defaults
  `document.formats` lists are the planner's.
- `crates/engine/tests/import_fuzz.rs`: damaged designs and random records behind each format's
  header, opened and placed, never panic.

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
- `crates/stitch/src/plan.rs` — running, satin and tatami fill, underlay, trims.
- `crates/stitch/src/format/` — `mod.rs` (shared frames, readers, block building), `dst.rs`,
  `exp.rs`, `pes.rs`, `jef.rs`.
- `crates/stitch/src/thread.rs` — the colour palettes and nearest-colour matching.
- `crates/engine/src/cmd/fileio/embroidery.rs` — the commands' side: opening a design as art,
  digitizing the document's art, the export options.
