# VectorCraft Stitch — instructions for agents working in this fork

This repository (`przbat-public/vectorcraft-stitch`) is a fork of
[storytold/vectorcraft](https://github.com/storytold/vectorcraft). It adds **machine embroidery**:
turning vector art into stitches and reading and writing embroidery machine files.

**This file comes first.** Everything in [`CLAUDE.md`](CLAUDE.md) / [`AGENTS.md`](AGENTS.md) still
applies (clean-room, never crash, everything is a command, layering, wasm, quality gates), with
these exceptions: **don't pick work from `docs/gaps.md` or the Illustrator-parity roadmap** (in
this fork, work comes from the list below, unless the user gives you a task); embroidery work
targets `feat/embroidery`, not `main` (Branches); and embroidery doesn't move the
Illustrator-parity scores (Practicalities, last item).

## Orient

1. Read [`docs/embroidery.md`](docs/embroidery.md): what exists, how it works, its limits and
   sources, and its Architecture section: the stitch plan every view of the stitches reads, and the
   decisions the next steps rest on.
2. The terms (needle point, stitch, record, jump, trim, tie, block, unit, satin, tatami,
   underlay, pull compensation, palette, ΔE) are explained at the top of `docs/embroidery.md` ›
   Terms; what is verified is in its Validation section.
3. Code: `crates/stitch` (L3: `plan/` stitch planning, `format/` DST/TAP/T01/EXP/PES/PEC/JEF/VP3,
   `thread.rs` palettes, `color.rs` the colour difference), `crates/engine/src/cmd/embroidery/`
   (the embroidery commands and the digitizer) and `crates/engine/src/cmd/fileio/embroidery/`
   (opening and exporting machine files).

## Branches

- `main` mirrors upstream. Don't commit embroidery work to it.
- `feat/embroidery` is the base of all embroidery work. It receives merges from `main` now and then.
- Embroidery PRs target `feat/embroidery`.
- **One branch and one PR per piece of work**, cut from `feat/embroidery` and named after what it
  does (for example `feat/embroidery-import-and-stitch-quality`). Every fix for that work is a
  further commit on its branch, with no side branches, until the user says otherwise.
- Commit locally after `cargo xtask ci` passes. **Don't push without the user's go-ahead.**
- **One embroidery PR at a time.** Start the next piece of work only after the PR before it has
  merged into `feat/embroidery`, and don't run parallel sessions or branches on separate pieces:
  they conflict, and the conflicts cost more than the time saved. The owner may allow an exception
  for one PR that touches few of the same files (they did for the new formats, beside the planning
  service, on 2026-10-21); then agree which files each side keeps out of.
- **Always pass `--repo przbat-public/vectorcraft-stitch` to `gh`** (or a full URL). The fork's and
  upstream's PR and issue numbers overlap, and a checkout without a `gh` default resolves a bare
  number to upstream: one review once covered upstream's PR of the same number by mistake. Before
  trusting a PR number in a brief, check its title and URL.
- **Never rename a PR's head branch** on GitHub: GitHub closes the PR instead of following the
  rename.
- **The owner squash-merges, except an upstream sync.** A PR that merges `main` into
  `feat/embroidery` is merged with "Create a merge commit": a squash would drop the record that
  upstream's commits are merged, and the next sync would meet the same conflicts again. After a
  base PR merges, merge `feat/embroidery` into any branch
  stacked on it (taking the branch's side: the squash equals the base's last commit), so its diff
  shows only its own changes. Never push to a merged PR's deleted branch: that recreates it.
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
   3) are not followed, so VectorCraft sews fewer of the person's stitches (the current share is
   in `docs/embroidery.md` › Validation, Real designs). Per-fabric
   values come from the fabric presets (Embroidery settings on objects, in the next list).
