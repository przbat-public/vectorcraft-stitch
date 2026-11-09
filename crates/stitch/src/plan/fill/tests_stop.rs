//! A fill stops where it asks whether to stop (a plan that is cancelled): before each outline's
//! edge run, every [`STOP_ROWS`] rows a layer finds, before each piece of regions, each order of
//! them tried, each region sewn and each step of a walk round them, before each part of a fill
//! sewn in parts and, past a few parts, before each part their order picks; a satin before each of
//! its lines. Within the steps that take longest too: a travel's search and its graph every few
//! corners, the graph's simplification of an outline, an outline's inset wherever it is made, a
//! layer's matches of spans, and a satin line as it resamples and works out its crossings. Each
//! ask is counted here, so that every one of them is pinned, and a plan asked to stop from the
//! start plans nothing.

use std::cell::Cell;

use vectorcraft_geom::{BezPath, FillRule};

use super::super::area::{MAX_TRAVEL_NODES, STOP_SETTLED, TRAVEL_SIMPLIFY, simplify};
use super::super::geometry::{STOP_SAMPLES, resample_by_until};
use super::super::satin::satin;
use super::super::{Aim, Params, Style, Underlay, try_fill_with_columns_measured_until, try_stitches_aimed_until};
use super::*;

/// A closed rectangle from `(x0, y0)` to `(x1, y1)`.
fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to((x0, y0));
    p.line_to((x1, y0));
    p.line_to((x1, y1));
    p.line_to((x0, y1));
    p.close_path();
    p
}

/// A square 40 mm a side with `n` × `n` square holes of 2 mm, 6 mm apart: one outline and one a
/// hole, filled even-odd.
fn holed(n: usize) -> BezPath {
    let mut p = rect(0.0, 0.0, 40.0, 40.0);
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (4.0 + 6.0 * i as f64, 4.0 + 6.0 * j as f64);
            p.extend(rect(x, y, x + 2.0, y + 2.0).elements().iter().copied());
        }
    }
    p
}

/// A comb: a spine 4 mm wide with `teeth` teeth 2 mm wide and 12 mm long, 4 mm apart. Rows across
/// the teeth (at 0°) break into a region a tooth, linked to the spine's.
fn comb(teeth: usize) -> BezPath {
    let mut p = BezPath::new();
    p.move_to((0.0, 0.0));
    for k in 0..teeth {
        let x = 4.0 * k as f64;
        p.line_to((x, 16.0));
        p.line_to((x + 2.0, 16.0));
        p.line_to((x + 2.0, 4.0));
        p.line_to((x + 4.0, 4.0));
    }
    let end = 4.0 * teeth as f64;
    p.line_to((end, 0.0));
    p.close_path();
    p
}

/// A regular polygon of `sides` corners on a circle of `radius` millimetres round the origin.
fn polygon(sides: usize, radius: f64) -> BezPath {
    let mut p = BezPath::new();
    for k in 0..sides {
        let turn = std::f64::consts::TAU * k as f64 / sides as f64;
        let corner = (radius * turn.cos(), radius * turn.sin());
        if k == 0 {
            p.move_to(corner);
        } else {
            p.line_to(corner);
        }
    }
    p.close_path();
    p
}

/// A counter of the asks that never says stop.
#[derive(Default)]
struct Asks(Cell<usize>);

impl Asks {
    fn ask(&self) -> bool {
        self.0.set(self.0.get() + 1);
        false
    }

    fn count(&self) -> usize {
        self.0.get()
    }
}

/// The shape of `path` (its outlines, `rule`), an edge grid left out.
fn area_of(subs: &[SubPath], rule: FillRule) -> (Vec<&SubPath>, FillRule) {
    (outlines(subs), rule)
}

