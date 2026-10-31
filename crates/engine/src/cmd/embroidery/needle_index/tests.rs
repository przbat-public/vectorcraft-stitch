//! The needle points stitch editing works on ([`super::NeedleIndex`]): what it holds and refuses,
//! its queries against brute force, its record numbers against `embroidery.stitches`, and that an
//! edit builds again only the paths it changed.

use std::collections::BTreeSet;

use proptest::prelude::*;
use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_geom::{Point, Rect};
use vectorcraft_testkit::embroidery as fx;
use vectorcraft_tools::embroidery::{NeedleRef, PlanFacts};

use super::super::needles;
use super::super::test_support::{PT_PER_MM, run};
use super::{Area, Lasso, NeedleIndex, paths_built};
use crate::Session;

fn runner(s: &mut Session) -> impl FnMut(&str, Value) -> Value + '_ {
    |c, p| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// The index of everything in the active document's layers.
fn whole(s: &Session) -> NeedleIndex {
    let doc = s.doc().unwrap().doc.clone();
    let roots: Vec<NodeId> = doc.layers.iter().map(|l| l.id).collect();
    NeedleIndex::of(&doc, &roots, None).expect("paths of stitches")
}

/// Every needle point of path `id`, as the stitch commands read it.
fn read_points(s: &Session, id: NodeId) -> Vec<Point> {
    let st = needles::read(s.doc().unwrap().doc.node(id).unwrap()).unwrap();
    st.runs.iter().flat_map(|r| r.needles.iter().map(|n| n.p)).collect()
}

/// The fixture's every kind of run: each editable path holds the needle points the stitch commands
/// read, at the same places, in the same order; the locked and the hidden runs are refused (counted),
/// and a run in a symbol's art isn't in the layers' scope at all.
#[test]
fn it_holds_the_needle_points_the_stitch_commands_edit() {
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    let index = whole(&s);
    let doc = s.doc().unwrap().doc.clone();
    assert!(!index.holds(NodeId(m.locked)) && !index.holds(NodeId(m.hidden)), "locked and hidden runs aren't edited");
    assert!(index.scope().refused >= 2, "{:?}", index.scope());
    let mut total = 0;
    for id in index.ids().collect::<Vec<_>>() {
        let points = read_points(&s, id);
        assert_eq!(index.count(id), points.len(), "{id:?}");
        for (k, p) in points.iter().enumerate() {
            assert_eq!(index.point(NeedleRef { id, index: k }), Some(*p), "{id:?} needle point {k}");
            for a in index.anchors(NeedleRef { id, index: k }) {
                assert_eq!(index.of_anchor(id, a), Some(k), "{id:?} anchor {a:?}");
            }
        }
        total += points.len();
        assert!(doc.is_editable(id));
    }
    assert_eq!(index.scope().needles, total);
    // A run of one needle point is two anchors at one spot: both are that needle point.
    let one = NeedleRef { id: NodeId(m.one_point), index: 0 };
    assert_eq!(index.anchors(one), vec![(0, 0), (0, 1)]);
    assert_eq!(index.count(NodeId(m.one_point)), 1);
    assert_eq!(index.count(NodeId(m.double_point)), 2, "a tack is two needle points at one spot");
}

