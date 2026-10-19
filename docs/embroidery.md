# Embroidery

> **Last reviewed:** 2026-10-19 · **Last updated:** 2026-10-19 · **Change:** pull compensation for fills (rows reach past the edge) and per side or by width for satins, push compensation at satin ends, `expand` to grow fills so neighbours meet, spacing by thread weight; a satin's last crossing goes across, and a move along two edges in line no longer crosses the gap between them

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
| `density` | 0.38 mm in fills; satin by width | distance between neighbouring threads: fill rows, satin crossings. Left out, a satin column picks it by its width (*auto spacing*: 0.5 mm under 2 mm wide, 0.45 under 4, 0.4 under 6, 0.37 wider; estimates), and both follow `threadWeight`; given, it is used as it is |
| `stitchLength` | 4 mm in fills, 2.5 mm in running lines | the longest stitch of a running line or a fill row |
| `stitchAngle` | 45° | the direction a fill's rows run |
| `pullCompensation` | 0.2 mm | how much farther the stitches reach at each side so the fabric's pull leaves no gap: each rail of a satin column, each end of a fill's rows. One number for both sides, or two: `[left, right]` of a satin as its path runs, `[start, end]` of a fill's rows along their direction |
| `pullCompensationPercent` | 0 | pull compensation that grows with the stitch, on top of `pullCompensation`: percent of a satin column's width, or of a fill row's length, added at each side (at most 2 mm a side with the fixed value). One number or two, as `pullCompensation` |
| `pushCompensation` | 0 mm | how much shorter an open satin column is sewn at each end, since sewing pushes the fabric out there; one number or two (`[start, end]` as the path is drawn). A column keeps at least half its length |
| `expand` | 0 mm | how far a filled shape grows all round before it is stitched, so that neighbouring shapes drawn apart meet; negative shrinks it; −2 to 2 |
| `threadWeight` | 40 | the thread's weight (wt: kilometres per kilogram, so a larger number is a finer thread): the default spacing of fills and satin columns follows its thickness (fill rows 0.41 mm apart for 30 wt, 0.34 for 60); 10 to 150 |
| `underlay` | `true` | an edge run under fills, a centre run under satin columns |
| `jumpLength` | 5 mm | a longer move lifts the needle (a jump), tied off before and in after so that it can be cut; a shorter move that can't be sewn (between separate pieces) jumps with the thread on |
| `trimLength` | 10 mm | a jump longer than this cuts the thread first (a trim); shorter jumps are left for the machine or a person to cut. Never less than `jumpLength`: a smaller value acts as `jumpLength` |
| `minStitch` | 0.4 mm | shorter stitches are left out |

An option left out keeps the style's own value. The defaults are the stitch planner's
(`plan::Params::for_style`, or `plan::Params::for_thread` for another thread weight);
`document.formats` lists the same ones, and a test keeps them equal. Compensation and the thread
weight are one value for the whole design for now: settings per object come with embroidery
properties on objects.

**Spacing by thread weight.** The default spacings are for 40 wt thread, standard embroidery
thread. Thread weight counts kilometres of thread per kilogram, so a thread's cross-section is
inversely proportional to it and its thickness to its square root; the spacing follows the square
root of the thickness, `(40 / weight)^¼` (half as fast as the thickness: a sewn thread flattens and
spreads). That moves a spacing by a few hundredths of a millimetre from one class of thread to the
next, the size of the offsets Wilcom's satin auto spacing applies by thread thickness: fill rows
0.41 mm apart for 30 wt, 0.38 for 40, 0.34 for 60. The exponent is an estimate, not a published
value.

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
Put inside a clip group, the lines are cut to the clipping path as any stroke is, and the needle
lifts across each cut the same way (tied off and in around a long jump).
Moving or scaling the lines moves or scales the stitches; editing a line's points edits the
stitches. Turning a design back into shapes that are stitched by the rules (what professional
software calls recognising objects) is not done yet.

