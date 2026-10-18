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
- **One branch per stage of the work list, cut from `feat/embroidery`, one PR per stage.** Stage A
  goes on `feat/embroidery-stage-a`: every fix for it is a further commit on that branch, with no
  side branches, until the user says otherwise. (`feat/embroidery-core`, PR #2, is merged.)
- Commit locally after `cargo xtask ci` passes. **Don't push without the user's go-ahead.**

## Work list (in this order)

**Stage A — fix results in the current design**

1. **Faithful import.** A design opened from a machine file keeps its exact stitches and exports
   them unchanged. Today it becomes thin lines that are re-stitched on export, so
   DST → VectorCraft → DST changes the design. Converting stitches to editable objects comes later,
   on request.
2. **Auto split for long satin stitches.** A stitch longer than a format allows is split today into
   equal pieces in a straight line, which lines up into a visible seam in wide satins. Spread the
   penetrations out instead.
3. **Travel inside fills.** Moving between tatami regions should sew a hidden travel stitch inside
   the shape (kept a margin away from its edge) instead of trimming. Overlap neighbouring regions
   by a row so no gaps open at the seams.
4. **Underlay by shape:** centre run for narrow columns, edge run plus zigzag for wide ones, a
   second, perpendicular tatami layer for large fills.
5. **Satin density by column width.**

**Stage B — change of direction: embroidery properties on objects**

1. An optional "embroidery" attribute on document nodes: stitch type, angle, density, underlay,
   pull compensation, entry and exit points. It is saved in `.vectorcraft`, set through commands
   (reachable over MCP), and left unset means today's automatic rules. The current export-time
   conversion stays as a quick convert.
2. A document-level fabric preset as the source of defaults (numbers cited by source).
3. Stitches computed per object and cached, for a canvas preview, a stitch counter and an
   "Embroidery" panel with the sewing order.

**Stage C — later.** Editing sewing order and entry/exit points on the canvas, branching of touching
objects, turning (variable-angle) satin and fill, embroidery lettering, a full PES block (so
Brother's PE-Design opens our files), validation on real Brother and Janome machines.

Known limitations not on this list are in `docs/embroidery.md` › What is not stitched, and in the
PR description.

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

## Practicalities

- **Rust 1.95 is required:** `cargo +1.95.0 …`. The default toolchain on this machine may be older.
- **Use a separate `CARGO_TARGET_DIR` per agent**, and delete it when done (it grows to tens of GB).
- **The full gate is `cargo +1.95.0 xtask ci`.** It takes over 10 minutes, so run it in the
  background. It needs the `wasm32-unknown-unknown` target for 1.95
  (`rustup target add wasm32-unknown-unknown --toolchain 1.95.0`).
- **The fork has no CI on GitHub:** say so when you report results, and don't claim checks passed
  there.
- **When a task lands:** update `docs/embroidery.md` (algorithms, limitations, test counts) and the
  PR description.
- **PR descriptions are in English** and self-contained, with embroidery terms explained for
  newcomers.
- **Embroidery isn't an Illustrator feature,** so it doesn't move the Illustrator-parity scores in
  `docs/target-app-parity.md`. Record it in `docs/file-format-parity.md` (format rows) and the
  ROADMAP's progress log.