/// The nearest needle point and the nearest stitch, against brute force over every needle point and
/// stitch of the fixture, at points near and far; a miss is `None`; points that aren't numbers find
/// nothing.
#[test]
fn queries_find_what_brute_force_finds() {
    let mut s = Session::new();
    fx::manual(&mut runner(&mut s));
    let index = whole(&s);
    let ids: Vec<NodeId> = index.ids().collect();
    let all: Vec<(NeedleRef, Point)> =
        ids.iter().flat_map(|&id| read_points(&s, id).into_iter().enumerate().map(move |(k, p)| (NeedleRef { id, index: k }, p))).collect();
    let tol = 2.0;
    for &(n, p) in all.iter().step_by(7) {
        for at in [p, p + vectorcraft_geom::Vec2::new(0.7, -0.4), p + vectorcraft_geom::Vec2::new(5.0, 5.0)] {
            let near = all.iter().filter(|(_, q)| q.distance(at) <= tol).map(|(_, q)| q.distance(at)).fold(f64::INFINITY, f64::min);
            match index.needle_at(at, tol) {
                Some(h) => assert!((h.point.distance(at) - near).abs() < 1e-9, "{n:?} at {at:?}: {h:?}, nearest {near}"),
                None => assert!(near.is_infinite(), "{n:?} at {at:?}: missed one {near} away"),
            }
        }
    }
    assert_eq!(index.needle_at(Point::new(f64::NAN, 0.0), tol), None);
    assert_eq!(index.needle_at(Point::new(0.0, 0.0), f64::INFINITY), None);
    // The nearest stitch: as near as the nearest of every stitch between two needle points of a run.
    let segments: Vec<(Point, Point)> = ids
        .iter()
        .flat_map(|&id| {
            let st = needles::read(s.doc().unwrap().doc.node(id).unwrap()).unwrap();
            st.runs.into_iter().flat_map(|r| r.needles.windows(2).map(|w| (w[0].p, w[1].p)).collect::<Vec<_>>())
        })
        .collect();
    let off = |a: Point, b: Point, p: Point| super::nearest_on(a, b, p).distance(p);
    for &(a, b) in segments.iter().step_by(11) {
        let at = a.midpoint(b) + vectorcraft_geom::Vec2::new(0.3, 0.4);
        let near = segments.iter().map(|&(a, b)| off(a, b, at)).filter(|d| *d <= tol).fold(f64::INFINITY, f64::min);
        let hit = index.thread_at(at, tol).expect("a stitch");
        assert!((hit.point.distance(at) - near).abs() < 1e-9, "{hit:?}: nearest {near}");
        assert!((off(hit.ends[0], hit.ends[1], at) - near).abs() < 1e-9, "the place lies on the stitch: {hit:?}");
    }
    assert_eq!(index.thread_at(Point::new(-5000.0, -5000.0), 1.0), None);
}

/// `[` and `]`: along a path, on to the next and back, round from the last to the first.
#[test]
fn steps_go_along_the_thread_and_round() {
    let mut s = Session::new();
    fx::manual(&mut runner(&mut s));
    let index = whole(&s);
    let first = index.first().unwrap();
    let mut at = first;
    let total = index.scope().needles;
    for _ in 0..total {
        at = index.step(at, true).unwrap();
    }
    assert_eq!(at, first, "a full round comes back");
    assert_eq!(index.step(index.step(first, true).unwrap(), false), Some(first));
    let last = index.step(first, false).unwrap();
    let ids: Vec<NodeId> = index.ids().collect();
    assert_eq!(last.id, *ids.last().unwrap(), "back from the first: the last path's");
    assert_eq!(last.index + 1, index.count(last.id));
}

/// An edit of one path builds that path's needle points again and no other's; the next query finds
/// the moved needle point where it is now.
#[test]
fn an_edit_builds_only_the_path_it_changed() {
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    let before = whole(&s);
    let id = NodeId(m.trim_after);
    let was = before.point(NeedleRef { id, index: 1 }).unwrap();
    run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": id.0, "index": 1}], "dx": 2.0}));
    let doc = s.doc().unwrap().doc.clone();
    let roots: Vec<NodeId> = doc.layers.iter().map(|l| l.id).collect();
    let built = paths_built();
    let after = NeedleIndex::of(&doc, &roots, Some(&before)).unwrap();
    assert_eq!(paths_built() - built, 1, "only the path moved");
    let now = after.point(NeedleRef { id, index: 1 }).unwrap();
    assert!((now.x - was.x - 2.0 * PT_PER_MM).abs() < 1e-6 && (now.y - was.y).abs() < 1e-9, "{was:?} → {now:?}");
    assert_eq!(after.needle_at(now, 0.5).map(|h| h.needle), Some(NeedleRef { id, index: 1 }));
    let again = NeedleIndex::of(&doc, &roots, Some(&after)).unwrap();
    assert_eq!(paths_built() - built, 1, "nothing changed, nothing built");
    assert_eq!(again.scope(), after.scope());
}

