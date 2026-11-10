//! A fill stops where it asks whether to stop (a plan that is cancelled): before each outline's
//! edge run, every [`STOP_ROWS`] rows a layer finds, before each piece of regions, each order of
//! them tried, each region sewn and each step of a walk round them, before each part of a fill
//! sewn in parts and, past a few parts, before each part their order picks; a satin before each of
//! its lines. Each ask is counted here, so that every one of them is pinned, and a plan asked to
//! stop from the start plans nothing.

use std::cell::Cell;

use vectorcraft_geom::{BezPath, FillRule};

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
    let subs = closed_subpaths(&holed(5));
    let (outlines, rule) = area_of(&subs, FillRule::EvenOdd);
    let area = Area { outlines: &outlines, rule, grid: None };
    let asks = Asks::default();
    let ask = || asks.ask();
    let mut travels = Travels::stopping(area, &ask);
    let mut runs = Runs::from(None, &p);
    edge_runs(area, &mut travels, &p, None, &mut runs, &|_| 0.0);
    assert_eq!(asks.count(), 26, "a square and its 25 holes");
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
        // The piece, the nearest-first order and each of its regions, then each walk's order and
        // its steps.
        assert_eq!(asks.count(), 1 + (1 + count) + walks * (1 + 2 * count - 1), "searching orders: {search}");
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
