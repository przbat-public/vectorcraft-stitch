# VectorCraft Stitch — instructions for agents working in this fork

This repository (`przbat-public/vectorcraft-stitch`) is a fork of
[storytold/vectorcraft](https://github.com/storytold/vectorcraft). It adds **machine embroidery**:
turning vector art into stitches and reading and writing embroidery machine files.

**This file comes first.** Everything in [`CLAUDE.md`](CLAUDE.md) / [`AGENTS.md`](AGENTS.md) still
applies (clean-room, never crash, everything is a command, layering, wasm, quality gates), with one
exception: **don't pick work from `docs/gaps.md` or the Illustrator-parity roadmap.** In this fork,
work comes from the list below, unless the user gives you a task.

## Orient

1. Read [`docs/embroidery.md`](docs/embroidery.md): what exists, how it works, its limits and
   sources.
2. Read the open embroidery PR's description on GitHub: it explains the terms (stitch, density,
   underlay, pull compensation, trim, tie-in…) for newcomers and lists what is verified.
3. Code: `crates/stitch` (L3: `plan.rs` stitch planning, `format/` DST/EXP/PES/JEF, `thread.rs`
   palettes) and `crates/engine/src/cmd/fileio/embroidery.rs` (open/export, the digitizer).

## Branches

- `main` mirrors upstream. Don't commit embroidery work to it.
- `feat/embroidery` is the base of all embroidery work. It receives merges from `main` now and then.
- Embroidery PRs target `feat/embroidery`.
- **One branch and one PR per piece of work**, cut from `feat/embroidery` and named after what it
  does (for example `feat/embroidery-import-and-stitch-quality`). Every fix for that work is a
  further commit on its branch, with no side branches, until the user says otherwise.
- Commit locally after `cargo xtask ci` passes. **Don't push without the user's go-ahead.**
- **No code names, anywhere.** Never label work with a stage letter, a step number or any other
  internal tag in branch names, commit messages, PR titles and descriptions, docs or code. Say what
  the change does ("an opened design is written back unchanged"). Every description must make sense
  on its own to someone who hasn't seen this file or the conversation behind it.

## Work list (in this order)

**Done: results closer to professional embroidery software.**
- An opened design is written back unchanged (its lines are marked as literal stitches,
  `doc::embroidery`).
- Long satin stitches are split, with the cuts spread out.
- Fills travel inside the shape between regions and overlap them by a row.
- Underlay follows the shape: centre, edge or zigzag by satin width, and a second tatami layer under
  large fills.
- Satin spacing follows the column width.

Also done, from comparing VectorCraft's output with designs a person digitized (the real designs
in `corpus/embroidery`, see `docs/embroidery.md` › Validation): unclosed shapes are filled, fill
penetrations sit on one stagger grid (no stitch longer than asked), short stitches are removed,
travels take the shortest way inside the shape (unlimited under the top stitching), and short
moves between separate pieces jump without a trim.

Details are in `docs/embroidery.md`.

**Done: fewer trims and hidden travels** (from the same review). A fill's regions are ordered so
travels pass under stitching still to come (several orders planned, the cheapest kept: a letter E
is sewn in one go). Jumps are tied off and in, and the thread is cut only past `trimLength`
(10 mm) or where an object asks for it (`embroidery.setTrimAfter`). Each object starts near where
the one before ended. Cuts made by clipping masks are tied. The travel graph keeps only reflex
corners and the outlines' edges are indexed in a grid, so jagged shapes travel instead of jumping.

**Next: the rest of the review's fixes (measured on the real designs).**

1. **Short stitches on tight satin curves.** Done: a bunched penetration on a curve's inner side
   moves in by 20 % or 40 % of the column's width; a 5 mm column round a 3 mm radius went from 16
   of 18 inner penetrations within 0.15 mm of the last to none.
2. **Default density and stitch length.** VectorCraft sews 40–65 % of the stitches of the person's
   Ink/Stitch designs (Ink/Stitch's defaults: rows 0.25 mm apart, 3 mm stitches; ours 0.4 and 4).
   Calibrate with the fabric presets below, citing sources.
3. **Pull compensation for fills, and gaps between shapes.** Only satin columns are widened. Ink/
   Stitch widens fill rows and can expand shapes so neighbours meet without a gap.
4. **Narrow filled areas as satin.** Every filled shape becomes tatami; narrow parts (stems, thin
   arms) are better as satin columns, as auto-digitizers choose by width.
5. **Satin compensation.** Pull compensation is a fixed width on both sides; professional tools
   also take a share of the column's width and a value per side, and *push* compensation shortens
   a column's ends, since sewing lengthens it.
6. **Auto spacing by thread thickness.** Wilcom adjusts satin spacing by thread weight (its
   auto-spacing settings list an offset per thread thickness); a small addition to
   `satin_spacing` once threads carry a weight.
7. **The trim length by machine.** `trimLength` is one number; machines differ in whether and
   from what length they cut jumps themselves. Take it from the machine settings once there are
   some.
8. **More real designs.** The corpus has eleven designs, all PES (one JEF), all from one project.
   Add real DST and EXP files, lettering, larger designs, and test on a machine.

**Then: embroidery properties on objects (a change of direction).**

1. An optional "embroidery" attribute on document nodes: stitch type, angle, density, underlay,
   pull compensation, entry and exit points.
   - Saved in `.vectorcraft`, and set through commands (so reachable over MCP).
   - Left unset, today's automatic rules apply.
   - The current export-time conversion stays as a quick convert.
2. A document-level fabric preset as the source of defaults (numbers cited by source).
3. Stitches computed per object and cached, for a canvas preview, a stitch counter and an
   "Embroidery" panel with the sewing order.

**Later:**
- random variation of satin width and spacing, which hides seams (Ink/Stitch and Wilcom offer it);
- turning a machine file's stitches back into shapes stitched by the rules ("recognise objects");
- editing sewing order and entry/exit points on the canvas;
- branching of touching objects;
- turning (variable-angle) satin and fill;
- embroidery lettering;
- a full PES block, so Brother's PE-Design opens our files;
- validation on real Brother and Janome machines.

Known limitations not on this list are in `docs/embroidery.md` › What is not stitched, and in the
PR descriptions.

## Decisions not to undo without asking the user

- `vectorcraft-stitch` is a separate L3 crate that depends only on `vectorcraft-geom`. Nothing that
  knows about the document goes in it.
- Sewing order follows stacking order. Same-thread objects merge into one pass only when that
  doesn't put anything on top of what it should be under.
- Only what would print is sewn: hidden objects, guides, template and non-printing layers are
  skipped. Clip groups cut fills exactly and strokes to the stitch.
- JEF files start at the design's centre (Janome sews from the hoop's centre). DST, EXP and PEC start
  at a corner.