/// The edge run asks before each outline: once a hole and once for the outside; asked to stop, it
/// sews none.
#[test]
fn an_edge_run_asks_before_each_outline() {
    let p = Params::for_style(Style::Fill).sane();
    // Nine holes: the travels between their runs settle fewer corners than a search asks after
    // ([`STOP_SETTLED`]), so every ask here is an outline's.
    let subs = closed_subpaths(&holed(3));
    let (outlines, rule) = area_of(&subs, FillRule::EvenOdd);
    let area = Area { outlines: &outlines, rule, grid: None };
    let asks = Asks::default();
    let ask = || asks.ask();
    let mut travels = Travels::stopping(area, &ask);
    let mut runs = Runs::from(None, &p);
    edge_runs(area, &mut travels, &p, None, &mut runs, &|_| 0.0);
    assert_eq!(asks.count(), 10, "a square and its 9 holes");
    assert!(!runs.is_empty());
    let mut travels = Travels::stopping(area, &|| true);
    let mut runs = Runs::from(None, &p);
    edge_runs(area, &mut travels, &p, None, &mut runs, &|_| 0.0);
    assert!(runs.is_empty(), "an edge run sewn though stopped");
}

/// A layer asks every [`STOP_ROWS`] rows it finds; asked to stop, it finds none.
#[test]
fn a_layer_asks_every_few_hundred_rows() {
    let p = Params { angle: 0.0, ..Params::for_style(Style::Fill) }.sane();
    let subs = closed_subpaths(&rect(0.0, 0.0, 20.0, 300.0));
    let (outlines, rule) = area_of(&subs, FillRule::NonZero);
    let area = Area { outlines: &outlines, rule, grid: None };
    let layer = Layer { axes: Axes::new(p.angle), spacing: p.density, inset: 0.0 };
    let Some((lo, hi)) = area.extent(layer.axes.across) else { panic!("an extent") };
    let asks = Asks::default();
    let regions = layer.regions(area, lo, hi, &p, None, &|| asks.ask());
    let rows: usize = regions.list.iter().map(|r| r.rows.len()).sum();
    assert!(rows > 3 * STOP_ROWS, "{rows} rows");
    assert_eq!(asks.count(), rows.div_ceil(STOP_ROWS));
    assert!(layer.regions(area, lo, hi, &p, None, &|| true).list.is_empty(), "rows found though stopped");
}

/// A layer whose rows have many spans asks as soon as [`STOP_PAIRS`] matches of them with the row
/// before's have been made since its last ask: across a thousand bars, a row's thousand spans are a
/// million matches, so it asks before every row.
#[test]
fn a_layer_asks_after_a_million_matches_of_spans() {
    /// Bars side by side, each a span of every row, and their size (millimetres).
    const BARS: usize = 1_000;
    const WIDE: f64 = 0.5;
    const APART: f64 = 1.0;
    const HIGH: f64 = 20.0;
    const { assert!(BARS * BARS >= STOP_PAIRS) };
    let p = Params { angle: 0.0, ..Params::for_style(Style::Fill) }.sane();
    let mut bars = BezPath::new();
    for k in 0..BARS {
        let x = APART * k as f64;
        bars.extend(rect(x, 0.0, x + WIDE, HIGH).elements().iter().copied());
    }
    let subs = closed_subpaths(&bars);
    let (outlines, rule) = area_of(&subs, FillRule::NonZero);
    let area = Area { outlines: &outlines, rule, grid: None };
    let layer = Layer { axes: Axes::new(p.angle), spacing: p.density, inset: 0.0 };
    let Some((lo, hi)) = area.extent(layer.axes.across) else { panic!("an extent") };
    let asks = Asks::default();
    let regions = layer.regions(area, lo, hi, &p, None, &|| asks.ask());
    let rows = regions.list.iter().flat_map(|r| r.rows.iter().map(|w| w.index)).max().map_or(0, |last| last + 1);
    assert!(rows > 1 && rows < STOP_ROWS, "{rows} rows");
    assert_eq!(asks.count(), rows, "before every row: the first as every few hundred rows are, the others after a million matches");
}

