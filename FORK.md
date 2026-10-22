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
   sources, and its Architecture section: the stitch plan every view of the stitches reads, and the
   decisions the next steps rest on.
2. Read the open embroidery PR's description on GitHub: it explains the terms (stitch, density,
   underlay, pull compensation, trim, tie-in…) for newcomers and lists what is verified.
3. Code: `crates/stitch` (L3: `plan/` stitch planning, `format/` DST/TAP/T01/EXP/PES/PEC/JEF/VP3,
   `thread.rs` palettes), `crates/engine/src/cmd/embroidery/` (the embroidery commands and the
   digitizer) and `crates/engine/src/cmd/fileio/embroidery/` (opening and exporting machine files).

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

**Done: the rest of the review's fixes (measured on the real designs).**

1. **Short stitches on tight satin curves.** Done: a bunched penetration on a curve's inner side
   moves in by 20 % or 40 % of the column's width; a 5 mm column round a 3 mm radius went from 16
   of 18 inner penetrations within 0.15 mm of the last to none.
2. **Default density and stitch length.** Done: fill rows 0.38 mm apart (Wilcom's tatami default,
   Morrish's gap-free spacing), stitches up to 4 mm (Wilcom). Ink/Stitch's denser defaults (0.25 and
   3) are not followed, so VectorCraft still sews 40–55 % of the person's stitches. Per-fabric
   values wait for the fabric presets below.
3. **Pull compensation for fills, and gaps between shapes.** Done: a fill's rows reach past the
   edge at each end by `pullCompensation` (0.2 mm, as satins), at most half way across a gap, and
   without changing the sewing order; `expand` grows a fill all round (path operations in the
   engine) so shapes drawn apart meet. With the default 0.2 mm, rows square to a gap close 0.4 mm of it (less where they cross it at an angle); two fills drawn 0.6 mm apart meet with 0.3 mm of pull compensation or `expand`. A move along two
   edges in line no longer crosses the gap between them.
4. **Narrow filled areas as satin.** Done: a fill's parts narrower than `narrowAsSatin` (7 mm)
   are found by pairing opposite stretches of the outline and sewn as satin columns of varying
   width that follow the shape (rings close, round and pointed ends are followed round); the
   export cuts them out with path operations and fills the rest, the parts sewn in the order that
   moves least. On the real designs trims fell from 26 to 19 and jumps from 57 to 44. Branching
   columns into one another (instead of a square of tatami where strokes meet) is left for
   "branching of touching objects" below.
5. **Satin compensation.** Done: `pullCompensationPercent` adds a share of the column's width,
   each compensation takes a value per side, and `pushCompensation` shortens an open column's
   ends (off by default: no published starting value was found). A column's last crossing now goes across
   it whichever way it runs. A satin's spacing is measured between two penetrations on one rail
   (peak to peak, as Madeira's chart, Ink/Stitch and Hatch do): its crossings lie half that apart,
   twice as dense as before, as dense as the person's satins in the real designs.
6. **Auto spacing by thread thickness.** Done: `threadWeight` (40 wt by default) sets the default
   spacing of fills and satins, which follows the thread's thickness (`(40 / weight)^½`, as
   Madeira's stitch density chart does). Per-thread weights wait for embroidery settings on
   objects.

The review's two other items moved: the trim length by machine to the document settings below,
more real designs to the work alongside.

**Next: from a converter to an embroidery editor.** Embroidery settings on objects, a stitch
preview, stitch counts and the sewing order, design checks, manual stitches and digitizing from an
image. `docs/embroidery.md` › Architecture records the decisions this order rests on. Each step is
its own branch and PR, in this order, so that what is shown comes before what is set: the preview,
counts and sewing order need only the plan and the threads, and the riskiest part, drawing
stitches fast, is tried before anything is built on it.

1. **A stitch plan for each object; the export unchanged.** Done: the export goes through
   `cmd::embroidery::plan` (sewing units, thread blocks, stitch numbers), and
   `crates/engine/tests/embroidery_output.rs` pins the bytes of every export.
2. **Threads from the format's palette.** Done: built-in swatch libraries of Brother's 61 PEC
   threads and Janome's 78 JEF threads, from the tables in `crates/stitch/src/thread.rs` (no new
   assets; their colours are pyembroidery's, credited in `NOTICE`, see `docs/embroidery.md` ›
   References); the document's thread palette (`embroidery.setup`, Janome's when unset), the
   first of the document's embroidery settings; colours matched by CIEDE2000 in CIE Lab instead
   of by RGB distance, a neighbouring block's thread avoided when a similar one is left (38 of the 100
   pinned exports changed, only in their thread numbers); the plan carries each block's thread
   (palette, number, name, ΔE), the
   PES and JEF writers use it, and `embroidery.threads` lists it, flagging threads past ΔE 10.
   Then the review's remaining fixes: PEC 42 is Linen and PEC's appliqué entries 62 to 64 are
   never matched (7 pinned PES exports changed, only in their thread numbers), thread numbers
   that name no thread read as named stand-ins, a newer file's embroidery settings open whatever
   their shape, the chosen thread is typed by palette, the export's warnings are translated, and
   each PES and JEF export is pinned apart from its thread numbers too.