**Cutting the thread after an object.** `embroidery.setTrimAfter` (`{on: true, ids?}`, the
selection by default) marks objects or groups whose thread is cut after their last stitch, however
near the next object starts, as Ink/Stitch's and Wilcom's "Trim After" do. Unmarked, only a jump
longer than `trimLength` is cut. The mark is saved in `.vectorcraft` files
(`NodeExtras::embroidery`). No menu or panel sets it yet: for now it is run through MCP or the
control channel (the command palette lists it, but runs it without `on`, which it refuses).

## How a shape becomes stitches

A *run* is a stretch sewn without lifting the needle. Runs are joined into a block: a short move
inside the shape (`jumpLength` or less) is just the next stitch; a move that can't be sewn (it
would leave the shape or cross a hole) jumps. A short jump keeps the thread on (it leaves no tail
worth cutting). One longer than `jumpLength` is tied off before and in after, so that the machine
or a person can cut it without the stitching coming undone, and the thread is cut first (a
*trim*) only when the jump is longer than `trimLength`, 10 mm by default. That follows what the
tools and machines do: Ink/Stitch adds a trim only where an object asks for one ("Trim After") and
otherwise ties jumps longer than its minimum jump length
([fill stitch](https://inkstitch.org/docs/stitches/fill-stitch/),
[preferences](https://inkstitch.org/docs/preferences/)); Wilcom trims when the next connector is
longer than a length the user sets
([automatic trim settings](https://docs.wilcom.com/embroiderystudio/27/en/OnlineHelp/Quality/connectors/connectors-8.htm));
and machines with jump trimming cut the jumps longer than a length set on the machine, Brother's
from 5 to 50 mm
([Brother](https://support.brother.com/g/s/hf/htmldoc/hsm/om/888p5x/en/07_03_07.html)). A trim
costs machine time and risks the thread pulling out, while a jump nobody cuts lies loose (and
entry-level machines don't cut jumps themselves). The default, 10 mm, is a choice between those, not
a published value: hops between letters or seeds stay jumps, and longer loops are cut in the file. Each colour block is one
thread. Satin and fill tie the thread in at the start and after every long jump, and off before
every long jump and at the end: a short stitch back and forth on the spot. A stitch shorter than
`minStitch` is left out, as professional software removes them: it can break the thread or cut
the fabric.

- **Running** follows a path at `stitchLength` steps, with a lock stitch (a short stitch back
  along the line and forward again, never past its end) at each end so the thread does not pull
  out. Sharp corners (turns over about 30°) get a needle point of their own, so the stitches follow
  the outline instead of cutting across a corner; the same holds for underlay. A point that would
  fall within 0.3 of a stitch of a corner moves onto it, once at most: a corner that close to one
  already stitched into is passed, so along a fine zigzag the stitches keep their length (at most
  1.3 stitch lengths) instead of one point moving from corner to corner.
- **Satin** sews a column: the path is its centre line, its stroke width the column's width, and
  the needle crosses from rail to rail at the column's spacing, so the threads lie side by side:
  more open in a narrow column, where the thread bunches at the rails, denser in a wide one so it
  covers (or `density`, when given). Pull compensation widens the column at each rail: a fixed
  `pullCompensation` (0.2 mm, Wilcom's starting value for drill and cotton) plus, if asked,
  `pullCompensationPercent` of the column's width, which gives wide columns more than narrow ones
  (as Ink/Stitch's percentage does); each can differ between the rails (the left one as the path
  is drawn, then the right one), as Wilcom's double-sided compensation does for a column that lies
  on fabric on one side and on other stitching on the other. Push compensation
  (`pushCompensation`) shortens an open column at its start and its end, underlay and all: the
  stitches pile up at a column's ends and push the fabric out, so a column comes out longer than
  drawn (Wilcom's push and pull article, Ink/Stitch's push compensation). No starting value is
  published, so it is off by default. Every crossing goes across the column, the last one too (the
  last point's direction used to be taken as left to right, so a column running any other way
  ended with a stitch along it or on the rail the one before used). The underlay holds the
  column up and follows its width: a centre run under columns narrower than 2 mm, an edge run (along one side and back along
  the other, kept inside the column) from 2 mm, and an edge run then a zigzag between its sides
  from 4.5 mm (estimates from the usual practice). The top stitching starts where the underlay
  ended, so the needle never travels back along the column. A crossing longer than 10 mm (an estimate; professional software makes it a setting) is split
  into shorter stitches (*auto split*), and the cuts move from one crossing to the next so they
  never line up into a seam down the column. The underlay is split the same way: a zigzag's
  stitches run at an angle and are longer than the column is wide, and Ink/Stitch's underlay guide
  warns that unbounded ones destabilise the column; so are the edge run's turn across the column
  and the stitch from one layer to the next. A column is at most 50 mm wide: a wider stroke is
  sewn 50 mm wide, and the export warns. Where a curve bunches the penetrations on the column's
  inner side, stitches there are shortened (*short stitches*, as Ink/Stitch's short stitch inset
  and Hatch's stitch shortening do): a penetration closer than the spacing to one of the last four
  on its rail moves in by 20 % of the column's width, or 40 % when that is still too close (when
  neither clears, the one farther from them). On a gentle curve every other inner stitch is
  shortened, on a tight one they spread over the two depths, and a straight column keeps its full
  width. A 5 mm column round a 3 mm radius had 16 of its 18 inner penetrations within 0.15 mm of
  the one before; now none is. The depths are estimates (15–25 % is the usual shortening).
- **Tatami fill** fills a shape with rows `density` apart at `stitchAngle` degrees, alternating
  their direction. The defaults, rows 0.38 mm apart and stitches up to 4 mm, are Wilcom's tatami
  defaults ([Wilcom: satin and tatami stitch](https://wilcom.com/resources/blog/mastering-basic-stitch-types-satin-stitch-and-tatami-stitch)),
  and 0.38 mm is the spacing David Morrish's *Digital Embroidery* (2025, ch. 2) gives for 40 wt
  thread without gaps (0.40 mm lies the threads flush). Fabrics that need more or less wait for
  fabric presets. A shape drawn without a final close is still filled, as paint fills it; only a
  subpath with no area is sewn as a line. The penetrations sit on a grid every `stitchLength`
  along the rows' direction, shifted a quarter stitch from one row to the next over a four-row
  cycle, so neighbouring rows — in any region — form one stagger pattern and never line up into a
  groove; no stitch is longer than `stitchLength` (an end stitch at most `minStitch` more), and
  only a row's first and last stitches are shorter. A row's last stitch is left out when it is
  short, so the needle turns diagonally into the next row instead of crawling along the edge; a
  long turn (where the edge runs nearly along the rows) is sewn as short travel stitches rather
  than one loose stitch; a turn between two rows that end on one straight edge runs along it. Rows sit half a spacing inside the shape's extent: no row lies on a
  tangent, and the thread's width covers to the edge. Holes are left unstitched: the shape's fill
  rule decides what is inside, as it does for paint.

  Pull compensation stretches each row past the edge at its ends: the fabric draws a row in along
  its length, so a fill sewn exactly to its outline comes out narrower and leaves a line of
  background where it meets its neighbour (Wilcom: tatami fills are pulled less than satins, but
  noticeably over large areas). Each row's first and last needle points move out by
  `pullCompensation` (the first value at the rows' start, the second at their end) plus
  `pullCompensationPercent` of the row's length, as Ink/Stitch's fill pull compensation extends
  each row outward from its centre; at most half way to the next stretch of the same row, so rows
  either side of a narrow gap or hole never meet across it. Nothing else changes: the stagger
  grid, which row stitch is left out at a turn, and every choice of the sewing order are made as
  without compensation (moves are judged between the rows' ends on the outline), so it changes no
  sewing order, and only the end stitches grow, by the compensation. The underlays get none: they
  stay inside the edge. `expand` grows the shape itself all round before it is filled (its union
  with a band twice as wide along its outlines, round at the corners; a negative value shrinks it),
  as Ink/Stitch's "Expand" does, for shapes drawn apart that should meet; a clip group cuts the
  grown shape, so it grows only where it shows.

  Underneath, an edge run along each outline holds the fabric, and a fill at least 10 mm across
  both ways also gets a second underlay: sparse tatami rows (2 mm apart, 3 mm stitches) across
  the top rows, kept 0.6 mm inside the edge so they never peek out (estimates).

  The rows are grouped into *regions* where the shape does not branch (above a hole, each side of
  it, below it; each arm of a letter), linked where they meet, and each region is sewn whole
  before the next, from whichever of its ends and sides is nearer the needle. Each region also
  takes one row of its neighbour's where they meet, cut to the stretch they share, so the fabric's
  pull opens no gap along the seam. The needle gets from one region (or outline) to the next by a
  *travel run*: 2 mm stitches along the shortest way that stays inside the shape, found on a
  visibility graph of the outlines' reflex corners (a shortest way turns at no other), each passed
  0.5 mm inside (as Ink/Stitch's "underpath" does). A travel under stitching still to come is
  hidden, so it may be any length (all of the underlay's are); one over the top stitching already
  sewn shows, and may show for at most 25 mm (an estimate); a way that would show more, or none
  (two separate shapes), lifts the needle instead. A move whose middle lies on the outline is
  checked stretch by stretch between the corners it passes: along two edges in line (the bottoms
  of two squares side by side) it used to be taken for a way inside, and the thread ran across the
  gap between them.

  The order of the regions decides how much travel shows. The pieces of a shape (regions linked
  to one another) are sewn one after another, the nearest next; in each, several orders are
  planned and the cheapest kept, counting travel that shows, hidden travel (it adds stitches),
  jumps and trims (weights chosen by judgement: a trim costs most, a visible travel much more than
  a hidden one). One order sews the nearest region next. The others each pick a region to sew last
  and sew every other region before the one it leads to, ending next to it, so that the way on
  always runs under regions still to come: the approach of US patent 6,397,120 (Soft Sight, 2002),
  which orders a fill's fragments by a walk from the one where sewing ends. In a letter E with
  vertical rows the arms are sewn first, each towards the spine, and the spine last: no jump and no
  trim at any angle, where the nearest-first order had to cut. A 30 mm ring with a hole and a
  puzzle piece are sewn without a trim. Up to 8 regions are tried as the last one (fewer in a
  piece of many regions), and a piece of more than 200 regions is sewn nearest-first.

  The corners' visibility is worked out once per shape, and the outlines' edges are sorted into a
  grid of cells, so that a test along a segment looks only at the edges in the cells it passes (an
  edge is listed only in the cells it passes through, and a shape whose edges would fill more than
  32 cells each on average, such as thousands of edges from corner to corner, keeps no grid and tests
  every edge: a grid of each edge's whole box took 0.8 GB for 20 000 such edges): a
  gear of 90 teeth with 24 holes plans in 60–90 ms (measured, release build) with 2–4 trims and as
  many jumps, where testing every edge against every corner took about 100 ms and gave up
  travelling (the graph was too large), ending with 25–34 trims and over 200 jumps.

Blocks are sewn in stacking order, back to front, because thread sewn later lies on top of thread
sewn earlier, as paint does. An object joins an earlier block of its thread only when it overlaps
none of the blocks sewn between the two, so the machine changes colour as few times as the stacking
allows rather than once per object; it is reached by a jump, with a trim when the jump is longer
than `trimLength` or the object before asked for one. Each object starts as near as it can to
where the needle is when its turn comes, as Ink/Stitch starts fills and satins: a satin column or
a running line at its nearer end (a closed one at its nearest point, several lines nearest
first), a fill's edge run at its nearest corner and its first region the nearest. Each block
starts with the needle up and a jump to its first stitch.

What is sewn is what would print: hidden objects, guides, template layers and non-printing layers
are left out. A clip group sews its clipped art only inside the clipping path: a filled area is cut
to it exactly before it is planned, and a stroke keeps the stitches whose needle points fall inside
(so a running line can stop up to a stitch short of the edge), the needle lifted across each cut
as between a shape's runs, with the thread tied off and in around a long jump. The clipping path's own fill is sewn
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
| Janome JEF | home machines (Janome) | 116-byte header plus the thread list, then 2-byte records | header with counts, hoop, size and margins, Janome palette indices, records, three zero-length jumps per trim (see below) |

Trims in JEF: the format has no trim record. EduTechWiki calls a jump of zero length the trim
command (Janome machines set to cut at cutting commands obey it, and set to cut automatically they
also cut before any jump longer than 3 mm); pyembroidery, asked to write trims, writes each as
three of them (its `trim_at` setting) and reads a run of three jumps as a trim when asked for trims, while it drops a
lone jump that goes nowhere. We write three, which both read as a trim, and read each zero-length
jump as a trim, so the three come back as one.

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
- Wide satin columns are split stitch by stitch (auto split), not into several narrower columns,
  and are at most 50 mm wide (wider strokes are warned about).
- Underlay is chosen by width and size only, not by fabric (no fabric presets yet).
- Pull and push compensation, `expand` and the thread weight are one value for the whole design,
  not per object, and not by fabric: 0.2 mm of pull compensation suits stable woven fabric
  (Wilcom's guide gives 0.35 for T-shirts and 0.4 for fleece). Push compensation is off by
  default: no starting value is published.
- Fill pull compensation stretches rows along their direction only: across the rows, a fill keeps
  half a row spacing inside its edge, as before. `expand` grows it both ways.
- The PES section of a `.pes` is the minimal version 1 header: machines sew the design correctly,
  but PE-Design shows it as a blank document because the editable shapes (CEmbOne/CSewSeg) are not
  written yet. Reading a PES reads its PEC block, which is what machines and every other app use.
- No stitch preview layer and no stitch simulator yet; the imported design is the preview.
- The trim length is one number for every machine and fabric: there are no machine or fabric
  settings yet. Jumps shorter than it are left to a machine with jump trimming, or to a person.
- A fill whose regions form one piece of more than 200 (a comb of 200 teeth) is sewn nearest
  region next, without comparing orders: with 30 mm teeth that is 100 jumps (a comb of five
  teeth is sewn without one).
- Only the order of a fill's regions is searched; the order of objects follows the stacking, and
  each object's start is the nearest one, not chosen with the next object in view.
- The default spacing and stitch length follow professional software (Wilcom), which is more
  open than Ink/Stitch's (rows 0.25 mm apart, 3 mm stitches): VectorCraft sews about 40–60 % as
  many stitches for the same artwork. No fabric presets yet.

## Validation

- `crates/stitch`: 65 unit tests and 3 property tests — the fill covers a rectangle (its rows
  reaching past the edges by the pull compensation, fixed or by the row's length, one value per
  end) and leaves its hole empty, compensated rows reach at most half way across a gap, the
  compensation changes no sewing order (the same rows in the same order, the same jumps, in a U,
  a ring and a comb at four angles), a satin's pull compensation can differ between its rails
  (drawn either way, sewn from either end) and grow with its width, push compensation shortens an
  open column's ends as drawn (never below half its length, a closed column not at all), every
  crossing goes across the column to the last, the spacing follows the thread's weight, an
  unclosed shape is filled, a ring is sewn in regions with no thread across its hole and,
  travelling inside, without a trim, regions overlap by a row, a narrow gap is never sewn across,
  a letter E is sewn in one go at any angle (every 5°) without crossing its gaps, and so are a U,
  combs, a ring and an H (every 15°, underlay or not: a travel that shows a little beats a jump,
  and a cut costs more still), of those orders the one that travels least wins (a U's arms are
  joined to the bar near where they meet it), a turn between rows that end on one straight edge
  is sewn along it (two U shapes at every spacing from 0.3 to 0.45 mm), a travel that would show more than 25 mm lifts the
  needle (a comb of 200 teeth, beyond the order search), a travel passes a corner half a
  millimetre inside, fill rows stagger a quarter stitch at a time through a cycle of four, a gear of 90 teeth and 24
  holes travels inside, the edge grid answers what testing every edge does (long crossing edges included) and stays small however long the edges are (or is not built), separate pieces jump
  with the thread on when close, tied when farther and cut only past the trim length, a move of
  exactly `jumpLength` is sewn (inside a fill, as one stitch) and a jump of exactly `trimLength`
  is not cut, the pieces of a cut block are joined the same way (a short gap by an untied jump),
  the lines of a running path are sewn nearest first, a shape
  starts nearest the needle (with no needle, a line's first subpath as drawn and the next from near it), large fills get a
  tatami underlay and small ones don't, no stitch is longer than asked or shorter than the
  shortest, the angle turns the rows, running lines keep their corners (and their stitch length along a fine zigzag, an edge run too) and lock without passing
  their ends, fills and satins tie in and off, satin underlay follows the width with no trim
  between it and the top, satin spacing follows the width unless given, wide satins and their
  zigzag underlay are split without a seam, satin crossings stay within the column, pull compensation widens it, a tight satin curve gets short stitches on its inner side (and a straight column none), an absurd
  plan is refused before it is made, the DST record round-trips every delta from -121 to 121,
  only a run of three jumps reads as a DST trim, a PEC stitch block with a 16-byte header whose first jump looks like the origin words reads the same as with 20 bytes (in step, only the leading jump is lost; out of step, its length sends the reader back to the right start), a DST colour change can carry a move, each format
  writes a design it reads back with the same blocks and size, a JEF's header extents are where
  its stitches go, an absurd JEF colour count is not believed, and no reader panics on random,
  truncated or damaged bytes. The property tests (`tests/prop_plan.rs`, random rectangles,
  ellipses and star-shaped polygons with random parameters, random pull compensation included):
  every needle point stays on the art (or past it by the compensation at most),
  the thread is cut exactly on the jumps longer than `trimLength`, and no stitch is longer than the
  parameters allow (this found the zigzag fault fixed above: a 50 mm zigzag with corners 0.5 mm
  apart was one 50 mm stitch).
- `crates/engine`: 35 tests (32 command-level) — export writes a readable design in every format,
  a fill's rows reach past its edges by the pull compensation (by default too, one side only when
  asked) and `expand` grows or shrinks it all round, two fills drawn 0.6 mm apart meet with pull
  compensation or `expand`, a finer thread sews more stitches, push compensation shortens a satin
  stroke, compensation takes one value or two (and is refused otherwise),
  options change the plan, a written PES opens as art (also by its magic bytes), an opened design
  is written back unchanged in every format (and again after a second round), with the cuts at
  its blocks' ends, converts between formats with the same needle points, ignores the style
  options and survives the native format, `document.formats` lists the four, empty documents,
  bad styles and absurd densities are refused, type is left out with a warning, the stacking
  order survives grouping by thread, clipping masks cut fills and strokes, template layers are not
  sewn, a forced fill fills a stroke's area rather than its path's inside, a satin wider than
  50 mm is warned about, cuts made by a clipping mask are tied and trimmed only past the trim
  length, `trimLength` decides which jumps are cut (never below `jumpLength`), an object can ask for a trim after it (and the document keeps it), so can a group, a
  compound path or a clip group (cut after its last stitch, never between its members), a missing
  id is refused without changing the others, the move to the next object is cut only when longer
  than `trimLength` (one of exactly that length is not), an object starts near
  the one before (one that starts where the last ended adds no second needle point there), and the defaults `document.formats` lists (the satin spacing table included) are
  the planner's.
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
  stitch longer than 10 mm, at most two shorter than 0.3 mm, no more trims and jumps than its ceilings (one above
  the counts below) and 30–80 % of the person's stitch count.

Measured with VectorCraft's defaults, before and after the ordering of fill regions and the trim
length landed (the person's design in brackets; its count includes the cuts before colour
changes, which ours doesn't). Jumps count every move with the needle up, cut or not, and include
short hops with the thread on (the bullseye's 8 new ones are about 1 mm, between its rings).

| Design | Stitches | Trims, before → now | Jumps, before → now |
|---|---|---|---|
| bullseye | 3590 (8133) | 1 → 0 (6) | 1 → 9 (8) |
| fox | 2205 (5268) | 5 → 3 (6) | 5 → 4 (3) |
| heart suit | 1338 (3429) | 0 → 0 (0) | 0 → 0 (0) |
| joystick | 1662 (3148) | 5 → 4 (4) | 5 → 5 (4) |
| pool 8 ball | 2328 (5214) | 0 → 0 (2) | 0 → 1 (2) |
| puzzle piece | 1344 (2988) | 1 → 0 (0) | 1 → 0 (0) |
| smiling face | 2628 (5733) | 3 → 3 (3) | 3 → 3 (6) |
| SOS button | 2631 (5849) | 8 → 4 (3) | 11 → 6 (2) |
| video game | 2157 (4496) | 8 → 3 (11) | 9 → 12 (12) |
| watermelon | 1549 (2598) | 13 → 2 (2) | 14 → 14 (2) |
| zebra | 3530 (8861) | 11 → 6 (21) | 12 → 15 (39) |

55 trims in all before, 25 now; stitch counts within 2 % of before (hidden travels add a few). With
a 25 mm trim length it would be 7; with 5 mm (every jump cut, as before) about 46. The
watermelon's seeds, which the person did not sew, are joined by tied jumps of 4–10 mm and two
trims.

A later fix: a turn between two rows that end on one straight edge (rows nearly parallel to the
edge) lay exactly on the outline, which the inside test can't tell from outside, so it was taken
for a way out of the shape and the needle lifted or travelled round. Now a segment along an edge
is inside. Jumps fall from 69 to 56 (the bullseye 9 → 3, the video game 12 → 9, the zebra
15 → 12, the pool ball 1 → 0); trims stay at 25, stitch counts within 2 %.

With fill rows 0.38 mm apart instead of 0.4, the designs sew up to 2 % more stitches (the
watermelon and the zebra 1–2 % fewer: rows fall differently against their shapes), 39–59 % of the
person's. Trims go from 25 to 26 in all and jumps from 56 to 58, moved a little by where rows now
fall (the smiling face and the video game gain a trim, the SOS button loses one). No stitch is
shorter than 0.3 mm (three were); the longest is 5.0 mm, the step from an underlay to the next
layer, inside the shape.

With pull compensation on fills (rows reaching 0.2 mm past the edge at each end), the designs
are wider by the compensation, and scaled to the person's size they sew within 2 % of the
stitches they did (39–59 % of the person's). Trims go from 26 to 27 in all (the SOS button gains
one) and jumps from 58 to 57 (the smiling face gains one, the zebra loses two): the needle ends
each object a fraction of a millimetre elsewhere, which moves a few jumps across the trim length.
Without the compensation every count is as before: the satin and travel fixes change none of
these designs. The longest stitch is 5.0 mm, as before.

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
- pyembroidery 1.5.1 reads our JEF trims (three zero-length jumps) as trims when its `trims`
  setting is on, and would not with a single zero-length jump. With its default settings it reads
  no explicit JEF trim at all, ours or anyone's: it drops jumps that go nowhere and adds a trim
  only before a jump longer than 3 mm, so a trim followed by a short move or a colour change is
  not reported. Ink/Stitch's fox JEF (written by pyembroidery) has no trim records: its cuts are
  long jumps, and its one zero-length jump comes right after a colour change.
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
  the read/write conventions (y direction, trim encoding: JEF's three zero-length jumps per trim
  from `JefWriter.py`'s `trim_at`); no code copied.
- EduTechWiki, [Embroidery format JEF](https://edutechwiki.unige.ch/en/Embroidery_format_JEF) —
  the JEF header, records and the zero-length jump as trim command.
- Connectors and trims: Ink/Stitch's [fill stitch](https://inkstitch.org/docs/stitches/fill-stitch/)
  and [preferences](https://inkstitch.org/docs/preferences/) pages ("Trim After", minimum jump
  stitch length, starting near the previous object), Wilcom's
  [automatic trim settings](https://docs.wilcom.com/embroiderystudio/27/en/OnlineHelp/Quality/connectors/connectors-8.htm),
  and Brother's [jump stitch trimming](https://support.brother.com/g/s/hf/htmldoc/hsm/om/888p5x/en/07_03_07.html)
  (the machine cuts jumps longer than a set 5–50 mm).
- Ordering a fill's regions: US patent 6,397,120 (Soft Sight, filed 1999, published 2002;
  [Google Patents](https://patents.google.com/patent/US6397120B1/en)), which orders fill
  fragments by a recursive walk from the one where sewing ends; read for the idea only.
- Ink/Stitch documentation (GPL-3.0 project, documentation read only) and vendor manuals
  (Wilcom/Hatch, ZSK, Madeira, Brother, Janome) for densities, stitch lengths, pull compensation
  and underlay — summarised in the local research notes with a source per number.
- Compensation: Wilcom's [automatic pull compensation](https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Quality/stabilizing/stabilizing-17.htm)
  (one overstitch allowance per fabric, 0.20 mm for drill and cotton, double-sided values) and
  [push and pull compensation](https://wilcom.com/resources/blog/push-and-pull-compensation)
  (satins pulled most, tatami less, column ends pushed out); Ink/Stitch's
  [satin column](https://inkstitch.org/docs/stitches/satin-column/) (pull compensation in
  millimetres and percent, per rail, push compensation at the ends) and
  [fill stitch](https://inkstitch.org/docs/stitches/fill-stitch/) (rows extended outward, Expand)
  parameters.
- Thread weight: [Thread (textiles)](https://en.wikipedia.org/wiki/Thread_(textiles)) (weight as
  kilometres per kilogram) and Wilcom's
  [satin auto spacing](https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Digitizing/stitches/stitches-10.htm)
  (spacing offsets by thread thickness).
- Thread charts: Brother's 64-thread PEC palette and Janome's 78-thread JEF palette, as published
  by those sources; the tables are in `crates/stitch/src/thread.rs`.

## Files

- `crates/stitch/src/lib.rs` — the model: units, commands, stitches, threads, blocks, patterns.
- `crates/stitch/src/plan/` — stitch planning: `mod.rs` (styles, parameters, entry points),
  `geometry.rs` (polylines, spacing points, the rows' axes, lines ordered nearest first),
  `area.rs` (inside a filled shape, row spans, the edge grid, the travel graph), `runs.rs` (joining runs: jumps, trims, ties, short stitches), `running.rs`,
  `satin.rs`, `fill.rs` (rows, regions and their order), `tests.rs`.
- `crates/stitch/src/format/` — `mod.rs` (shared frames, readers, block building), `dst.rs`,
  `exp.rs`, `pes.rs`, `jef.rs`.
- `crates/stitch/src/thread.rs` — the colour palettes and nearest-colour matching.
- `crates/engine/src/cmd/fileio/embroidery/` — the commands' side: `mod.rs` (export options and
  entry point), `import.rs` (a design opened as art), `digitize.rs` (the document's art planned as
  stitches), `sequence.rs` (the order blocks are sewn in, where the needle is before each object,
  trims between objects), and the `embroidery.setTrimAfter` command.
- `crates/engine/tests/embroidery_corpus.rs` and `xtask/src/embroidery_corpus.rs` — the real
  designs and the test that uses them.

Every value these files decide with is a named constant with a description: what it is, its unit,
and its source (a format's documentation, a cited default) or that it is an estimate. What stays
bare is plain arithmetic (halves, the middle of a step, degrees to radians) and a format's field
widths where its layout is written out. Tolerances are shared: `SAME_POINT` (two points closer
than it are one, `lib.rs`) and `NO_LENGTH` (a segment without a direction, far below it, a guard
on dividing by a length, `plan/geometry.rs`). The default stitch parameters are at the top of
`plan/mod.rs`, with the ranges `Params::sane` clamps them to; each format's record codes and
header fields are at the top of its module.