3. **Pull compensation for fills, and gaps between shapes.** Done: a fill's rows reach past the
   edge at each end by `pull` (0.2 mm, as satins), at most half way across a gap, and
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
5. **Satin compensation.** Done: `pullPercent` adds a share of the column's width,
   each compensation takes a value per side, and `push` shortens an open column's
   ends (off by default: no published starting value was found). A column's last crossing now goes across
   it whichever way it runs. A satin's spacing is measured between two penetrations on one rail
   (peak to peak, as Madeira's chart, Ink/Stitch and Hatch do): its crossings lie half that apart,
   twice as dense as before, as dense as the person's satins in the real designs.
6. **Auto spacing by thread thickness.** Done: `threadWeight` (40 wt by default) sets the default
   spacing of fills and satins, which follows the thread's thickness (`(40 / weight)^½`, as
   Madeira's stitch density chart does). The document's thread weight is a setting (Embroidery
   settings on objects, in the next list); an export can still give its own.

The review's two other items moved: the trim length by machine to the document settings below,
more real designs to the work alongside.

**Next: from a converter to an embroidery editor.** Embroidery settings on objects, a stitch
preview, stitch counts and the sewing order, design checks, manual stitches and digitizing from an
image. `docs/embroidery.md` › Architecture records the decisions this order rests on. Each step is
its own branch and PR, in this order, so that what is shown comes before what is set: the preview,
counts and sewing order need only the plan and the threads, and the riskiest part, drawing
stitches fast, is tried before anything is built on it.

1. **A stitch plan for each object; the export unchanged.** Done: the export goes through
   `cmd::embroidery::plan` (sewing units, thread blocks, record numbers), and
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
3. **A planning service.** Done: one per open document (`cmd::embroidery::service`), each sewing
   unit's stitches kept by exactly what they are planned from (bounded, least recently used out;
   a plan with them equals a fresh plan, tested as a property and on the real designs), the last
   plan kept per document snapshot and options, cancelling and progress, `Send` and `Sync`; the
   export plans afresh, past the document's service (`service::plan_afresh`). Commands (so reachable over MCP): `embroidery.stats` (counts,
   size, stitch lengths, thread and sewing time, estimates on models whose numbers are named and
   sourced), `embroidery.sequence` (each unit with how it is sewn and why), and `embroidery.check`
   (Melco's design checker's thresholds and others, each rule with its sources, the format's
   limits; Melco's fill, satin and walk-stitch limits checked on each object's settings, as its
   checker reads them, the owner's choice).
4. **The stitch preview on the canvas, and the Embroidery panel's Sewing order and Check tabs.**
   Done: the plan drawn as threads on the canvas (View › Stitch Preview, ⌘E, an Art | Stitches
   switch), planned on a thread of its own with the units kept between plans and shown as soon as
   a light design is planned (measured at three gates, `docs/embroidery.md` › Drawing stitches);
   the selection's entry and exit; Window › Embroidery with a Sewing order tab (rows dragged, or
   Sew Earlier, Later, First and Last, move objects in the stacking order with `layer.move`, in one
   undo step, and the list says what the move did) and a Check tab (problems first, Show, the
   findings ringed on the canvas, the stitch lengths' chart); the counters in the status bar. Every
   word in the 23 languages, numbers as each writes them (and in Turkish, which upstream added
   later, since the sync of 2026-10-24: AI-drafted, not yet reviewed). The first change a user sees.
5. **Embroidery settings on objects and in the document.** Done: stored per kind of part (all of an
   object's fills, all its strokes) on paths and compound paths (a group's set on its objects;
   type takes none until lettering sews it), points moved with the object, files from newer
   versions still opening; values resolved with their source (the object's, then the export's
   option, then the document's, then the default); document settings: fabric (one of eight presets
   copied into the document, each value sourced), thread weight, machine, joining one colour's
   objects; the trim length comes from the machine: machines differ in whether, and past what
   length, they cut jumps themselves. Commands to set, reset, copy and inspect them and to store
   entry and exit points; objects made from others keep them. What moves to item 6: the panels.
6. **The Object tab, and an Embroidery tab in Document Setup.** Done (in review): the Embroidery
   panel's Object tab, first, with the selected objects' settings by part, read without a plan
   (a value set on them with a bar, strong text and ↺; an inherited one dim with its source;
   blank and counted where they differ; Auto's choices in words; entry and exit, Trim After,
   Restore from Fabric, Copy and Paste settings), each edit one undo step; Document Setup ›
   Embroidery with the fabric (a preset copied in, values changed from it marked), the thread and
   its palette, the machine and the joining, and the bar that applies the fabric to objects with
   their own values on OK. Every word in the 24 catalogues (AI-drafted, not yet reviewed).
7. **The embroidery tool: entry and exit points.** One tool with kinds of handles (later angle
   lines, satin rails, needle points, the hoop), its cursor in `crates/tools/src/cursors.toml`;
   dragging the entry and exit, "nearest connection"; the planner ends a fill at its exit point
   and sews running lines and satins the way asked.
8. **Manual stitches.** "Convert to manual stitches" and "Restore object" (the original kept in
   the node's extras), editing needle points with the tool above, drawn thin with their needle
   points by the stitch scene; a Stitches tab listing the plan's numbered stitches; stops, trims,
   and stitches turned into jumps. A converted object's run is a literal run, as an opened file's
   are: the Sewing order's refusal of a lone run (`reorder::Refusal::FromFile`, "a run of an
   opened machine file moves with its block") then needs to tell the two apart, by a mark the
   convert or the import leaves.
9. **Digitizing from an image.** First the engine, through commands: the smallest area in mm²,
   colour overlap in mm, centre lines as running stitch or satin, optionally an angle along the
   shape, colours matched to the format's palette through Image Trace with the thread library.
   Then a large working window in four steps with a live preview. Then an inspector for each area,
   whose overrides survive retracing (kept by the point clicked and its colour, not the area's
   number) and become the areas' embroidery settings.

**Alongside, at any time:** new machine formats (done: Husqvarna Viking and Pfaff VP3, read and
written with each block's own colour, Brother PEC on its own, Happy TAP and Wilcom T01; next: HUS,
Singer's XXX, Barudan's U01 and SEW; see Adding a machine format below); real thread
colours in PES (the plan now carries each block's thread); more real designs (the corpus has
nineteen, all from one project: eleven PES, one also JEF, with their artwork, and eight VP3, seven
also PES; add real DST and EXP files, lettering and larger designs, and test on a machine); a
planner fix: a fill row's remainder spread over its last stitches so that no fill stitch is
shorter than the needle's blade (0.75 mm for a 75/11; the rows end in stitches of 0.4 to 0.75 mm
today, which `embroidery.check`'s `stitchBelowNeedle` flags in 41 of the 44 blocks of the real
designs' artwork), after which that check becomes a warning again.

**Later**, on the steps above: embroidery lettering; satins from rails, angle lines, and turning
(variable-angle) satin and fill; quality options (triple running stitch, corners, more underlay
choices, removing stitches hidden under others); production (a PDF worksheet, hoops, colourways);
a stitch player; turning a machine file's stitches back into shapes stitched by the rules
("recognise objects"); appliqué; decorative stitches and designs larger than a hoop; random
variation of satin width and spacing, which hides seams (Ink/Stitch and Wilcom offer it);
branching of touching objects; a full PES block, so Brother's PE-Design opens our files;
validation on real Brother and Janome machines.

Known limitations not on this list are in `docs/embroidery.md` › What is not stitched.

## Adding a machine format

A reader and a writer take a `Pattern` (blocks of stitches, each in a `Thread`). Then, in this
order, so that nothing is left half done:

1. `crates/stitch/src/format/<format>.rs`: `read`, `write`, and `losses` (what opening a file
   leaves out), each cited from public documentation; the reader bounded by `MAX_POINTS` and the
   file's size, the writer's output by `MAX_FILE_BYTES`.
2. `crates/stitch/src/format/mod.rs`: the `Format` variant and its row in the format table
   (extension, label, palette, `max_step`, `max_colors`, where it starts the needle, `sniff` when
   its bytes can be told), and `Format::ALL`.
3. The engine: `FORMATS` and `UNASSOCIATED_EXTS` in `crates/engine/src/cmd/fileio/mod.rs`, and the
   format lists in the param docs of `document.export`, `document.serialize`, `export.selection`
   and `embroidery.setTrimAfter`.
4. Tests: round trips in the format's module, the `EMBROIDERY` list in
   `crates/engine/tests/import_fuzz.rs`, its pinned exports in
   `crates/engine/tests/embroidery_output.rs` (a new listing, in its own commit), and real files in
   the corpus when a public, licensed source has them.
5. Docs: `docs/embroidery.md` (The formats, Validation), `docs/file-format-parity.md` (its row and
   counts), `docs/mcp.md`, and the ROADMAP's progress log.

## Decisions register

Every decision the project owner made for the fork, and the rules that follow from them, in one
place: when, what, why, what was not chosen, and where the code or a test holds it. Don't undo one
without asking the owner. A new decision gets a row here in the PR that makes it; `docs/embroidery.md`
› Architecture explains the design ones at length and links back here.

| Date | Decision | Why | Not chosen | Where it holds |
|---|---|---|---|---|
| 2026-10-18 | `vectorcraft-stitch` is a separate layer-3 crate whose only workspace dependency is `vectorcraft-geom`: nothing that knows about the document goes in it | the planner and the formats stay testable on their own and usable by the sibling apps | stitch code inside the engine | `cargo xtask layers` |
| 2026-10-18 | Only what would print is sewn: hidden objects, hidden fills and strokes, guides, template and non-printing layers are skipped; clip groups cut fills exactly and strokes to the stitch | a machine file is a print of the art | sewing everything drawn | `Digitizer::kind` (`cmd/embroidery/digitize.rs`); `fileio/tests_embroidery.rs`, `embroidery/tests.rs` |
| 2026-10-18 | An opened design is written back unchanged: its lines are literal stitch runs (`doc::embroidery::StitchRun`), sewn as they are | a machine file opened and saved again must not be re-digitized | planning stitches from the opened lines | `Digitizer::literal`; `embroidery_corpus.rs` (`real_machine_files_are_written_back_unchanged`) |
| 2026-10-18 | `.dst .tap .t01 .exp .pes .pec .jef .vp3` are in `UNASSOCIATED_EXTS`: VectorCraft doesn't register as their default app | embroidery software owns these files on a user's machine | registering as their app | `cmd/fileio/mod.rs`; `fileio/tests.rs` |
| 2026-10-18 | JEF files start the needle at the design's centre, DST, TAP, T01, EXP, PES and PEC files at a corner (VP3, added 2026-10-22, at the centre) | Janome sews around the hoop's centre and real VP3 files start there; the others' decodings start at a corner | one convention for every format | each writer's frame (`format::Frame`); `jef.rs` and `vp3.rs` tests |
| 2026-10-18 | Untrusted input is capped: 4 million stitches a shape, 2 million a design, counts read from files bounded, every reader fuzzed | a crash or an exhausted memory loses unsaved work | trusting files and parameters | `plan::try_stitches`, `digitize.rs`; `import_fuzz.rs`, `no_reader_panics_on_hostile_input` |
| 2026-10-18 | The thread is cut only where a jump is longer than `trimLength` (10 mm) or an object asks for it (`embroidery.setTrimAfter`); longer jumps are tied off and in | machines and people cut short jumps themselves; fewer cuts sew faster | cutting every jump past `jumpLength` | `plan::TRIM`, `Sequence::add`; `a_move_of_exactly_the_trim_length_is_not_cut` |
| 2026-10-19 | Fill rows 0.38 mm apart and fill stitches up to 4 mm by default (Wilcom's tatami defaults) | professional software's defaults; covers 40 wt thread without gaps | Ink/Stitch's denser 0.25 mm rows and 3 mm stitches | `plan::FILL_SPACING`, `FILL_STITCH` |
| 2026-10-19 | Every value the embroidery code decides with is a named constant that says what it is, its unit, and its source or that it is an estimate | nobody could tell what bare numbers meant | bare literals | review; `docs/embroidery.md` › Files |
| 2026-10-19 | Sewing order follows stacking order; objects of one colour join one block only where that puts nothing on top of what it should be under | thread sewn later covers thread sewn earlier, as paint does | reordering freely to save colour changes | `Sequence::target` (`cmd/embroidery/sequence.rs`) |
| 2026-10-20 | Where a value comes from: the object's own, then the export's option, then the document's embroidery settings, then the built-in default; *Auto* is a rule, not a value | one order every view can show | per-command precedence | `Digitizer::settings`; `docs/embroidery.md` › Architecture |
| 2026-10-20 | A fill and a stroke are separate parts with settings of their own; a fabric preset is copied into the document, not linked; the Sewing order tab moves objects in the stacking order; an Embroidery panel of its own; manual stitches by an explicit convert that can be undone; digitizing from an image in a large window | the product plan (`docs/embroidery.md` › Architecture) | one setting per object; live fabric links; a free sewing order; editing planned stitches in place | `docs/embroidery.md` › Architecture › Decided |
| 2026-10-21 | The stitch plan (`cmd::embroidery::plan::DesignPlan`) is the one way from a document to stitches: the export writes it and every other view reads it | every view shows what will be sewn | views that plan stitches of their own | `embroidery_output.rs` pins every export |
| 2026-10-21 | Defaults have one source, `plan::Params::for_style`; the export options and `document.formats` mirror it | one place to change a default | defaults restated in the engine | `defaults_are_the_planners` |
| 2026-10-21 | Colours are matched to threads by CIEDE2000 | it keeps greys, darks and browns in neutral and brown threads | OKLab (straighter saturated blues, but tinted threads for neutrals) | `vectorcraft_stitch::color`; `prop_threads.rs` |
| 2026-10-21 | A document that sets no thread palette is matched to Janome's JEF palette | the owner's product choice | Brother's PEC palette (a little nearer on the real designs: a median ΔE of 5.4 against 6.0) | `cmd::embroidery::DEFAULT_PALETTE` |
| 2026-10-21 | A thread farther than ΔE 10 from the art's colour is flagged as a mismatch | a line between "looks like it" and "looks unlike it" (an estimate) | no flag; a tighter line most real art would fail | `thread::MISMATCH_DELTA_E` |
| 2026-10-21 | A block takes another thread than a neighbouring block's only when one within ΔE 10 is left; otherwise it keeps its nearest, with a machine stop between two blocks of one thread | otherwise the zebra's dark grey was sewn in Royal Purple | always changing thread at a colour change | `Palette::assign`; `sequence.rs` tests |
| 2026-10-21 | PEC's entries 62 to 64 are appliqué entries, never matched or written | they are not threads | treating them as threads | `Palette::is_thread` |
| 2026-10-21 | pyembroidery's thread tables (colours, names, order) are used and credited in `NOTICE` with its MIT notice; nothing else of pyembroidery is taken, and its code is never read | the thread makers publish names and catalogue codes but no screen colours | EduTechWiki's tables (CC BY-NC-SA 3.0, which doesn't fit this repository) | `crates/stitch/src/thread.rs`, `NOTICE`; Clean-room below |
| 2026-10-21 | One embroidery PR at a time (see Branches) | parallel work conflicts | parallel sessions, except one the owner allows | this file |
| 2026-10-22 | VP3 writes each block's art colour, exactly as the object is painted | an opened VP3 keeps its colours | the palette thread's colour | `vp3::thread_record` |
| 2026-10-22 | Melco's fill, satin and walk-stitch length limits are checked on each object's settings | Melco's design checker reads objects and their settings | measuring every stitch | `cmd/embroidery/check.rs` |
| 2026-10-22 | The needle-blade rule (`stitchBelowNeedle`) is information until the planner spreads a fill row's remainder | VectorCraft's own fill rows end in 0.4 to 0.75 mm remainders | a warning on nearly every fill | `check.rs` (`RULES`) |
| 2026-10-22 | The thread and time figures stay estimates, said so in every answer, until they are measured | no published formula | presenting them as measurements | `cmd/embroidery/stats.rs` (`estimate`) |
| 2026-10-22 | Where the needle comes from stays exact in the planning cache's key (not rounded to 0.1 mm) | measured: rounding saved no re-planning | rounding the start point | `cmd/embroidery/cache.rs` |
| 2026-10-22 | The export plans afresh, not from the units kept between plans | what a file holds never depends on what was kept | reusing kept units | `service::plan_afresh` |
| 2026-10-22 | All the fixes of the ten-reviewer review of the fork go in one PR, the large performance rebuilds included | the owner's choice of scope ("everything") | only the major findings | the review-fixes PR |
| 2026-10-22 | Output may change for four fixes: closed running lines get lock stitches; the JEF header's record count equals the body's records; fill rows are centred, half a spacing from each edge; a VP3 trim is written as a move a machine recognises as a trim (EduTechWiki: a long form of 255 or more), and VP3 long moves read back as cuts | each one sews better or reads right elsewhere | keeping today's bytes | the planner (`plan/running.rs`, `plan/fill.rs`), `format/jef.rs`, `format/vp3.rs`; listed in `embroidery_output.rs` |
| 2026-10-22 | Bobbin thread is calibrated to Madeira's per-1,000-stitch figures (bobbin about 0.6 of the top thread), still an estimate | the earlier model was about half of Madeira's figure | the satin balance scaled by Wilcom's factors | `cmd/embroidery/stats.rs` |
| 2026-10-22 | The ArtCraft marks and the ArtCraft community links are removed from what the fork shows; "based on VectorCraft by the ArtCraft team" stays in plain text; the brand files stay in the repository, unshown | `docs/brand/LICENSE-brand.txt`: a published modified version must remove or replace the marks, and this fork is published as its own version, not kept only to propose changes back | keeping them, which the licence allows only a fork that proposes its changes back | `README.md` (no logos, Discord badge or getartcraft.com links up top); the app: no `help.discord`, `help.website` or `help.appPage` (`cmd/help.rs`), no Discord button in the app bar (`chrome.rs`), only the GitHub link in About and Home (`community.rs`), the About credit line (`dialogs/about.rs`), the Help menus (`menus.rs`); the Linux and Windows packages' links point at the fork's repository |
| 2026-10-22 | A design opened from a machine file keeps its start point and the DST, TAP, T01, EXP, PES and PEC exports start from it; new art starts where the format expects (a corner); JEF and VP3 start every design at its centre; overwriting a file whose opening lost something asks first | an operator lines the needle up on a garment mark from the file's start; Janome and Husqvarna Viking machines put the start at the hoop's centre, so a start at a corner (as our DST, EXP, PES and PEC files have it) would put the design beside it | always starting at a corner; JEF and VP3 starting from the file's start too | the document's embroidery settings (`start`, `embroidery.setup`); `format::Frame`; the export's loss check |
| 2026-10-22 | Blocks join only on the exact art colour, not on the thread matched to it | the owner's choice: the art's colours are what the person drew | joining on the matched thread | `Sequence::target`; `docs/embroidery.md` › How a shape becomes stitches |
| 2026-10-22 | One vocabulary for the export's options and the planned settings on objects, before those exist; the options' earlier names stay as aliases; `underlay` takes a kind besides true and false | one name per setting in every place it is set or reported | two vocabularies mapped by hand | `cmd/embroidery/options.rs`; `docs/embroidery.md` › Architecture |
| 2026-10-22 | Keys are camelCase in commands and replies, snake_case in the `.vectorcraft` file (as every node and document key upstream writes); an object's embroidery reads as the document's settings do: unknown keys and values of another type are kept, nothing in it stops a file from opening | the file's own convention, and a file a newer version saves must open in an older one | camelCase in the file; a closed schema that drops what it doesn't know | `doc::embroidery::Embroidery` (`extra`, `unread`); `prop_embroidery::any_embroidery_an_object_carries_opens_and_saves_back` |
| 2026-10-22 | The stitch preview's scope is the plan's, less five items: picking by stitch (to the embroidery tool's PR), the shaded thread look (later), planning in slices on the web (it plans in one go), the cached stitch raster (built only if measured needed: it wasn't), a planner for documents that aren't open (to the image window's PR) | the owner's choice of scope ("full, less five") | everything at once | `docs/embroidery.md` › Planning for the stitch preview, Drawing stitches |
| 2026-10-22 | The stitch view draws each block in its matched palette thread, what the machine gets (a VP3 writes the art's colour instead: said in the docs) | the preview shows what will be sewn | the art's colour | `cmd/embroidery/scene.rs` |
| 2026-10-22 | A click in the stitch view selects the art, as in the art view | picking by stitch comes with the embroidery tool | picking stitches now | the canvas's hit testing, unchanged; `embroidery/tests.rs` |
| 2026-10-22 | Threads are drawn flat at their real width (0.40 mm by the thread weight), with a thin contrasting edge for a thread near the fabric's colour; jumps and cuts are drawn (dashes where no thread lies, a thin line where it stays, a × at a cut), on by default with a toggle | white thread on white fabric stays visible; moves show where the thread goes | a shaded look; moves hidden | `render::lines`, `cmd/embroidery/scene.rs`; View › Stitch Preview Options |
| 2026-10-22 | A selection is never dimmed in the stitch view: its frame and its entry ◆ and exit ⊕ markers only; a moment's emphasis on hovering a Sewing order row or after the Check tab's Show, by a tint of the frame drawn, never drawing the stitches again | dimming hides what the person is looking at | dimming everything unselected | `embroidery::overlay`, `StitchLayer::emphasize` |
| 2026-10-22 | Read-only embroidery text writes numbers as the interface language does (CLDR's separators and minimum grouping: Polish "0,38 mm", "4752", "10 000"); input fields stay as they are | the person reads their own language's numbers | a point everywhere | `embroidery::numbers` |
| 2026-10-22 | Superseded on 2026-10-23 (below): the stitch preview plans with the options of this document's last embroidery export, else the app's last export's, else the defaults; the panel says which; not saved in the file | the preview shows what the next export writes | the defaults only; options saved in the document (the next PR's settings) | was `cmd/embroidery/view.rs` (`note_export`, removed with it) |
| 2026-10-22 | The status bar shows stitches · colours · thread changes · about the time, only while the stitch view or the Embroidery panel is open; trims go in the Sewing order's footer | the figures a person sews by, at no cost while not wanted | trims in the bar; counters always | `embroidery::counters` |
| 2026-10-22 | The first Window › Embroidery docks the panel as the last tab beside Properties, Layers and Libraries, turning the dock into the shared layout once | as the mockups draw it | a floating panel | `panels::embroidery::show_panel` |
| 2026-10-22 | A drag in the Sewing order that would put an object into a clip group or take it out of one is refused, the reason in the drop's hint | a drag changes when something is sewn, never what | moving it in or out | `reorder::Refusal::ChangesClipping` |
| 2026-10-22 | A drag in the Sewing order moves objects with `layer.move` ("A just above B, in B's parent"), one undo step named Change Sewing Order, locks respected | exactly the stacking change a drop means | `node.move` | `cmd/embroidery/reorder.rs` |
| 2026-10-22 | The rest of the stitch preview's choices: ⌘E for View › Stitch Preview; Outline draws over the stitches; the art under the stitches hidden by default (a toggle shows it); the fabric white; a passed check a neutral ✓; the panel's Object and Stitches tabs hidden until built; a slow design's stitches refresh about 0.1 s after a drag ends; a design that can't be planned shows its last stitches dimmed, with the reason; the chart's "too long" band from 12.7 mm, as the check's; the web plans in one go; Next and Previous Finding in; the canvas's notice says "planning" or "can't be planned" when the status bar doesn't | the owner accepted them together ("accept all") | each one's alternative in the plan | `embroidery::menu`, `stitch_layer`, `marks`, `stitch_preview`, `panels/embroidery/lengths.rs`, `notice` |
| 2026-10-22 | The planner sews an object's appearance as it prints (stroke alignment, dashes, width profiles, brushes, arrowheads, live effects, a group's own fills and strokes, a symbol instance's staining); what still can't be sewn gets a warning | sew what prints | sewing the base path and warning | `Digitizer::shape` |
| 2026-10-23 | Embroidery settings come in two PRs, one after the other: the data, the resolution and the commands, then the Object tab and Document Setup › Embroidery | the owner's choice: each PR reviewable alone, the interface built on commands already pinned | one PR | this file › Work list |
| 2026-10-23 | A fill's angle counts counter-clockwise on the page, as every angle in VectorCraft (any number of degrees, not brought into 0° to 180°: a turned set angle is kept in (−180°, 180°], an unset one reported as the default plus the turn; `docs/embroidery.md` › Settings and transforms); the default rows read 135° (the planner's 45°, the same bits) and the export's `angle` turns with it; converted for the planner in one place | one convention; no export changes | the planner's clockwise angle | `options::planner_angle`; `a_fill_angle_turns_counter_clockwise_on_the_page`; the pinned "every option" document passes `stitchAngle` 30 (the 2026-10-24 row) |
| 2026-10-23 | Entry and exit points per part (a fill's, a stroke's), in document points, moved and turned with the object; a part starts at the start its stitches can take nearest to its entry; the exit is kept and reported, and planned toward with the embroidery tool | each part is its own unit | one pair per object | `PartSettings::points`; `an_entry_point_starts_the_part`; `embroidery.setEntryExit` |
| 2026-10-23 | The preview and every export resolve each value from the object's own, then the export's own options (that one export or command alone), then the document's settings, then the built-in default; the preview remembers no export's options (superseding the 2026-10-22 row) | the preview shows what the document sews; an export's options are a one-off override | the last export's options | `cmd/embroidery/resolve.rs`; `an_exports_options_are_not_the_previews`; `the_first_layer_that_gives_a_value_wins` |
| 2026-10-23 | An angle turns with its object: a set one as it is stored (its direction's image, a half turn under a mirror; pull sides swapped), an unset one by the object's bounding-box angle (upright after Reset Bounding Box; a mirror maps the box's axis to its image nearest it, so a turned object's unset rows don't mirror with it) | a shape turned keeps its rows the same across it | unset angles staying put on the page | `Embroidery::transform`, `UnitSettings::above_the_document`; `an_unset_angle_turns_with_its_object`; the pinned `symbols` document |
| 2026-10-23 | Settings set on a group are written to its objects, as paint is; nothing is inherited from groups and layers | one place a value is kept | inheritance | `objects::takers`; `set_takes_a_groups_objects_and_lists_what_it_skips` |
| 2026-10-23 | Copying settings from one object to others is in the data PR (`embroidery.copyFrom`); Copy and Paste in the Object tab come with it | the owner's choice: the Object tab's Paste needs only a command, which agents and scripts get too | later, with the tab | `objects::copy_from` |
| 2026-10-23 | Objects made from others keep the settings of what they come from: Pathfinder, Offset Path, Shape Builder, Divide Objects Below, Split Into Grid, Create Outlines, Make and Release Compound Path, Outline Stroke, Expand Appearance, Ungroup and Break Link, through one-line hooks; Merge keeps shapes of one colour apart when their settings differ | losing a person's settings silently is lost work | dropping them | `cmd/embroidery/derive.rs`; `tests_derive.rs` |
| 2026-10-23 | Fabric presets: Wilcom's eight auto fabrics under our own words, and a fabric of one's own; values by kind of stitch (tatami, satin); stabiliser, topping and needle for information; every value with its source or marked an estimate; the one-family caveat (BERNINA's software is Wilcom's) stated in the code, `docs/embroidery.md` and `embroidery.fabrics`; the default fabric sews as the built-in values, bit for bit | more fabrics at once, each value traceable | fewer presets; a copied table | `crates/stitch/src/fabric.rs`; `embroidery.fabrics`; `the_test_sheet_on_each_fabric_exports_unchanged` |
| 2026-10-23 | Embroidery fields show the interface language's decimal mark and take both "," and "."; a set value is marked by an accent bar, ↺ and strong text with the inherited value as a hint, an inherited one dim with its source, the coral outline for focus alone | the owner's choices for the panels: a person can tell at a glance what an object sets and what it inherits | other fields' conventions | `embroidery/fields.rs` (the numbers), `embroidery/rows.rs` (the marks), `panels/embroidery/object.rs` (the Object tab), `embroidery/setup_tab.rs` (Document Setup › Embroidery) |
| 2026-10-23 | The rest, accepted together: type takes no settings until lettering sews it; a symbol instance takes only Trim After (its art takes the settings); satin isn't offered for a fill in the interface (the data keeps its meaning); Restore from Fabric clears an object's values but not its entry, exit or Trim After; choosing a fabric replaces the values changed before it; "apply the fabric to N objects too" acts on OK, in the same undo step; the fabric's colour shows in the preview; no embroidery options dialog at export (the save options dialog's unreachable rows removed, `file.formatOptions` reports the options unset; options stay for commands and MCP, one export at a time); a fabric's spacings are for 40 wt and scale with the thread weight, while values typed on an object or given to an export are used as they are; `embroidery.set` takes every part key; one set of settings per kind of part; files saved by upstream lose objects' settings (documented, proposed to upstream); the fork's own app identity; the Polish terms "Kompensacja ściągania", "Poszerzenie wypełnień" and joining by "kolor" | the owner accepted them together ("accept all"). Each one's reason and what wasn't chosen: type, because no lettering sews it yet (settings that sew nothing; not: settings on type now); the instance, because its art is what is sewn (not: settings on instances overriding the art's); satin on a fill offered nowhere, because a fill has no width to sew across (not: offering it); Restore from Fabric keeps the points and the cut, which are no fabric's values (not: clearing everything); a new fabric replaces the values changed before it, since a fabric is copied (not: keeping them); applying the fabric to objects in Document Setup's own undo step (not: a step of its own); the fabric's colour in the preview, to see the design on it (not: a neutral ground); no options dialog at export, since the object and the document now hold the settings (not: the dialog); a fabric's spacing for 40 wt, as the presets' sources give it (not: for the thread in use); every part key on both parts, since a stroke can be sewn as an area (not: per-part key lists); one set per kind of part, since each part is one unit's settings (not: one per fill and per stroke, which no file of another tool keeps); files saved by upstream losing objects' settings, since its node reader drops unknown keys (not: a change to upstream's reader in this fork); the app identity (the 2026-10-24 row); the Polish terms the catalogue already used (not: new ones) | `objects.rs`, `setup.rs`, `fabrics.rs`, `scene::fabric_color`, `fileio::embroidery::unset_option`, `io.rs`'s `asks_options`; the catalogues |
| 2026-10-24 | The fork has an app identity of its own on macOS and Windows: the bundle identifier `ai.storyteller.vectorcraft.stitch`, the name "VectorCraft Stitch" on the bundle (`VectorCraft Stitch.app`) and in the Windows installer (product, install folder, registry key, shortcuts, descriptions), its own MSI upgrade code and component GUIDs, and "VectorCraft Stitch contributors" as the installer's publisher. The `.vectorcraft` document types stay VectorCraft's (both apps open the same files), and the executables keep their names. Linux comes later | macOS takes two apps of one identifier for one app, and an installer with VectorCraft's upgrade code would uninstall VectorCraft; with its own name the fork installs beside it; the publisher doesn't credit upstream's company with a version it doesn't make | Linux now (its id is in about fifteen places and file names that upstream changes often, and the fork builds no Linux packages); the identifiers alone (two apps named VectorCraft can't sit side by side); upstream's publisher | `packaging/macos/Info.plist.in`, `packaging/macos/package.sh`, `xtask/src/bundle.rs`, `packaging/windows/vectorcraft.wxs` |
| 2026-10-24 | Five choices of the settings PR, accepted together: `embroidery.applyFabric` without ids acts on the selection, else on every object of the document; a stitch or underlay id an object holds that this version doesn't know reads as unset (the export's, else Auto); `embroidery.set` takes `satin` on a fill (the panel won't offer it); an entry point starts its part at the start its stitches can take nearest to it; the cotton preset is 0.55 mm thick (Melco's broadcloth figure) | ids stand in for the selection in every command; a newer version's value falls through as any unread value does; the data keeps its meaning; the planner's start as it is; a sourced thickness | the whole document always; Auto; refusing satin on a fill; the exact point; the stats default of 0.5 mm | `fabrics::apply_fabric`, `resolve::PartLayer::read`, `objects::value_param`, `Digitizer::plan`'s `from`, `stitch::fabric` |
| 2026-10-24 | A stroke of an object expanded for sewing that nothing reshapes (no brush, no effect of its own or of the object's) is sewn as a basic object's stroke, along its path, by its own settings; a stroke a brush or an effect reshapes is outlined as before, and a running or satin stitch set on it (or given to the export) is said not to follow its path | a stroke set to running sews one line along its path, not two round its outline | outlining every stroke of an expanded object | `printed::split_strokes`, `printed::STROKE_LOSES_ITS_PATH`; `tests_expanded.rs` |
| 2026-10-24 | The options' earlier name `stitchAngle` keeps its earlier meaning, the planner's clockwise angle (`angle` = 180 − `stitchAngle`); `angle` counts counter-clockwise on the page, and a value recorded with it before is `180 − old` | the alias exists to keep earlier callers working: recorded Actions, scripts and agents that give it sew as before | the alias following `angle`'s new meaning (every old value's rows mirrored, silently) | `options::as_held`; `docs/embroidery.md` and `docs/mcp.md` (the `angle` option) |
| 2026-10-24 | The settings commands' "invalid parameters" errors stay English, as every command's do; the messages of their other errors are translated | an agent or a script reads them, and upstream's parameter errors are English too | translating them | `crate::cmd::bad`; `tests_embroidery.rs` in the catalogues |
| 2026-10-24 | Two conventions for what an object's settings keep that a version doesn't read, which every later version keeps to: every `[x, y]` inside a part's `points` object (to a depth of 16) is a position moved with the object, and a `frame` of six numbers is composed with the object's transform; nothing else kept moves | older versions move a newer file's kept data on every transform, so the rule must hold in both directions; a newer version's satin rails and angle lines can be kept under `points` | moving nothing kept (a newer version's positions left behind on the first nudge) | `doc/src/embroidery/transform.rs`; `docs/embroidery.md` › Settings and transforms |

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
  tools (today: the Emoji Embroidery Project, CC BY-SA 4.0 as its mirror's index says, digitized
  with Ink/Stitch; two radish designs' artwork licences are unverified). They are
  fetched by `cargo xtask corpus --embroidery` into the gitignored `corpus/embroidery/`, verified
  against `xtask/embroidery-corpus.sha256`, used locally and never committed. Prefer them to random
  shapes when checking stitch quality.

## Keeping up with upstream

**How a sync goes.** `git fetch upstream --prune`; fast-forward `main` to `upstream/main` (never a
fork commit on it) and push it; on a branch cut from `feat/embroidery`, `git merge main`; resolve
as below, keeping upstream's changes and putting the fork's hooks back; add the fork's rows to any
catalogue upstream added (the catalogue tests want every fork row in every language); gate with
`EMBROIDERY_CORPUS_REQUIRED=1 cargo +1.95.0 xtask ci` in a checkout of its own, with the
embroidery perf rows (`vectorcraft-cli perf --embroidery`) before and after; open the PR to
`feat/embroidery`, merged with a merge commit (Branches). The merge commit's message lists every
conflicted file and how it was resolved.

**Syncs so far.** 2026-10-18, at the fork's start (upstream `904f0fe5`, no conflicts), and again
the same day (upstream `cdba0aac`, PR #5: the ROADMAP's log and `fileio/mod.rs`'s format lists).
2026-10-24, after the stitch preview: upstream `cdba0aac..4c82749b`, 1305 commits; 35 conflicting hunks in 14
files, all listed in the merge commit, of which the status bar (upstream's fixed slots) and
`tab_title` (a parameter of upstream's in the fork's place) were re-ported by hand; Turkish rows
for every fork string; three fixes in upstream's code for the fork's Rust 1.95 gate (below).

**Upstream's CI lints with the stable clippy** (on 2026-10-24, Rust 1.99.1), newer than the fork's
1.95 gate (`rust-version = "1.95"`), and since that sync the same CI runs on the fork's PRs
(Practicalities). Code must pass both:
- Upstream code can fail the fork's 1.95 gate, through a lint 1.95 doesn't know or one 1.95 flags
  and the newer clippy doesn't. Allow it in place, marked "VectorCraft Stitch", and list it with
  the hooks below. Drop the allow when the fork's gate moves to upstream's clippy.
- The fork's code can fail the stable clippy (the DST reader's `chunks_exact`, which 1.99 wants as
  `as_chunks`). Fix it there, in a way 1.95 accepts too.
- So before pushing, also run `cargo +stable clippy --workspace --all-targets --keep-going -- -D
  warnings`.

Where merging `main` (upstream) into the fork conflicts, and how to resolve each so nothing is lost:

- **`crates/doc/src/node.rs`, the node's extras:** the fork's `embroidery` sits after `perspective`
  in all six lists (`NodeExtras`, `NO_EXTRAS`, `Serialize for Node`'s `skips` and its `field!`
  calls, `NodeWire`, `From<NodeWire>`), not at their ends, where upstream appends its own extras.
  `skips` and the `field!` calls pair up by position: keep them in the same order, or a field is
  written as skipped and the next as `null`. The key-order test in `node.rs` checks it.
- **The interface catalogues (`crates/ui-egui/src/i18n/*.tsv`):** both sides add rows at the end.
  `.gitattributes` merges them as a union, so both sides' rows are kept without a conflict. A row
  both sides changed comes back twice: keep the newer (the i18n tests fail on a broken row).
- **`crates/engine/src/cmd/fileio/mod.rs`:** `UNASSOCIATED_EXTS` is a union of both sides' lists
  (taking either alone would associate the embroidery extensions with the app, or drop upstream's
  new ones). In the param docs of `document.serialize`, `document.export` and `export.selection`,
  take upstream's text and add `|dst|tap|t01|exp|pes|pec|jef|vp3` to its format list again.
- **The generator string** (`crates/format/src/lib.rs`): the fork's files say `VectorCraft Stitch`
  and the version, so a file from the fork can be told from upstream's in a report.
- **Objects made from others keep their embroidery settings** (`cmd/embroidery/derive.rs`, each
  hook marked "VectorCraft Stitch"): in `cmd/pathops.rs`, `Style`'s `embroidery` field (set in
  `of`, `style_of`'s text arm, given in `apply`: Pathfinder, Offset Path, Shape Builder, Divide
  Objects Below, Split Into Grid) and the `Expanding` around `assemble` in `outline_strokes`
  (Outline Stroke); in `cmd/effectcmd.rs`, `derive::expand_leaf` for `effects::expand_leaf` in
  `expanded_node` and `derive::expand_art` for `effects::expand_art` in `expand_pieces`, and `derive::outlined` after
  the outlines' `expand_pieces` in `expanded_node` (Expand
  Appearance at any depth, and the embroidery walk's copy of the document as it prints; an
  expanded path's pieces also take its bounding box's angle, so that a stroke's outline turns its
  unset rows as the path did); in `cmd/typecmd.rs`'s
  `create_outlines`, `outlined_type`; in `cmd/object.rs`, `ungrouped` in `ungroup`, `compound` in
  `compound_make`, `released` in `compound_release`; in `cmd/brushsym.rs`'s `break_links`,
  `unlinked`; in `cmd/path.rs`'s `transform_anchors`, `anchors_transformed` (a path whose anchors
  all move takes its settings along).
- **One-line hooks the fork keeps in upstream files**, to keep when resolving: `doc/src/node.rs`'s
  `transform_scaled`, after the bounding box's angle (an object's embroidery settings move and turn
  with it, `Embroidery::transform`), `doc/src/mem.rs`'s document and node estimates (the
  embroidery settings' memory, `embroidery::setup_bytes` and `object_bytes` in
  `doc/src/embroidery/mem.rs`; `ALLOC`, `map_bytes` and `json_bytes` made `pub(crate)` for them), `fileio/load.rs`'s `is_loss` (`super::embroidery::is_loss`), `cmd/swatchlib.rs`'s `libraries` and `library`
  (`cmd/embroidery/threadlib.rs`), `i18n/mod.rs`'s `mod tests_embroidery`, `command_sweep.rs`'s
  `mod embroidery_common` and `cases.extend(…)`, and the `pub use` lines in `engine/src/lib.rs`.