- `.dst/.exp/.pes/.jef` are in `UNASSOCIATED_EXTS`: VectorCraft doesn't register as their default app.
- Defaults have one source, `plan::Params::for_style`. Export options and `document.formats` mirror
  it, and a test keeps them equal.
- Untrusted input is capped: `plan::try_stitches` (4 million stitches per shape), 2 million per
  design, counts read from files are bounded by file size, and `engine/tests/import_fuzz.rs` covers
  every format.

## Clean-room for embroidery

The upstream rules stand (no Adobe software, no GPL code). For embroidery specifically:

- **Formats:** only public documentation, cited in each module, mainly EduTechWiki's pages on
  [DST](https://edutechwiki.unige.ch/en/Embroidery_format_DST),
  [EXP](https://edutechwiki.unige.ch/en/Embroidery_format_EXP),
  [PES](https://edutechwiki.unige.ch/en/Embroidery_format_PES),
  [PEC](https://edutechwiki.unige.ch/en/Embroidery_format_PEC) and
  [JEF](https://edutechwiki.unige.ch/en/Embroidery_format_JEF).
- **Behaviour of professional software:** public help pages, such as
  [Wilcom EmbroideryStudio's online help](https://docs.wilcom.com/embroiderystudio/28/en/index.htm),
  may be read and cited by URL for concepts and indicative values. Don't copy their text or tables,
  and never run or inspect those programs.
- **`pyembroidery` (MIT)** may be used only as an outside oracle, in a throwaway environment, to
  check that our files read there and theirs read here. Never copy its code into this repository.
- **Never read GPL implementations:** Ink/Stitch, Embroidermodder/libembroidery and similar.
  Ink/Stitch's *user documentation* may be read.
- No test files made with commercial embroidery software in the repository. Write test inputs from
  scratch in code.
- **Real designs for testing** come from public projects whose licence allows it, made with open
  tools (today: the Emoji Embroidery Project, CC BY-SA 4.0, digitized with Ink/Stitch). They are
  fetched by `cargo xtask corpus --embroidery` into the gitignored `corpus/embroidery/`, verified
  against `xtask/embroidery-corpus.sha256`, used locally and never committed. Prefer them to random
  shapes when checking stitch quality.

## Practicalities

- **Rust 1.95 is required:** `cargo +1.95.0 …`. The default toolchain on this machine may be older.
- **Use a separate `CARGO_TARGET_DIR` per agent**, and delete it when done (it grows to tens of GB).
- **The full gate is `cargo +1.95.0 xtask ci`.** It takes over 10 minutes, so run it in the
  background. It needs the `wasm32-unknown-unknown` target for 1.95
  (`rustup target add wasm32-unknown-unknown --toolchain 1.95.0`).
- **`.gitignore` ignores every directory named `plan/`** (meant for the local notes at the root):
  `crates/stitch/src/plan/` is let back in by name. A new directory called `plan` anywhere else
  would silently stay out of commits.
- **Fetch the real designs once** with `cargo +1.95.0 xtask corpus --embroidery`: the engine's
  `embroidery_corpus` test skips without them (`EMBROIDERY_CORPUS_REQUIRED=1` makes it fail
  instead).
- **The fork has no CI on GitHub:** say so when you report results, and don't claim checks passed
  there.
- **When a task lands:** update `docs/embroidery.md` (algorithms, limitations, test counts) and the
  PR description.
- **Commit a refactor apart from behaviour changes:** first the pure move (code moved or renamed,
  tests unchanged and passing), then the fixes, so each diff shows what really changed.
- **PR descriptions are in English** and self-contained, with embroidery terms explained for
  newcomers.
- **Embroidery isn't an Illustrator feature,** so it doesn't move the Illustrator-parity scores in
  `docs/target-app-parity.md`. Record it in `docs/file-format-parity.md` (format rows) and the
  ROADMAP's progress log.