/// A machine file's run in a clip group is cut by it, so it isn't edited; manual stitches in one
/// never are, so they are. The stitch commands judge it alike: by address refused, in the
/// selection passed over as clipped.
#[test]
fn a_files_run_in_a_clip_group_is_shown_not_edited() {
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    let index = whole(&s);
    let doc = s.doc().unwrap().doc.clone();
    let member = doc.node(NodeId(m.converted.clip_member)).unwrap();
    let mut inside = Vec::new();
    member.walk(&mut |c| {
        if c.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some()) {
            inside.push(c.id);
        }
    });
    assert!(!inside.is_empty() && inside.iter().all(|id| index.holds(*id)), "manual stitches in a clip group are edited");
    // A file's run put in a clip group: refused.
    let run_id = m.trim_before;
    let circle = run(&mut s, "shape.ellipse", json!({"x": 0, "y": 0, "width": 2000, "height": 2000}))["id"].as_u64().unwrap();
    run(&mut s, "select.set", json!({"ids": [run_id, circle]}));
    run(&mut s, "object.clippingMask.make", json!({}));
    let index = whole(&s);
    assert!(!index.holds(NodeId(run_id)), "a file's run cut by a clip group");
    let refused = s.execute("embroidery.moveNeedlePoints", &json!({"needlePoints": [{"id": run_id, "index": 0}], "dx": 1})).unwrap_err().to_string();
    assert!(refused.contains("clipping mask"), "{refused}");
    run(&mut s, "select.set", json!({"ids": [run_id, m.trim_after]}));
    let reply = run(&mut s, "embroidery.moveNeedlePoints", json!({"dx": 1}));
    assert_eq!(reply["passedOver"], json!([{"id": run_id, "reason": "clipped"}]), "{reply}");
}