- **The stitch preview's hooks in upstream files**, each marked "VectorCraft Stitch" in a comment
  (the fork's own code is under `crates/ui-egui/src/embroidery/`, `panels/embroidery/` and
  `cmd/embroidery/`):
  - `ui-egui/src/lib.rs`: `pub mod embroidery`, the app's `embroidery` field and its init,
    `stitch_preview::poll` after the background jobs, `canvas_controls` after the canvas (before
    upstream's isolation bar, which lies over the canvas's top: the switch and the notice move
    below it), `embroidery::end_frame` at the frame's end (a click on the counters, then
    `stitch_preview::schedule`), and the screenshot wait (`holds_screenshots`,
    `MAX_SCREENSHOT_WAIT_MS`);
  - `canvas.rs`: `art_pass` before the art, `paint_fabric` instead of the paper and the outline's
    ground, `templates`/`hidden` in the art's key and options, `&& art.render`, `&& !art.stitches`,
    `stitch_layer::draw` after the art, `to_screen_path` and `node_outline` made `pub(crate)`, the
    selection overlay's `frame_only`; `fit_zoom` and `FIT_MARGIN` split out of `fit` for View › Zoom
    to Box (`embroidery/zoom.rs`);
  - `chrome.rs`: the tab title's mode (`tab_title`'s fifth parameter, `stitches`, after upstream's
    `display_print_size`), and in `status_readouts` the counters (`embroidery::counters::show`)
    given the bar's area first: they take its right end and return the rest, and `diagnostics`
    waits on their answer (`timings &&`), so the timings give way to them. The bar is drawn with
    the app only read: the counters keep their state in cells and act on a click at the frame's
    end;
  - `menus.rs`: View › Stitch Preview and its options after Pixel Preview, the `checked` and
    `enabled` hooks, Window › Embroidery, the `UI_COMMANDS` rows (`ui.embroideryPanel`,
    `view.zoomTo`, `view.checkMarks`, `embroidery.showFinding`, `embroidery.nextHit`,
    `embroidery.previousHit`, `embroidery.copySettings`, `embroidery.pasteSettings`), the
    `view.zoomTo` arm (into `embroidery::zoom`), and `panels::embroidery::command` before the dock's
    commands;
  - `control.rs`: `ui.set {stitchPreview}`, `ui.render {stitchPreview}`, `ui.inspect`'s
    `stitchPreview`;
  - `panels/mod.rs`, `dock.rs`, `state.rs`, `icon_data.rs`: the panel's module, its show and menu
    arms, its height, its `ICON_PANELS` row and `UiState::embroidery_panel`, its icon
    (`assets/icons/dc-embroidery.svg`, a row in `ASSETS.md`);
  - `io.rs`'s `asks_options`: an embroidery format's save never asks for options (they are for
    commands alone); the engine's `fileio/save.rs`'s `format_options`: `unset_option` reports an
    embroidery format's options unset;
  - the engine's `lib.rs`: the `pub use` of `PlanKey` and `Planned`, and in
    `close_document` the closed document's planning forgotten (in a batch, by `fileio/batch.rs`
    once the batch holds; its `roll_back` forgets a document the batch opened);
  - `doc/src/lib.rs`: `Document::same_outside_layers` names the `embroidery` field (upstream's
    list of every field, for redrawing only what an edit changed);
  - `xtask/src/main.rs`: the `embroidery-corpus` step before `test`, and `cmd_test`'s `envs`, which
    the gate's test step passes `EMBROIDERY_CORPUS_REQUIRED` through;
  - for the fork's Rust 1.95 gate: `#[allow(unknown_lints)]` above `effects/src/group.rs`'s
    `clippy::manual_isolate_lowest_one`, and `#[allow(clippy::neg_cmp_op_on_partial_ord)]` on
    `brush/src/track.rs`'s `tracks`;
  - `render/src/lib.rs`: `pub mod lines`; `testkit/src/lib.rs`: `pub mod embroidery`;
    `vectorcraft-cli`'s `main.rs`: `bench` prints the preview's line;
  - `apps/vectorcraft/src/prefs_dir.rs`: `mod stitch_profile`; `profile.rs`'s `prefs_path`: the
    path passed through `stitch_profile::own` (the fork's settings folder).
- **The embroidery settings' hooks in upstream files** (the Embroidery panel's Object tab and
  Document Setup's Embroidery tab; the fork's own code is `panels/embroidery/object.rs` and
  `embroidery/rows.rs`, `fields.rs`, `vocabulary.rs`, `setup_tab.rs`), each marked "VectorCraft
  Stitch":
  - `ui-egui/src/widgets.rs`: `recessed_text`, `step_value` and `select_all_on_focus` made
    `pub(crate)` (their bodies as upstream has them), so that the embroidery fields
    (`embroidery/fields.rs`: `number_field`, `choice`, `Look`, `NumberFormat`) are drawn by
    upstream's own text box, stepping, scrub (`scrub::field`) and combo (`combo`). The fork's code
    relies on how they behave: on a sync, a change to `recessed_text`'s text placement (the look
    paints the value over it, in its place), to `combo`'s frame (`fields::combo_button` finds its
    button's id as `combo` does) or to how `step_value` and `scrub::field` count steps (the field
    hands them its value counted in its unit's steps) wants a look at `fields.rs`; its tests
    (`embroidery/tests_fields.rs`) compare it with upstream's field and fail on such a change.
  - `ui-egui/src/dialogs/document_setup.rs`: the Embroidery tab (`embroidery/setup_tab.rs`): its
    name in `TABS`, its fields added in `open`, its body as the last arm of `body`'s `match`, and
    in `confirm` the `document.setup` call alone replaced by `setup_tab::run` (which runs it, with
    the tab's changes in one `command.batch`), so upstream's own steps around it still run.
  - `menus.rs`: the `UI_COMMANDS` row `embroidery.documentSetup` (Embroidery Settings…).
- **Upstream's internals the fork reads without a hook** (nothing to resolve, but a sync that
  changes them breaks a feature quietly): Window › Embroidery's dock-width fit
  (`panels::embroidery::fit_dock_tabs`) reads the shared dock's area id
  `"vectorcraft-panel-docking"` (`panel_docking.rs`), craft-ui's tab ids under it (`("tab",
  panel)`), `dock::shared_dock_default_width` and `clamp_shared_dock_width`, and sets
  `UiState::dock_width` and `dock_width_pending`. If they change, the fit finds no tabs and does
  nothing; `the_docks_four_tabs_show_their_labels_whole` then fails.
- **The fork's app identity in upstream's packaging** (Decisions register, 2026-10-24), to keep
  when resolving: in `packaging/macos/Info.plist.in` the name, display name and identifier (the
  document types stay upstream's); `APP_NAME` in `packaging/macos/package.sh`; `APP` in
  `xtask/src/bundle.rs`, with the `bundle` line of `xtask/src/main.rs`'s usage and of the README's
  Quick start; in `packaging/windows/vectorcraft.wxs` the product name, publisher, upgrade code,
  install folder, registry key, shortcuts, descriptions and the two component GUIDs; in the
  workflows, `windows-arm64.yml`'s `app-dir` (the install test's folder) and `release.yml`'s
  `app-title`. Still
  upstream's, and open: Linux's app id (the desktop entry and its file names, the window's
  `with_app_id`, metainfo, MIME, Flatpak, FreeBSD); the DMG's Finder layout
  (`packaging/macos/dmg/DS_Store`, from `dmg/generate.py`), which still places `VectorCraft.app`
  on a volume named VectorCraft, so regenerate it before the fork ships a DMG; and Windows' App
  Paths entry for `vectorcraft.exe`, which both apps write (the last installed wins, and
  uninstalling either removes it).
- **The fork's text in upstream's docs**, to put back on upstream's own text when resolving: the
  pointer to this file at the top of `CLAUDE.md`; in `README.md` the fork's note and "Based on
  VectorCraft", with ArtCraft's marks and links left out (`docs/brand/LICENSE-brand.txt`); the
  embroidery rows of `ROADMAP.md`'s progress log and of `docs/file-format-parity.md` (its format
  counts, the embroidery row, its history), with each one's status line; in
  `docs/architecture.md` the `stitch` crate in the layer table and the embroidery paragraph under
  Rendering; in `docs/mcp.md` the thread palettes among the swatch libraries and the Embroidery
  section at its end; in `docs/control-protocol.md` the "Embroidery (VectorCraft Stitch)"
  paragraphs after Save PDF's. The fork's text goes in paragraphs of its own, never into upstream's
  rows (a row both sides extend conflicts on every sync).
- **The settings folder:** the fork keeps `ui.json` and the folders beside it (Swatches, Fonts,
  Data Recovery, logs and the rest) in `VectorCraft Stitch` (`vectorcraft-stitch` on Linux) beside
  upstream's `VectorCraft`, wherever upstream's docs name VectorCraft's folder; the first start
  copies VectorCraft's settings into it, all but its logs and Data Recovery copies
  (`apps/vectorcraft/src/stitch_profile.rs`, the project owner's choice). Upstream's own
  `VECTORCRAFT_CONFIG_DIR`, portable and DrawCraft folders are kept as they are. The reason:
  upstream's `panel_docking::valid` resets a dock layout holding a panel it doesn't know, so a
  shared `ui.json` would lose the person's layout in both apps once the Embroidery panel is docked.
  Upstream dropping the unknown ids instead of the layout would let them share it again (a proposal
  for upstream, not filed).
- **Files saved by upstream VectorCraft:** upstream keeps the document's `embroidery` settings it
  doesn't know (in `Document::extra`) but drops every object's (`NodeWire` keeps no unknown keys),
  so a design opened from a machine file, saved by upstream and opened here again, is art: its
  lines are stitched as thin strokes, and its Trim After marks are gone; so are every object's
  embroidery settings (its fills' and strokes' values, entry and exit points). No warning can say so.
  Upstream keeping a node's unknown keys would fix it (a proposal for upstream).

## Practicalities

- **Rust 1.95 is required:** `cargo +1.95.0 …`. The default toolchain on this machine may be older.
- **Use a separate `CARGO_TARGET_DIR` per agent**, and delete it when done (it grows to tens of GB).
- **The full gate is `cargo +1.95.0 xtask ci`.** It takes about 10 minutes on a 12-core machine
  (25 or more while other builds run), so run it in the background. It needs the `wasm32-unknown-unknown` target for 1.95
  (`rustup target add wasm32-unknown-unknown --toolchain 1.95.0`).
- **`.gitignore` ignores every directory named `plan/`** (meant for the local notes at the root):
  `crates/stitch/src/plan/` is let back in by name. A new directory called `plan` anywhere else
  would silently stay out of commits.
- **The real designs are part of the gate.** `cargo xtask ci` fetches them (only what is missing,
  each file checked against `xtask/embroidery-corpus.sha256`) and runs the tests with
  `EMBROIDERY_CORPUS_REQUIRED=1`, so the tests on them fail rather than pass without running. In a
  fresh checkout, copy a checked `corpus/embroidery` in or let the gate fetch it; offline, it fails
  and says how to fetch them (`EMBROIDERY_CORPUS_REQUIRED=0` runs the gate without them, said out
  loud). A plain `cargo test` skips an absent corpus with a note.
- **CI on GitHub: upstream's workflows, since the 2026-10-24 sync.** Every PR to the fork runs
  them: the gate (`cargo xtask ci` on the stable toolchain, with the real designs), Desktop
  printing, Windows ARM64 (builds the MSI, installs it into `Program Files\VectorCraft Stitch` and
  uninstalls it), Windows 7, FreeBSD, Nix, Packaging lint, SonarCloud and the Affinity 3 corpus.
  Read them with `gh pr checks <number> --repo przbat-public/vectorcraft-stitch`, and report what
  they say. The release workflow is upstream's too, and it needs signing secrets the fork
  doesn't have.
- **When a task lands:** update `docs/embroidery.md` (algorithms, limitations, test counts) and the
  PR description, and, in every progress doc the change touches (`ROADMAP.md`,
  `docs/file-format-parity.md`, `docs/architecture.md`, `docs/embroidery.md`…), its status line and
  its revision history, as `CLAUDE.md` asks. A decision the owner makes gets a row in the
  Decisions register above.
- **A change to what is sewn fails `crates/engine/tests/embroidery_output.rs`:** it lists the
  exports that changed, with the fields of each one's summary that moved (needle points, jumps and
  trims per block, size, longest and shortest stitch, squares covered), and prints the listings as
  they now stand. Paste the listing in the commit
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
