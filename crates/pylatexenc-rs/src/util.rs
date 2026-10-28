//! Helpers (port of `pylatexenc._util`).

/// `LineNumbersCalculator`: line and column numbers of positions of a text.
#[derive(Debug, Clone)]
pub struct LineNumbersCalculator {
    pos_new_lines: Vec<usize>,
}

impl LineNumbersCalculator {
    /// For a text given as chars.
    pub fn new(s: &[char]) -> Self {
        let mut pos_new_lines = vec![0];
        for (k, &c) in s.iter().enumerate() {
            if c == '\n' {
                pos_new_lines.push(k + 1);
            }
        }
        LineNumbersCalculator { pos_new_lines }
    }

    /// `pos_to_lineno_colno(pos)`: `(lineno, colno)`, the line number counting from 1 and the
    /// column from 0.
    pub fn pos_to_lineno_colno(&self, pos: usize) -> (usize, usize) {
        // bisect_right(pos_new_lines, pos) - 1
        let line_no = self.pos_new_lines.partition_point(|&p| p <= pos) - 1;
        (1 + line_no, pos - self.pos_new_lines[line_no])
    }
}