/// Sewing a layer's regions asks before its piece, before the order tried and before each region;
/// trying orders, before each step of each walk round the regions too (each region reached and
/// left: twice a region but the walk's first); asked to stop, it sews nothing.
#[test]
fn sewing_regions_asks_before_each_piece_order_and_region() {
    let p = Params { angle: 0.0, underlay: Underlay::None, ..Params::for_style(Style::Fill) }.sane();
    let subs = closed_subpaths(&comb(6));
    let (outlines, rule) = area_of(&subs, FillRule::NonZero);
    let area = Area { outlines: &outlines, rule, grid: None };
    let layer = Layer { axes: Axes::new(p.angle), spacing: p.density, inset: 0.0 };
    let Some((lo, hi)) = area.extent(layer.axes.across) else { panic!("an extent") };
    let regions = layer.regions(area, lo, hi, &p, None, &|| false);
    let count = regions.list.len();
    assert!(count >= 7, "the spine and its teeth: {count} regions");
    for search in [false, true] {
        let asks = Asks::default();
        let ask = || asks.ask();
        let mut travels = Travels::stopping(area, &ask);
        let mut runs = Runs::from(None, &p);
        sew(&regions, &mut travels, MAX_SEEN, search, &p, &[], None, &mut runs, &|_| 0.0);
        let piece: Vec<usize> = (0..count).collect();
        let walks = if search { roots(&piece, &regions, &[]).len() } else { 0 };
        // The travels' graph, built once round the spine, looks at every corner of the comb.
        let graph = outlines.iter().map(|o| o.pts.len()).sum::<usize>() / STOP_SETTLED;
        // The piece, the nearest-first order and each of its regions, then each walk's order and
        // its steps, and the graph.
        assert_eq!(asks.count(), 1 + (1 + count) + walks * (1 + 2 * count - 1) + graph, "searching orders: {search}");
        assert!(!runs.is_empty());
    }
    let mut travels = Travels::stopping(area, &|| true);
    let mut runs = Runs::from(None, &p);
    sew(&regions, &mut travels, MAX_SEEN, true, &p, &[], None, &mut runs, &|_| 0.0);
    assert!(runs.is_empty(), "regions sewn though stopped");
}

/// A fill sewn in parts asks before each part: three bars, each a narrow part sewn as satin, and no
/// rest; asked to stop, it sews none.
#[test]
fn a_fill_of_parts_asks_before_each_part() {
    let (fill, satin_params) = (Params::for_style(Style::Fill), Params::for_style(Style::Satin));
    let mut bars = BezPath::new();
    for k in 0..3 {
        let y = 10.0 * f64::from(k);
        bars.extend(rect(0.0, y, 30.0, y + 3.0).elements().iter().copied());
    }
    let columns = super::super::narrow::columns(&bars, FillRule::NonZero, fill.narrow);
    assert_eq!(columns.len(), 3, "a narrow part a bar");
    let asks = Asks::default();
    let sewn =
        fill_with_columns_measured(&bars, FillRule::NonZero, &BezPath::new(), &columns, &fill, &satin_params, Aim::from_needle(None), None, &|| {
            asks.ask()
        });
    assert_eq!(asks.count(), 3);
    assert!(!sewn.is_empty());
    let stopped =
        fill_with_columns_measured(&bars, FillRule::NonZero, &BezPath::new(), &columns, &fill, &satin_params, Aim::from_needle(None), None, &|| true);
    assert!(stopped.iter().all(|s| s.command != crate::Command::Stitch), "parts sewn though stopped");
}

/// The order of more parts than every order is weighed for asks before each part it picks, the
/// nearest next; asked to stop, it ends there.
#[test]
fn the_order_of_many_parts_asks_before_each_part_it_picks() {
    /// Parts in a row, 10 mm apart, each touched at one point: more than [`MAX_ORDERED`].
    const PARTS: usize = 10;
    const APART_MM: f64 = 10.0;
    /// The asks after which the second order is told to stop.
    const STOP_AFTER: usize = 3;
    const { assert!(PARTS > MAX_ORDERED) };
    let contacts: Vec<Vec<Point>> = (0..PARTS).map(|k| vec![Point::new(APART_MM * k as f64, 0.0)]).collect();
    let asks = Asks::default();
    let whole = Order { contacts: &contacts, through: &|_| false, exit: None, first: None, stopped: &|| asks.ask() }.best(None);
    assert_eq!(whole, (0..PARTS).collect::<Vec<_>>(), "the nearest part next, along the row");
    assert_eq!(asks.count(), PARTS, "once before each part picked");
    let asked = Cell::new(0usize);
    let later = || {
        asked.set(asked.get() + 1);
        asked.get() > STOP_AFTER
    };
    let cut = Order { contacts: &contacts, through: &|_| false, exit: None, first: None, stopped: &later }.best(None);
    assert_eq!(cut.len(), STOP_AFTER, "the order ends where it is told to stop");
}

