use std::fs;
use std::sync::mpsc;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use papers_core::BibWatch;

const WAIT: Duration = Duration::from_secs(5);
/// longer than the quiet time of the watch
const NOTHING: Duration = Duration::from_secs(2);

#[test]
fn reports_each_change_once() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("papers-desktop-watch-{nanos}"));
    fs::create_dir(&dir).unwrap();
    let bib = dir.join("papers.bib");
    fs::write(&bib, "a").unwrap();

    let (tx, rx) = mpsc::channel();
    let watch = BibWatch::new(bib.clone(), move || tx.send(()).unwrap()).unwrap();
    assert_eq!(watch.path(), bib);
    sleep(Duration::from_millis(500));

    // a burst of writes is one change
    fs::write(&bib, "b").unwrap();
    fs::write(&bib, "cc").unwrap();
    assert!(rx.recv_timeout(WAIT).is_ok());
    assert!(rx.recv_timeout(NOTHING).is_err());

    // a new file renamed over the bibtex, as papers and editors save
    fs::write(dir.join("papers.bib.tmp"), "ddd").unwrap();
    fs::rename(dir.join("papers.bib.tmp"), &bib).unwrap();
    assert!(rx.recv_timeout(WAIT).is_ok());

    // other files of the directory, and reading the bibtex, are no change
    fs::write(dir.join("notes.txt"), "x").unwrap();
    fs::read_to_string(&bib).unwrap();
    assert!(rx.recv_timeout(NOTHING).is_err());

    drop(watch);
    fs::remove_dir_all(&dir).unwrap();
}
