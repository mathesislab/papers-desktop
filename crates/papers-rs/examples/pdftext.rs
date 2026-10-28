//! What papers-rs reads of PDFs, for `tests/measure_pdfs.py`: for each file named on the
//! standard input (one per line), a line of JSON with the DOI of the PDF metadata, the text of
//! the first pages and of the last page as `extract_pdf_metadata` reads them, and the DOIs that
//! `parse_dois` finds in them. The same record as the script makes with papers.
//!
//!     cargo run --release --example pdftext < files.txt > rs.jsonl

use std::io::BufRead;

use papers_rs::extract::PdfFile;
use papers_rs::extract::doi::parse_dois;
use serde_json::{Value, json};

fn record(path: &str) -> Result<Value, String> {
    let file = PdfFile::new(path);
    let meta = file.metadata_doi().map_err(|e| e.message)?;
    let head = file.head(10, 200, false).map_err(|e| e.message)?;
    let tail = file.tail().map_err(|e| e.message)?;
    let (dois, err) = match parse_dois(&head, &tail) {
        Ok(d) => (json!(d), Value::Null),
        Err(e) => (Value::Null, json!(e.0)),
    };
    Ok(json!({"pdf": path, "meta": meta, "head": head, "tail": tail, "dois": dois, "err": err}))
}

fn main() {
    // the PDFs that pdf-extract cannot read panic: that is a failed file here
    std::panic::set_hook(Box::new(|_| {}));
    for line in std::io::stdin().lock().lines() {
        let path = line.unwrap();
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        let owned = path.to_string();
        let result = std::panic::catch_unwind(move || record(&owned))
            .unwrap_or_else(|_| Err("panic".to_string()));
        let line = match result {
            Ok(v) => v,
            Err(e) => json!({"pdf": path, "exc": e}),
        };
        println!("{line}");
    }
}