/// A travel's search asks every [`STOP_SETTLED`] corners it settles, and its graph every as many
/// corners it looks at as it is built. From the tip of a comb's first tooth to its last's the
/// graph looks at every corner of the comb, and the search settles every corner at the teeth's
/// feet (two a gap between teeth, and one where the last tooth meets the spine's end) and both
/// ends: asked once at each multiple of them settled before the last. Told to stop, it gives up,
/// and the "no way" it found then is not kept.
#[test]
fn a_travel_search_asks_every_few_corners_it_settles() {
    /// Teeth enough for more corners than the search settles between two asks.
    const TEETH: usize = 24;
    /// The corners at the teeth's feet: two at each gap between teeth, and the last tooth's.
    const CORNERS: usize = 2 * (TEETH - 1) + 1;
    /// Where the travel starts and ends: a millimetre in from the first and the last tooth's tips.
    const TIP_Y: f64 = 15.0;
    const PITCH: f64 = 4.0;
    const IN: f64 = 1.0;
    let subs = closed_subpaths(&comb(TEETH));
    let (outlines, rule) = area_of(&subs, FillRule::NonZero);
    let area = Area { outlines: &outlines, rule, grid: None };
    let (a, b) = (Point::new(IN, TIP_Y), Point::new(PITCH * (TEETH - 1) as f64 + IN, TIP_Y));
    let asks = Asks::default();
    let ask = || asks.ask();
    let mut travels = Travels::stopping(area, &ask);
    let way = travels.way(a, b);
    assert!(way.as_ref().is_some_and(|w| w.len() > 3), "a way down the first tooth, along the spine and up the last: {way:?}");
    // The graph's corners looked at; each corner and both ends settled, the last ask at the last
    // multiple before the end.
    let looked: usize = outlines.iter().map(|o| o.pts.len()).sum();
    assert_eq!(asks.count(), looked / STOP_SETTLED + (CORNERS + 1) / STOP_SETTLED);
    let first_only = Cell::new(true);
    let once = || first_only.replace(false);
    let mut travels = Travels::stopping(area, &once);
    assert_eq!(travels.way(a, b), None, "a search stopped at its first ask finds no way");
    assert_eq!(travels.way(a, b), way, "and that is not kept: the next search finds it");
}