/// The marquee: `embroidery.selectNeedlePoints {rect}` selects exactly the needle points inside it,
/// of the object isolated alone; `{polygon}` those inside a triangle; add and subtract; junk refused.
#[test]
fn a_shape_selects_the_needle_points_inside_it() {
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    let group = m.converted.fill;
    run(&mut s, "object.isolate", json!({"id": group}));
    let doc = s.doc().unwrap().doc.clone();
    let index = NeedleIndex::of(&doc, &[NodeId(group)], None).unwrap();
    let all: Vec<(NeedleRef, Point)> =
        index.ids().flat_map(|id| read_points(&s, id).into_iter().enumerate().map(move |(k, p)| (NeedleRef { id, index: k }, p))).collect();
    let b = index.scope().bounds.unwrap();
    let r = Rect::new(b.x0, b.y0, b.center().x, b.center().y);
    let mm = |v: f64| v / PT_PER_MM;
    let reply = run(&mut s, "embroidery.selectNeedlePoints", json!({"rect": [mm(r.x0), mm(r.y0), mm(r.x1), mm(r.y1)]}));
    let inside: BTreeSet<NeedleRef> = all.iter().filter(|(_, p)| Area::Rect(r).contains(*p)).map(|(n, _)| *n).collect();
    assert!(!inside.is_empty());
    assert_eq!(reply["selectedCount"].as_u64().unwrap() as usize, inside.len(), "{reply}");
    let selected = |s: &Session| -> BTreeSet<NeedleRef> {
        let st = s.doc().unwrap();
        st.selection
            .anchors
            .iter()
            .flat_map(|(id, anchors)| anchors.iter().filter_map(|a| index.of_anchor(*id, *a)).map(|k| NeedleRef { id: *id, index: k }))
            .collect()
    };
    assert_eq!(selected(&s), inside);
    // The same rectangle, subtracted: nothing left.
    run(&mut s, "embroidery.selectNeedlePoints", json!({"rect": [mm(r.x0), mm(r.y0), mm(r.x1), mm(r.y1)], "mode": "subtract"}));
    assert!(selected(&s).is_empty());
    // A triangle over the whole object: those inside it by the even-odd rule.
    let tri = [Point::new(b.x0 - 1.0, b.y0 - 1.0), Point::new(b.x1 + 1.0, b.y0 - 1.0), Point::new(b.x0 - 1.0, b.y1 + 1.0)];
    let lasso = Lasso::new(&tri).unwrap();
    let want: BTreeSet<NeedleRef> = all.iter().filter(|(_, p)| lasso.contains(*p)).map(|(n, _)| *n).collect();
    let poly: Vec<Value> = tri.iter().map(|p| json!([mm(p.x), mm(p.y)])).collect();
    run(&mut s, "embroidery.selectNeedlePoints", json!({ "polygon": poly, "mode": "add" }));
    assert_eq!(selected(&s), want);
    for junk in [
        json!({"rect": [0, 0, 1]}),
        json!({"rect": [0, 0, 1, "x"]}),
        json!({"rect": [0, 0, 1, 1e300]}),
        json!({"polygon": [[0, 0], [1, 1]]}),
        json!({"polygon": [[0, 0], [1, 1], ["x", 2]]}),
        json!({"polygon": [[0, 0], [1, 1], [2, 1e300]]}),
        json!({"rect": [0, 0, 1, 1], "polygon": [[0, 0], [1, 0], [0, 1]]}),
        json!({"rect": [0, 0, 1, 1], "records": [1, 2]}),
        json!({"rect": [0, 0, 1, 1], "needlePoints": []}),
        json!({"rect": [0, 0, 1, 1], "kinds": ["stitch"]}),
    ] {
        assert!(s.execute("embroidery.selectNeedlePoints", &junk).is_err(), "{junk}");
    }
}

/// Roots named many times, or inside one another, are walked once: the index holds and refuses what
/// the outermost hold and refuse alone, and an id that isn't in the document costs nothing. A
/// shape's selection over the layers named as often as `ids` may selects and passes over as over
/// them once.
#[test]
fn repeated_and_nested_roots_are_walked_once() {
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    let doc = s.doc().unwrap().doc.clone();
    let layers: Vec<NodeId> = doc.layers.iter().map(|l| l.id).collect();
    let alone = NeedleIndex::of(&doc, &layers, None).unwrap();
    assert!(alone.scope().refused >= 2, "the fixture's locked and hidden runs: {:?}", alone.scope());
    let mut roots: Vec<NodeId> = layers.iter().copied().cycle().take(layers.len() * 300).collect();
    roots.extend([NodeId(m.converted.group), NodeId(m.one_point), NodeId(u64::MAX), NodeId(m.locked), NodeId(m.hidden)]);
    roots.extend(&layers);
    let many = NeedleIndex::of(&doc, &roots, None).unwrap();
    assert_eq!(many.scope(), alone.scope());
    assert!(many.ids().eq(alone.ids()), "the same paths, in the same order");
    // Nested first: the object it says it edits stays the first named, its paths walked once.
    let mut nested = vec![NodeId(m.converted.group)];
    nested.extend(&layers);
    nested.push(NodeId(m.converted.group));
    let nested = NeedleIndex::of(&doc, &nested, None).unwrap().scope();
    assert_eq!(nested.object, NodeId(m.converted.group));
    assert_eq!((nested.paths, nested.needles, nested.refused), (alone.scope().paths, alone.scope().needles, alone.scope().refused));
    let b = alone.scope().bounds.unwrap();
    let mm = |v: f64| v / PT_PER_MM;
    let rect = json!([mm(b.x0), mm(b.y0), mm(b.x1), mm(b.y1)]);
    let ids = |times: usize| -> Vec<u64> { layers.iter().map(|l| l.0).cycle().take(layers.len() * times).collect() };
    let once = run(&mut s, "embroidery.selectNeedlePoints", json!({"rect": rect, "ids": ids(1)}));
    let repeated = run(&mut s, "embroidery.selectNeedlePoints", json!({"rect": rect, "ids": ids(needles::MAX_PATHS / layers.len())}));
    assert_eq!(repeated, once);
    assert_eq!(once["passedOver"].as_u64().unwrap() as usize, alone.scope().refused, "{once}");
}

