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
- **The owner squash-merges.** After a base PR merges, merge `feat/embroidery` into any branch
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
   3) are not followed, so VectorCraft still sews 40–55 % of the person's stitches. Per-fabric
   values wait for the fabric presets below.
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
   export plans afresh through it. Commands (so reachable over MCP): `embroidery.stats` (counts,
   size, stitch lengths, thread and sewing time, estimates on models whose numbers are named and
   sourced), `embroidery.sequence` (each unit with how it is sewn and why), and `embroidery.check`
   (Melco's design checker's thresholds and others, each rule with its sources, the format's
   limits; Melco's fill, satin and walk-stitch limits checked on each object's settings, as its
   checker reads them, the owner's choice).
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

Known limitations not on this list are in `docs/embroidery.md` › What is not stitched, and in the
PR descriptions.

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
| 2026-10-22 | The ArtCraft marks and the ArtCraft community links are removed from what the fork shows; "based on VectorCraft by the ArtCraft team" stays in plain text; the brand files stay in the repository, unshown | `docs/brand/LICENSE-brand.txt`: a published modified version must remove or replace the marks | keeping them, which the licence allows only a fork that proposes its changes back | `README.md`, the app's about and welcome texts |
| 2026-10-22 | A design opened from a machine file keeps its start point and the DST, TAP, T01, EXP, PES and PEC exports start from it; new art starts where the format expects (a corner); JEF and VP3 start every design at its centre; overwriting a file whose opening lost something asks first | an operator lines the needle up on a garment mark from the file's start; Janome and Husqvarna Viking machines put the start at the hoop's centre, so a start at a corner (as our DST, EXP, PES and PEC files have it) would put the design beside it | always starting at a corner; JEF and VP3 starting from the file's start too | the document's embroidery settings (`start`, `embroidery.setup`); `format::Frame`; the export's loss check |
| 2026-10-22 | Blocks join only on the exact art colour, not on the thread matched to it | the owner's choice: the art's colours are what the person drew | joining on the matched thread | `Sequence::target`; `docs/embroidery.md` › How a shape becomes stitches |
| 2026-10-22 | One vocabulary for the export's options and the planned settings on objects, before those exist; the options' earlier names stay as aliases; `underlay` takes a kind besides true and false | one name per setting in every place it is set or reported | two vocabularies mapped by hand | `cmd/embroidery/options.rs`; `docs/embroidery.md` › Architecture |
| 2026-10-22 | Keys are camelCase in commands and replies, snake_case in the `.vectorcraft` file (as every node and document key upstream writes); an object's embroidery reads as the document's settings do: unknown keys and values of another type are kept, nothing in it stops a file from opening | the file's own convention, and a file a newer version saves must open in an older one | camelCase in the file; a closed schema that drops what it doesn't know | `doc::embroidery::Embroidery` (`extra`, `unread`); `prop_format::any_embroidery_an_object_carries_opens_and_saves_back` |
| 2026-10-22 | The planner sews an object's appearance as it prints (stroke alignment, dashes, width profiles, brushes, arrowheads, live effects, a group's own fills and strokes, a symbol instance's staining); what still can't be sewn gets a warning | sew what prints | sewing the base path and warning | `Digitizer::shape` |

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

## Keeping up with upstream

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
- **One-line hooks the fork keeps in upstream files**, to keep when resolving: `fileio/load.rs`'s
  `is_loss` (`super::embroidery::is_loss`), `cmd/swatchlib.rs`'s `libraries` and `library`
  (`cmd/embroidery/threadlib.rs`), `i18n/mod.rs`'s `mod tests_embroidery`, `command_sweep.rs`'s
  `mod embroidery_common` and `cases.extend(…)`, and the `pub use` lines in `engine/src/lib.rs`.
- **Files saved by upstream VectorCraft:** upstream keeps the document's `embroidery` settings it
  doesn't know (in `Document::extra`) but drops every object's (`NodeWire` keeps no unknown keys),
  so a design opened from a machine file, saved by upstream and opened here again, is art: its
  lines are stitched as thin strokes, and its Trim After marks are gone. No warning can say so.
  Upstream keeping a node's unknown keys would fix it (a proposal for upstream).

## Practicalities

- **Rust 1.95 is required:** `cargo +1.95.0 …`. The default toolchain on this machine may be older.
- **Use a separate `CARGO_TARGET_DIR` per agent**, and delete it when done (it grows to tens of GB).
- **The full gate is `cargo +1.95.0 xtask ci`.** It takes over 10 minutes, so run it in the
  background. It needs the `wasm32-unknown-unknown` target for 1.95
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
- **The fork has no CI on GitHub:** say so when you report results, and don't claim checks passed
  there.
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