/// A travel's graph on an outline of more corners than it may have asks every [`STOP_SETTLED`]
/// corners it looks at, and stops looking once it has found more than [`MAX_TRAVEL_NODES`]: then
/// there is no graph, and no way round. A comb's outline has four corners a tooth, the two at its
/// foot reflex, after the spine's corner it starts at: the reflex corner past the bound is the
/// first of the next tooth's foot. On a comb of a thousand teeth it asks as it simplifies the
/// outline first, as often as simplifying it alone does, and then at the same corners. Told to stop
/// as the graph is built, the search gives up, and the "no way" it found then is not kept.
#[test]
fn a_travel_graph_asks_as_it_looks_at_many_corners() {
    /// Teeth enough for more reflex corners than a graph may have.
    const TEETH: usize = MAX_TRAVEL_NODES;
    const CORNERS_A_TOOTH: usize = 4;
    const REFLEX_A_TOOTH: usize = 2;
    /// The corners looked at when the one past the bound is found: the start, every corner of the
    /// teeth whose feet hold the bound's reflex corners, and the next tooth's two tips and foot.
    const FOUND_AT: usize = 1 + CORNERS_A_TOOTH * (MAX_TRAVEL_NODES / REFLEX_A_TOOTH) + 3;
    const TIP_Y: f64 = 15.0;
    const PITCH: f64 = 4.0;
    const IN: f64 = 1.0;
    let subs = closed_subpaths(&comb(TEETH));
    let (outlines, rule) = area_of(&subs, FillRule::NonZero);
    let area = Area { outlines: &outlines, rule, grid: None };
    let (a, b) = (Point::new(IN, TIP_Y), Point::new(PITCH * (TEETH - 1) as f64 + IN, TIP_Y));
    let asks = Asks::default();
    let ask = || asks.ask();
    let mut travels = Travels::stopping(area, &ask);
    assert_eq!(travels.way(a, b), None, "too many corners for a graph: no way round");
    assert_eq!(asks.count(), FOUND_AT / STOP_SETTLED, "every few corners looked at");
    // Teeth enough for an outline whose simplification asks: its asks first, then the same corners'.
    const MANY: usize = 1_000;
    let subs = closed_subpaths(&comb(MANY));
    let Some(sub) = subs.first() else { panic!("the comb's outline") };
    let simplifying = Asks::default();
    assert!(simplify(&sub.pts, TRAVEL_SIMPLIFY, &|| simplifying.ask()).is_some(), "the comb's outline simplified");
    assert!(simplifying.count() > 0, "a simplification that asks");
    let (outlines, rule) = area_of(&subs, FillRule::NonZero);
    let area = Area { outlines: &outlines, rule, grid: None };
    let b = Point::new(PITCH * (MANY - 1) as f64 + IN, TIP_Y);
    let asks = Asks::default();
    let ask = || asks.ask();
    let mut travels = Travels::stopping(area, &ask);
    assert_eq!(travels.way(a, b), None, "too many corners for a graph: no way round");
    assert_eq!(asks.count(), simplifying.count() + FOUND_AT / STOP_SETTLED, "as it simplifies, then every few corners looked at");
    // As many teeth as a graph may have reflex corners for: built, with a way round.
    const FEWER: usize = MAX_TRAVEL_NODES / REFLEX_A_TOOTH;
    let subs = closed_subpaths(&comb(FEWER));
    let (outlines, rule) = area_of(&subs, FillRule::NonZero);
    let area = Area { outlines: &outlines, rule, grid: None };
    let b = Point::new(PITCH * (FEWER - 1) as f64 + IN, TIP_Y);
    let way = Travels::new(area).way(a, b);
    assert!(way.is_some(), "a way round the spine");
    let first_only = Cell::new(true);
    let once = || first_only.replace(false);
    let mut travels = Travels::stopping(area, &once);
    assert_eq!(travels.way(a, b), None, "a graph stopped as it is built gives no way");
    assert_eq!(travels.way(a, b), way, "and that is not kept: the next search builds it");
}

/// Simplifying an outline for a travel's graph asks every [`STOP_SCANNED`] points it looks at: a
/// comb's outline of a thousand teeth splits near one end each time, so it looks at its points
/// millions of times; told to stop, it gives no outline.
#[test]
fn simplifying_an_outline_asks_every_million_points_it_looks_at() {
    /// Teeth enough for millions of points looked at: four corners a tooth.
    const TEETH: usize = 1_000;
    let subs = closed_subpaths(&comb(TEETH));
    let Some(sub) = subs.first() else { panic!("the comb's outline") };
    let asks = Asks::default();
    let simple = simplify(&sub.pts, TRAVEL_SIMPLIFY, &|| asks.ask());
    assert!(simple.is_some_and(|s| s.len() > 3 * TEETH), "the comb's corners kept");
    assert!(asks.count() >= 2, "{} asks within one outline", asks.count());
    assert_eq!(simplify(&sub.pts, TRAVEL_SIMPLIFY, &|| true), None, "an outline simplified though stopped");
}

/// An edge run's inset of one outline asks every [`STOP_CORNERS`] corners of each of its two
/// passes; told to stop, it is cut short.
#[test]
fn an_inset_asks_every_few_hundred_corners() {
    /// Teeth enough for a few hundred corners, four a tooth.
    const TEETH: usize = 100;
    let p = Params::for_style(Style::Fill).sane();
    let subs = closed_subpaths(&comb(TEETH));
    let (outlines, rule) = area_of(&subs, FillRule::NonZero);
    let area = Area { outlines: &outlines, rule, grid: None };
    let Some(sub) = outlines.first() else { panic!("the comb's outline") };
    let corners = sub.pts.len();
    assert!(corners > 3 * STOP_CORNERS, "{corners} corners");
    let d = p.density * EDGE_RUN_INSET;
    let asks = Asks::default();
    let inset = inset_outline(sub, area, d, &|| asks.ask());
    assert_eq!(inset.len(), corners);
    assert_eq!(asks.count(), 2 * ((corners - 1) / STOP_CORNERS), "every few hundred corners, in both passes");
    assert!(inset_outline(sub, area, d, &|| true).len() < corners, "an inset made whole though stopped");
}

