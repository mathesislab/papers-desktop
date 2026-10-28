//! Reading a PDF: its text, page by page, and its metadata (the document information and
//! the XMP).
//!
//! papers reads PDFs with PyMuPDF (`page.get_text()`), which is under the AGPL. papers-rs
//! uses `pdf-extract` (MIT, on `lopdf`) for the characters of a page, in the order of its
//! content stream, and makes the lines of them (`text`). It is not the text of PyMuPDF: the
//! same words in most pages, not always the same line ends, and a font that `pdf-extract`
//! decodes differently gives other characters. What papers does with the text, the DOI
//! parser and the check of the title, is the same (`extract::doi`);
//! `tests/measure_pdfs.py` measures how often the result agrees.
//!
//! `pdf-extract` panics on some PDFs: they are caught (the page has no text), which needs a
//! build that unwinds. With `panic = "abort"` (the release profile of this workspace) such a
//! PDF ends the process.

use std::panic::{AssertUnwindSafe, catch_unwind};

use pdf_extract::{Document, Object};

/// A PDF that cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfError(pub String);

impl std::fmt::Display for PdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PdfError {}

/// A PDF document.
pub struct Pdf {
    doc: Document,
    pages: Vec<u32>,
}

impl Pdf {
    /// Open a file (PyMuPDF: `pymupdf.open`).
    pub fn open(path: &str) -> Result<Pdf, PdfError> {
        let failed = || {
            PdfError(format!(
                "Failed to open file {}.",
                crate::config::json::py_repr_str(path)
            ))
        };
        let loaded = catch_unwind(|| Document::load(path)).map_err(|_| failed())?;
        let mut doc = loaded.map_err(|_| failed())?;
        if doc.is_encrypted() {
            let _ = doc.decrypt("");
        }
        // pdf-extract runs the data of every XObject as a content stream, the images
        // too, whose bytes are read as operators (and may panic): an image has no text
        for object in doc.objects.values_mut() {
            if let Object::Stream(stream) = object
                && stream
                    .dict
                    .get(b"Subtype")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    == Some(b"Image")
            {
                stream.content.clear();
                stream.dict.remove(b"Filter");
                stream.dict.remove(b"DecodeParms");
            }
        }
        let pages = doc.get_pages().keys().copied().collect();
        Ok(Pdf { doc, pages })
    }

    /// The number of pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// The text of a page (`page.get_text()`), counted from 0; empty past the last page, and
    /// for a page that cannot be read.
    pub fn page_text(&self, index: usize) -> String {
        let Some(&number) = self.pages.get(index) else {
            return String::new();
        };
        catch_unwind(AssertUnwindSafe(|| {
            let mut text = String::new();
            {
                let mut output = super::text::Lines::new(&mut text);
                let _ = pdf_extract::output_doc_page(&self.doc, &mut output, number);
            }
            text
        }))
        .unwrap_or_default()
    }

    /// The text of the last page, if the PDF has more than one (`pdftail`: publishers such as
    /// AAAS append a cover page with the DOI).
    pub fn tail(&self) -> String {
        if self.page_count() < 2 {
            String::new()
        } else {
            self.page_text(self.page_count() - 1)
        }
    }

    /// The text of the first pages: up to `maxpages`, and while it has fewer than `minwords`
    /// words (`pdfhead`).
    pub fn head(&self, maxpages: usize, minwords: i64) -> String {
        let mut txt = String::new();
        let mut i = 0;
        while (super::doi::py_split(crate::bib::pystr::strip(&txt)).len() as i64) < minwords
            && i < maxpages
        {
            txt.push_str(&self.page_text(i));
            i += 1;
        }
        txt
    }

    /// A field of the document information (`doc.metadata`), empty when it has none.
    pub fn info_field(&self, name: &str) -> String {
        let info = self
            .doc
            .trailer
            .get(b"Info")
            .ok()
            .and_then(|o| self.doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok());
        let Some(object) = info.and_then(|d| d.get(name.as_bytes()).ok()) else {
            return String::new();
        };
        match self.doc.dereference(object) {
            Ok((_, Object::String(bytes, _))) => text_string(bytes),
            _ => String::new(),
        }
    }

    /// The XMP metadata (`doc.get_xml_metadata()`).
    pub fn xmp(&self) -> Option<String> {
        let catalog = self.doc.catalog().ok()?;
        let (_, metadata) = self.doc.dereference(catalog.get(b"Metadata").ok()?).ok()?;
        let stream = metadata.as_stream().ok()?;
        let data = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        Some(String::from_utf8_lossy(&data).into_owned())
    }

    /// The DOI in the metadata of the PDF (`parse_doi_from_pdf_metadata_pymupdf`): in the
    /// subject, the keywords or the title of the document information, else in the XMP.
    pub fn metadata_doi(&self) -> Option<String> {
        let by_info = super::doi::doi_from_info_fields(
            &self.info_field("Subject"),
            &self.info_field("Keywords"),
            &self.info_field("Title"),
        );
        by_info.or_else(|| {
            self.xmp()
                .filter(|x| !x.is_empty())
                .and_then(|x| super::doi::doi_from_xmp(&x))
        })
    }
}

/// A PDF text string: UTF-16 with a byte order mark, UTF-8 with one, else PDFDocEncoding.
fn text_string(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    bytes.iter().map(|&b| pdfdoc(b)).collect()
}

/// PDFDocEncoding of a byte (the Latin-1 characters, but for the punctuation of 0x80-0xA0).
fn pdfdoc(b: u8) -> char {
    const HIGH: [char; 33] = [
        '\u{2022}', '\u{2020}', '\u{2021}', '\u{2026}', '\u{2014}', '\u{2013}', '\u{192}',
        '\u{2044}', '\u{2039}', '\u{203a}', '\u{2212}', '\u{2030}', '\u{201e}', '\u{201c}',
        '\u{201d}', '\u{2018}', '\u{2019}', '\u{201a}', '\u{2122}', '\u{fb01}', '\u{fb02}',
        '\u{141}', '\u{152}', '\u{160}', '\u{178}', '\u{17d}', '\u{131}', '\u{142}', '\u{153}',
        '\u{161}', '\u{17e}', '\u{fffd}', '\u{20ac}',
    ];
    match b {
        0x80..=0xa0 => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}
