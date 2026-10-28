//! The plain text of a page, as lines: the characters of the page in the order of its content
//! stream, a space where one starts a little to the right of the end of the previous one, a
//! line end where the position jumps (to another baseline, far to the right, or back to the
//! left), along the direction of the text. That is what PyMuPDF's `get_text()` does, as far as
//! the DOI parser sees: it looks at the line ends (a DOI is cut by them), and `pdf-extract`'s
//! own plain text output breaks lines in fewer places, which glues a DOI to the word that
//! PyMuPDF starts a line with.

use pdf_extract::{MediaBox, OutputDev, OutputError, Transform};

/// A baseline farther than this (in sizes of the font) is another line.
const LINE: f64 = 0.9;
/// A gap wider than this starts a line; one wider than `SPACE` is a space.
const SPACE_MAX: f64 = 0.8;
const SPACE: f64 = 0.15;
/// A jump back wider than this starts a line (those that follow the same baseline are mostly
/// the characters that a PDF draws in another order; the constants were chosen to agree
/// with the lines of PyMuPDF on a few hundred PDFs, `tests/measure_pdfs.py`).
const BACK: f64 = 1.0;

pub struct Lines<'a> {
    out: &'a mut String,
    /// the end of the previous character: the point where the next one would be, along the
    /// direction of the text
    last: Option<(f64, f64)>,
}

impl<'a> Lines<'a> {
    pub fn new(out: &'a mut String) -> Lines<'a> {
        Lines { out, last: None }
    }
}

impl OutputDev for Lines<'_> {
    fn begin_page(
        &mut self,
        _page_num: u32,
        _media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        char: &str,
    ) -> Result<(), OutputError> {
        let (x, y) = (trm.m31, trm.m32);
        // the direction of the text (its x axis) and the size of the characters, on the page
        let along = (trm.m11.powi(2) + trm.m12.powi(2)).sqrt();
        let across = (trm.m21.powi(2) + trm.m22.powi(2)).sqrt();
        let size = font_size * (along * across).sqrt();
        let (dx, dy) = if along > 0.0 {
            (trm.m11 / along, trm.m12 / along)
        } else {
            (1.0, 0.0)
        };
        if let Some((ex, ey)) = self.last {
            let gap = (x - ex) * dx + (y - ey) * dy;
            let offset = -(x - ex) * dy + (y - ey) * dx;
            if offset.abs() > size * LINE || gap > size * SPACE_MAX || gap < -size * BACK {
                self.out.push('\n');
            } else if gap > size * SPACE {
                self.out.push(' ');
            }
        }
        self.out.push_str(char);
        let advance = width * size;
        self.last = Some((x + advance * dx, y + advance * dy));
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}