/// Through the session: the embroidery tool's answers hold the needle points only while it works
/// inside isolation of an object holding stitches, in the stitch view; the numbers are the plan's
/// (`embroidery.stitches`' records of the path's needle points); out of isolation, what was kept
/// goes.
#[test]
fn the_tool_edits_needle_points_in_isolation_of_a_stitch_object() {
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    let group = m.converted.stroke;
    run(&mut s, "embroidery.sequence", json!({}));
    s.select_tool("embroidery", crate::ViewInfo::default()).unwrap();
    // The scope, and each of the first needle points' numbers by their index.
    let asked = |s: &mut Session, ids: &[(NodeId, usize)]| {
        let st = s.docs.get(s.active.unwrap()).unwrap();
        let facts = s.embroidery_plans.facts_now("embroidery", st, false).unwrap();
        facts.needle_points().map(|n| (n.scope(), ids.iter().map(|&(id, index)| n.number(NeedleRef { id, index })).collect::<Vec<_>>()))
    };
    assert!(asked(&mut s, &[]).is_none(), "nothing isolated");
    run(&mut s, "object.isolate", json!({"id": group}));
    assert!(asked(&mut s, &[]).is_none(), "the art view: needle points aren't drawn there");
    run(&mut s, "view.stitchPreview", json!({"on": true}));
    let listed = run(&mut s, "embroidery.stitches", json!({"ids": [group], "kinds": ["stitch"], "limit": 20}));
    let rows: Vec<(NodeId, usize, usize)> = listed["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| Some((NodeId(r["node"].as_u64()?), r["index"].as_u64()? as usize, r["record"].as_u64()? as usize)))
        .collect();
    assert!(rows.len() > 2, "{listed}");
    let ids: Vec<(NodeId, usize)> = rows.iter().map(|&(id, k, _)| (id, k)).collect();
    let (scope, numbers) = asked(&mut s, &ids).expect("editing");
    assert_eq!(scope.object, NodeId(group));
    assert_eq!(numbers, rows.iter().map(|&(_, _, r)| Some(r)).collect::<Vec<_>>(), "the plan's numbers");
    assert_eq!(s.embroidery_plans.kept_needles(), 1);
    // A needle point moved: its path's numbers wait for the next plan.
    let (id, k, _) = rows[0];
    run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": id.0, "index": k}], "dx": 1.0}));
    let (_, numbers) = asked(&mut s, &[(id, k)]).unwrap();
    assert_eq!(numbers, vec![None], "the plan lags behind the edit");
    run(&mut s, "object.exitIsolation", json!({}));
    assert!(asked(&mut s, &[]).is_none());
    assert_eq!(s.embroidery_plans.kept_needles(), 0, "dropped when the editing ends");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// A lasso's inside, against a plain even-odd test over every edge, for random polygons and
    /// points.
    #[test]
    fn a_lasso_holds_what_every_edge_says(
        poly in proptest::collection::vec((-100.0f64..100.0, -100.0f64..100.0), 3..40),
        points in proptest::collection::vec((-120.0f64..120.0, -120.0f64..120.0), 1..60),
    ) {
        let poly: Vec<Point> = poly.into_iter().map(|(x, y)| Point::new(x, y)).collect();
        let Ok(lasso) = Lasso::new(&poly) else { return Ok(()) };
        for (x, y) in points {
            let p = Point::new(x, y);
            let mut inside = false;
            for k in 0..poly.len() {
                let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
                if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                    inside = !inside;
                }
            }
            prop_assert_eq!(lasso.contains(p), inside, "{:?}", p);
        }
    }
}