3. **A planning service.** Each sewing unit's stitches kept between plans, a snapshot of the
   document, cancelling; commands (so reachable over MCP): `embroidery.stats` (stitches, threads,
   thread changes, trims, jumps, size, thread used, and sewing time at a speed that is a named
   constant marked as an estimate), `embroidery.sequence`, and `embroidery.check` (thresholds from
   Melco's design checker, and the format's longest stitch).
4. **The stitch preview on the canvas, and the Embroidery panel's Sewing order and Check tabs.** The
   stitch scene and its renderer (settling the layering, see Architecture), an Art/Stitches switch
   with planning in the background, the selected object highlighted and the checks marked, counts in
   the status bar. Dragging in the Sewing order tab moves objects in the stacking order with the
   existing commands (`node.move`; sewing order is stacking order), in one undo step; objects of one
   thread still join when nothing lies between them, and the list says so rather than pretend
   another order. The first change a user sees.
5. **Embroidery settings on objects and in the document.** Stored as Architecture describes (per
   fill and stroke, allowed on type and groups, points moved with the object, files from newer
   versions still opening); values resolved with their source (the object's, then the export's
   option, then the document's, then the default); document settings: fabric (a preset copied into
   the document), thread weight, machine, joining one thread's objects. The trim length comes from
   the machine here: machines differ in whether, and past what length, they cut jumps themselves.
   Commands to set, reset and inspect them, and to store entry and exit points.
6. **The Object tab, and an Embroidery tab in Document Setup.** The interface on the commands
   above: unset values grey with their source, set ones marked, "restore from the fabric".
7. **The embroidery tool: entry and exit points.** One tool with kinds of handles (later angle
   lines, satin rails, needle points, the hoop), its cursor in `crates/tools/src/cursors.toml`;
   dragging the entry and exit, "nearest connection"; the planner ends a fill at its exit point
   and sews running lines and satins the way asked.
8. **Manual stitches.** "Convert to manual stitches" and "Restore object" (the original kept in
   the node's extras), editing needle points with the tool above, drawn thin with their needle
   points by the stitch scene; a Stitches tab listing the plan's numbered stitches; stops, trims,
   and stitches turned into jumps.
9. **Digitizing from an image.** First the engine, through commands: the smallest area in mm²,
   colour overlap in mm, centre lines as running stitch or satin, optionally an angle along the
   shape, colours matched to the format's palette through Image Trace with the thread library.
   Then a large working window in four steps with a live preview. Then an inspector for each area,
   whose overrides survive retracing (kept by the point clicked and its colour, not the area's
   number) and become the areas' embroidery settings.

**Alongside, at any time:** new machine formats (done: Husqvarna Viking and Pfaff VP3, read and
written with each block's own colour, Brother PEC on its own, Happy TAP and Wilcom T01; next: HUS,
Singer's XXX, Barudan's U01 and SEW; they need only `Pattern`, `Block` and `Thread`); real thread
colours in PES (the plan now carries each block's thread); more real designs (the corpus has
nineteen, all from one project: eleven PES, one also JEF, with their artwork, and eight VP3, seven
also PES; add real DST and EXP files, lettering and larger designs, and test on a machine).

**Later**, on the steps above: embroidery lettering; satins from rails, angle lines, and turning
(variable-angle) satin and fill; quality options (triple running stitch, corners, more underlay
choices, removing stitches hidden under others); production (a PDF worksheet, hoops, colourways);
a stitch player; turning a machine file's stitches back into shapes stitched by the rules
("recognise objects"); appliqué; decorative stitches and designs larger than a hoop; random
variation of satin width and spacing, which hides seams (Ink/Stitch and Wilcom offer it);
branching of touching objects; a full PES block, so Brother's PE-Design opens our files;
validation on real Brother and Janome machines.

Known limitations not on this list are in `docs/embroidery.md` › What is not stitched, and in the
PR descriptions.

## Decisions not to undo without asking the user

- `vectorcraft-stitch` is a separate L3 crate that depends only on `vectorcraft-geom`. Nothing that
  knows about the document goes in it.
- The stitch plan (`cmd::embroidery::plan::DesignPlan`) is the one way from a document to stitches:
  the export writes it, and every other view of the stitches reads it rather than planning stitches
  of its own. A unit's settings are decided in one place (`Digitizer::settings`).
- Sewing order follows stacking order. Same-thread objects merge into one pass only when that
  doesn't put anything on top of what it should be under.
- Only what would print is sewn: hidden objects, guides, template and non-printing layers are
  skipped. Clip groups cut fills exactly and strokes to the stitch.
- JEF and VP3 files start at the design's centre (Janome sews from the hoop's centre; VP3 files
  start there). DST, TAP, T01, EXP and PEC start at a corner.