/// Each inset asks so wherever it is made. An edge run asks as it insets each outline, besides once
/// before it; from an entry, as it insets the outline the entry is on first, and not again when it
/// sews that one. A fill with no underlay asks so as it finds where its entry goes in: on a regular
/// polygon of a few hundred corners, whose rows are one region reached in a straight line, the
/// entry adds just its inset's asks.
#[test]
fn every_inset_asks_where_it_is_made() {
    /// Teeth enough for a few hundred corners, four a tooth.
    const TEETH: usize = 100;
    /// Where the needle goes in: a millimetre in from the comb's first corner, in its spine.
    const ENTRY_MM: f64 = 1.0;
    /// A polygon's corners, more than a few hundred, and its size (millimetres).
    const SIDES: usize = 600;
    const RADIUS: f64 = 10.0;
    let p = Params::for_style(Style::Fill).sane();
    let subs = closed_subpaths(&comb(TEETH));
    let (outlines, rule) = area_of(&subs, FillRule::NonZero);
    let area = Area { outlines: &outlines, rule, grid: None };
    let Some(sub) = outlines.first() else { panic!("the comb's outline") };
    let inset = 2 * ((sub.pts.len() - 1) / STOP_CORNERS);
    assert!(inset > 0, "an inset that asks");
    for entry in [None, Some(Point::new(ENTRY_MM, ENTRY_MM))] {
        let asks = Asks::default();
        let ask = || asks.ask();
        let mut travels = Travels::stopping(area, &ask);
        let mut runs = Runs::from(None, &p);
        edge_runs(area, &mut travels, &p, entry, &mut runs, &|_| 0.0);
        assert!(!runs.is_empty(), "the comb's edge run");
        assert_eq!(asks.count(), 1 + inset, "before the comb's outline and as it is inset, from {entry:?}");
    }
    let none = Params { underlay: Underlay::None, ..p };
    let subs = closed_subpaths(&polygon(SIDES, RADIUS));
    let (outlines, rule) = area_of(&subs, FillRule::NonZero);
    let area = Area { outlines: &outlines, rule, grid: None };
    let asked = |entry: Option<Point>| {
        let asks = Asks::default();
        let ask = || asks.ask();
        let mut travels = Travels::stopping(area, &ask);
        let mut runs = Runs::from(None, &none);
        sew_area(area, &mut travels, &none, entry, None, &[], &mut runs, &|_| 0.0);
        assert!(!runs.is_empty(), "the polygon's rows, from {entry:?}");
        asks.count()
    };
    assert_eq!(asked(Some(Point::ZERO)), asked(None) + 2 * ((SIDES - 1) / STOP_CORNERS), "the entry's inset asks");
}

/// Resampling a line asks every [`STOP_SAMPLES`] points it makes; told to stop, it returns what it
/// made. A straight line a quarter of a millimetre's step long `STEPS` steps and a half: `STEPS`
/// points made along it after its first, all exact in binary.
#[test]
fn resampling_asks_every_few_thousand_points() {
    const STEP: f64 = 0.25;
    const STEPS: usize = 3 * STOP_SAMPLES;
    const HALF: f64 = 0.5;
    let end = STEP * (STEPS as f64 + HALF);
    let line = [Point::new(0.0, 0.0), Point::new(end, 0.0)];
    let asks = Asks::default();
    let points = resample_by_until(&line, false, &|_| STEP, &|| asks.ask());
    assert!(points.len() > STEPS, "{} points", points.len());
    // Asked as the points made reach each multiple: the first, then those along it.
    assert_eq!(asks.count(), (STEPS + 1) / STOP_SAMPLES);
    let cut = resample_by_until(&line, false, &|_| STEP, &|| true);
    assert_eq!(cut.len(), STOP_SAMPLES, "made up to the first ask, then stopped");
}

