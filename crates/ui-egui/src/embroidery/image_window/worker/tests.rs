//! The image window's thread: the newest job runs and a replaced one is answered as cancelled, a
//! panic is a failure and the thread goes on, and in line a job runs at the next poll.

use std::sync::Arc;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use serde_json::json;
use vectorcraft_engine::Session;
use vectorcraft_engine::cmd::fileio::progress::Progress;

use super::*;

/// A session holding a small picture placed and selected (an orange square on white), and its id.
pub(crate) fn picture_session() -> (Session, vectorcraft_engine::doc::NodeId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    let picture = image::RgbaImage::from_fn(120, 80, |x, y| {
        image::Rgba(if (20..100).contains(&x) && (15..65).contains(&y) { [240, 130, 20, 255] } else { [255, 255, 255, 255] })
    });
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(picture).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let data = vectorcraft_format::base64_encode(&png);
    s.execute("file.place", &json!({"name": "logo.png", "dataBase64": data})).unwrap();
    let id = s.doc().unwrap().selection.objects[0];
    (s, id)
}

fn job(s: &Session, id: vectorcraft_engine::doc::NodeId, ticket: u64, colors: u64) -> Job {
    let request = Arc::new(s.digitize_window_request(id, &json!({"colors": colors}), None).unwrap());
    Job::new(ticket, request, Progress::new())
}

/// The steps of `ticket` from `worker`, polled until its last, by time.
fn until_last(worker: &mut Worker, ticket: u64) -> Vec<Done> {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut got = Vec::new();
    loop {
        for d in worker.poll() {
            let last = d.last && d.ticket == ticket;
            got.push(d);
            if last {
                return got;
            }
        }
        assert!(Instant::now() < deadline, "the thread never answered");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// A job sent while another waits replaces it: the one replaced is answered as cancelled, the
/// newest runs every stage and finishes.
#[test]
fn the_newest_job_runs_and_a_replaced_one_is_answered_cancelled() {
    let (s, id) = picture_session();
    let ctx = egui::Context::default();
    let mut worker = Worker::start(&ctx, s.digitize_pipeline()).map_err(|_| "no thread").unwrap();
    // The first job holds the thread until the test lets it go, so the next two queue.
    let (hold_tx, hold_rx) = channel::<()>();
    let hold = std::sync::Mutex::new(hold_rx);
    let mut first = job(&s, id, 1, 3);
    first.hook = Some(Box::new(move |_| {
        let _ = hold.lock().unwrap().recv();
    }));
    assert!(worker.submit(first).is_none());
    assert!(worker.submit(job(&s, id, 2, 3)).is_none());
    assert!(worker.submit(job(&s, id, 3, 4)).is_none());
    hold_tx.send(()).unwrap();
    let steps = until_last(&mut worker, 3);
    let of = |t: u64| steps.iter().filter(move |d| d.ticket == t);
    assert!(of(2).all(|d| matches!(d.outcome, Outcome::Cancelled)), "the replaced one answered as cancelled");
    assert!(of(2).count() == 1);
    let stages: Vec<DigitizeStage> = of(3).filter_map(|d| if let Outcome::Stage { stage, .. } = &d.outcome { Some(*stage) } else { None }).collect();
    assert_eq!(stages, DigitizeStage::ALL.to_vec(), "every stage handed over in order");
    assert!(of(3).next_back().is_some_and(|d| matches!(d.outcome, Outcome::Finished(_))));
}

/// A panic while digitizing is answered as a failure, and the thread serves the next job.
#[test]
fn a_panic_is_a_failure_and_the_thread_goes_on() {
    let (s, id) = picture_session();
    let ctx = egui::Context::default();
    let mut worker = Worker::start(&ctx, s.digitize_pipeline()).map_err(|_| "no thread").unwrap();
    let mut broken = job(&s, id, 1, 3);
    broken.hook = Some(Box::new(|_| panic!("a bug")));
    assert!(worker.submit(broken).is_none());
    let steps = until_last(&mut worker, 1);
    assert!(matches!(&steps.last().unwrap().outcome, Outcome::Failed(m) if m.contains("a bug")));
    assert!(worker.submit(job(&s, id, 2, 3)).is_none());
    let steps = until_last(&mut worker, 2);
    assert!(matches!(steps.last().unwrap().outcome, Outcome::Finished(_)));
}

/// In line (no thread) a job runs at the next poll, not when it is sent, so the frame that asked
/// for it is drawn first; a job queued before it is answered as cancelled.
#[test]
fn in_line_a_job_runs_at_the_next_poll() {
    let (s, id) = picture_session();
    let ctx = egui::Context::default();
    let mut driver = Driver::new(&ctx, s.digitize_pipeline());
    assert!(driver.in_line(), "unit tests run in line");
    driver.submit(job(&s, id, 1, 3));
    driver.submit(job(&s, id, 2, 4));
    let steps = driver.poll();
    assert!(steps.iter().any(|d| d.ticket == 1 && matches!(d.outcome, Outcome::Cancelled)));
    assert!(steps.last().is_some_and(|d| d.ticket == 2 && matches!(d.outcome, Outcome::Finished(_))));
    assert!(driver.poll().is_empty(), "nothing more");
}