- `.dst/.tap/.t01/.exp/.pes/.pec/.jef/.vp3` are in `UNASSOCIATED_EXTS`: VectorCraft doesn't register
  as their default app.
- VP3 writes each block's art colour, exactly as the object is painted, not the stitch plan's
  palette thread: the project owner's choice (2026-10-22), so an opened VP3 keeps its colours.
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
  [PEC](https://edutechwiki.unige.ch/en/Embroidery_format_PEC),
  [JEF](https://edutechwiki.unige.ch/en/Embroidery_format_JEF) and
  [VP3](https://edutechwiki.unige.ch/en/Embroidery_format_VP3) (with the KDE Community Wiki's
  [VP3 page](https://community.kde.org/Projects/Liberty/File_Formats/Viking_Pfaff), CC BY-SA 4.0).
- **Behaviour of professional software:** public help pages, such as
  [Wilcom EmbroideryStudio's online help](https://docs.wilcom.com/embroiderystudio/28/en/index.htm),
  may be read and cited by URL for concepts and indicative values. Don't copy their text or tables,
  and never run or inspect those programs.
- **`pyembroidery` (MIT)** may be used only as an outside oracle, in a throwaway environment, to
  check that our files read there and theirs read here. Never read its code or copy it into this
  repository. One exception, decided by the project owner (2026-10-21): the thread palette tables
  in `crates/stitch/src/thread.rs` (each thread's screen colour and name, in Brother's and
  Janome's order) are pyembroidery's, credited in `NOTICE` with its MIT notice, because the thread
  makers publish names and catalogue codes but no screen colours. Nothing else of it is taken.
- **EduTechWiki** is cited for facts (the formats' layouts and numbering, a single thread's name
  and colour), never copied as content: its default licence is CC BY-NC-SA 3.0
  ([EduTech Wiki:Copyrights](https://edutechwiki.unige.ch/en/EduTech_Wiki:Copyrights)), which
  doesn't fit this repository's MIT/Apache licences, so it is no source for whole tables.
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
- **A change to what is sewn fails `crates/engine/tests/embroidery_output.rs`:** it lists the
  exports that changed and prints the listing as it now stands. Paste the listing in the commit
  that makes the change, and say in the PR what changed and why. A refactor keeps every hash.
- **Commit a refactor apart from behaviour changes:** first the pure move (code moved or renamed,
  tests unchanged and passing), then the fixes, so each diff shows what really changed.
- **PR descriptions are in English and self-contained.** Someone who knows neither this
  repository's history nor the conversation behind the work must understand them: explain the
  embroidery terms, and don't lean on another PR's text (link it and say in a sentence what it
  brings). Start with a **TL;DR**: a few bullets on what changes, what it gives, and what to
  watch. Then the details, the results (measured, with numbers), the tests, what is not done, and
  the checks run.
- **Don't hard-wrap text that goes to GitHub.** Write each paragraph and each list item of a PR
  description, issue or comment on one line: GitHub shows every line break there, so text wrapped
  at a fixed width reads as ragged short lines. (Files in the repository keep their wrapping.)
- **Embroidery isn't an Illustrator feature,** so it doesn't move the Illustrator-parity scores in
  `docs/target-app-parity.md`. Record it in `docs/file-format-parity.md` (format rows) and the
  ROADMAP's progress log.