/// A satin line asks within itself too, as it resamples its middle and as it works out its
/// crossings: a long one asks every few thousand crossings of each, and told to stop at its first
/// ask within the line, it is cut short there.
#[test]
fn a_long_satin_line_asks_within_itself() {
    /// A straight line long enough for many asks' worth of crossings (millimetres).
    const LENGTH: f64 = 10_000.0;
    let p = Params { width: 3.0, underlay: Underlay::None, ..Params::for_style(Style::Satin) };
    let mut line = BezPath::new();
    line.move_to((0.0, 0.0));
    line.line_to((LENGTH, 0.0));
    let asks = Asks::default();
    let (whole, _) = satin(&line, &p, Aim::from_needle(None), &|| asks.ask());
    // Its crossings: about the line's length over half the spacing.
    let sane = p.sane();
    let crossings = (LENGTH / (sane.column_spacing(sane.width) / 2.0)) as usize;
    assert!(crossings > 4 * STOP_SAMPLES, "{crossings} crossings");
    let within = crossings / STOP_SAMPLES - 1;
    // Once before the line, and more than once for each of its resampling and its crossings.
    assert!(asks.count() > 2 * within, "{} asks for {crossings} crossings: within its resampling and its crossings", asks.count());
    let asked = Cell::new(0usize);
    let after_the_first = || {
        asked.set(asked.get() + 1);
        asked.get() > 1
    };
    let (cut, _) = satin(&line, &p, Aim::from_needle(None), &after_the_first);
    assert!(cut.len() < whole.len() / 2, "{} of {} stitches though stopped within the line", cut.len(), whole.len());
}

/// Five lines 30 mm long, 10 mm apart.
fn lines() -> BezPath {
    let mut lines = BezPath::new();
    for k in 0..5 {
        let y = 10.0 * f64::from(k);
        lines.move_to((0.0, y));
        lines.line_to((30.0, y));
    }
    lines
}

/// A satin asks before each of its lines; asked to stop, it sews none.
#[test]
fn a_satin_asks_before_each_line() {
    let p = Params { width: 3.0, ..Params::for_style(Style::Satin) };
    let lines = lines();
    let asks = Asks::default();
    let (sewn, _) = satin(&lines, &p, Aim::from_needle(None), &|| asks.ask());
    assert_eq!(asks.count(), 5);
    assert!(!sewn.is_empty());
    let (stopped, _) = satin(&lines, &p, Aim::from_needle(None), &|| true);
    assert!(stopped.is_empty(), "lines sewn though stopped");
}

/// A plan stopped partway is no plan: a fill, a satin of five lines and a fill of parts asked to
/// stop at once, or after a few asks, give nothing, and never asked to, their stitches.
#[test]
fn a_plan_stopped_partway_is_none() {
    let fill = Params::for_style(Style::Fill);
    let satin_params = Params { width: 3.0, ..Params::for_style(Style::Satin) };
    let (shape, lines) = (holed(5), lines());
    let later = |after: usize| {
        let asked = Cell::new(0usize);
        move || {
            asked.set(asked.get() + 1);
            asked.get() > after
        }
    };
    for (path, p) in [(&shape, &fill), (&lines, &satin_params)] {
        let plan = |stopped: &dyn Fn() -> bool| try_stitches_aimed_until(path, FillRule::EvenOdd, p, Aim::from_needle(None), stopped).unwrap();
        assert!(plan(&|| true).is_none(), "{:?} planned though stopped at once", p.style);
        assert!(plan(&later(3)).is_none(), "{:?} planned though stopped partway", p.style);
        assert!(plan(&|| false).is_some_and(|s| !s.is_empty()), "{:?}", p.style);
    }
    let columns = super::super::narrow::columns(&shape, FillRule::EvenOdd, fill.narrow);
    let plan = |stopped: &dyn Fn() -> bool| {
        try_fill_with_columns_measured_until(&shape, FillRule::EvenOdd, &shape, &columns, &fill, &satin_params, Aim::from_needle(None), None, stopped)
            .unwrap()
    };
    assert!(plan(&|| true).is_none() && plan(&later(3)).is_none());
    assert!(plan(&|| false).is_some_and(|s| !s.is_empty()));
}