/// What the walk doesn't sew isn't edited on the canvas, by the walk's own rules: a run at no
/// opacity and a run without a colour are left out, each counted as one that can't be edited; and
/// after a stop the needle is lifted, so no stitch is offered there.
#[test]
fn what_the_walk_doesnt_sew_is_left_out() {
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    let before = whole(&s);
    let (clear, unpainted) = (NodeId(m.opened[0].runs[0]), NodeId(m.opened[0].runs[1]));
    assert!(before.holds(clear) && before.holds(unpainted));
    s.edit("Not Sewn", |d, _| {
        d.node_mut(clear).unwrap().opacity = 0.0;
        d.node_mut(unpainted).unwrap().appearance.items.clear();
        Ok(())
    })
    .unwrap();
    let after = whole(&s);
    assert!(!after.holds(clear) && !after.holds(unpainted), "neither is sewn");
    assert_eq!(after.scope().refused, before.scope().refused + 2, "{:?}", after.scope());
    // The run the machine stops in, after its second needle point: the lift to the third isn't a
    // stitch (a stitch sewn later over the same place may be); one in its middle is.
    let stopped = NodeId(m.stopped);
    let p = read_points(&s, stopped);
    let mid = |a: Point, b: Point| Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let tol = 0.01;
    let k = p.len() / 2;
    assert_eq!(after.thread_at(mid(p[k], p[k + 1]), tol).map(|t| t.from), Some(NeedleRef { id: stopped, index: k }), "a stitch");
    let lift = NeedleRef { id: stopped, index: 1 };
    assert!(after.path(stopped).unwrap().stops.contains(&lift.index), "the stop kept by the index");
    assert!(after.thread_at(mid(p[1], p[2]), tol).is_none_or(|t| t.from != lift), "the lift after the stop");
}

/// A hostile lasso costs a bounded amount: a zigzag of the most points a lasso may have, each edge
/// spanning its whole height (every edge in every band), is refused before anything is filled; a
/// selection that would test past its budget of edges is refused, nothing selected; a fair lasso
/// over the same needle points selects.
#[test]
fn a_hostile_lasso_is_refused_at_a_bounded_cost() {
    use super::{LassoRefused, MAX_LASSO_POINTS};
    let zigzag: Vec<Point> = (0..MAX_LASSO_POINTS).map(|k| Point::new(k as f64, if k % 2 == 0 { 0.0 } else { 1000.0 })).collect();
    assert_eq!(Lasso::new(&zigzag).err(), Some(LassoRefused::TooIntricate));
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    let doc = s.doc().unwrap().doc.clone();
    let index = NeedleIndex::of(&doc, &[NodeId(m.converted.fill)], None).unwrap();
    let b = index.scope().bounds.unwrap();
    let around = [
        Point::new(b.x0 - 1.0, b.y0 - 1.0),
        Point::new(b.x1 + 1.0, b.y0 - 1.0),
        Point::new(b.x1 + 1.0, b.y1 + 1.0),
        Point::new(b.x0 - 1.0, b.y1 + 1.0),
    ];
    let area = Area::Lasso(Lasso::new(&around).unwrap());
    let all = index.within(&area).expect("a fair lasso");
    assert_eq!(all.iter().map(|(_, k)| k.len()).sum::<usize>(), index.scope().needles);
    assert!(index.within_tested(&area, 10).is_none(), "past its budget of edge tests");
    // Through the command: the zigzag refused, saying why.
    let mm = |v: f64| v / PT_PER_MM;
    let poly: Vec<Value> = zigzag.iter().map(|p| json!([mm(p.x), mm(p.y)])).collect();
    let e = s.execute("embroidery.selectNeedlePoints", &json!({"polygon": poly})).unwrap_err().to_string();
    assert!(e.contains("too intricate"), "{e}");
}
