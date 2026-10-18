use crate::issue::{Location, Span};

/// Maps byte offsets to line/column positions.
pub struct LineIndex<'s> {
    src: &'s str,
    /// Byte offset of the start of each line.
    starts: Vec<u32>,
}

impl<'s> LineIndex<'s> {
    pub fn new(src: &'s str) -> Self {
        let mut starts = Vec::with_capacity(src.len() / 32 + 1);
        starts.push(0);
        let bytes = src.as_bytes();
        for i in memchr::memchr_iter(b'\n', bytes) {
            starts.push(i as u32 + 1);
        }
        // Lone CR line terminators are rare; handle them for correctness.
        if memchr::memchr(b'\r', bytes).is_some() {
            let mut fixed = Vec::with_capacity(starts.len());
            fixed.push(0);
            let mut i = 0;
            while i < bytes.len() {
                match bytes[i] {
                    b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                        fixed.push(i as u32 + 2);
                        i += 2;
                        continue;
                    }
                    b'\r' | b'\n' => fixed.push(i as u32 + 1),
                    _ => {}
                }
                i += 1;
            }
            starts = fixed;
        }
        Self { src, starts }
    }

    #[inline]
    pub fn line_count(&self) -> u32 {
        self.starts.len() as u32
    }

    /// 1-based line of a byte offset.
    #[inline]
    pub fn line(&self, offset: u32) -> u32 {
        match self.starts.binary_search(&offset) {
            Ok(i) => i as u32 + 1,
            Err(i) => i as u32,
        }
    }

    #[inline]
    pub fn line_start(&self, line: u32) -> u32 {
        self.starts[(line - 1) as usize]
    }

    /// Text of a 1-based line, without its terminator.
    pub fn line_text(&self, line: u32) -> &'s str {
        let i = (line - 1) as usize;
        let start = self.starts[i] as usize;
        let end = self.starts.get(i + 1).map_or(self.src.len(), |&e| e as usize);
        self.src[start..end].trim_end_matches(['\n', '\r'])
    }

    /// 0-based column of a byte offset, counted in characters.
    pub fn col(&self, offset: u32) -> u32 {
        let line = self.line(offset);
        let start = self.line_start(line) as usize;
        let end = (offset as usize).min(self.src.len());
        let slice = &self.src.as_bytes()[start..end];
        if slice.is_ascii() {
            slice.len() as u32
        } else {
            self.src.get(start..end).map_or(slice.len(), |s| s.chars().count()) as u32
        }
    }

    pub fn location(&self, span: Span) -> Location {
        Location {
            line: self.line(span.start),
            col: self.col(span.start),
            end_line: self.line(span.end),
            end_col: self.col(span.end),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions() {
        let idx = LineIndex::new("ab\ncd\r\nef\rgh");
        assert_eq!(idx.line_count(), 4);
        assert_eq!(idx.line(0), 1);
        assert_eq!(idx.line(3), 2);
        assert_eq!(idx.line(7), 3);
        assert_eq!(idx.line(10), 4);
        assert_eq!(idx.col(4), 1);
        assert_eq!(idx.line_text(2), "cd");
    }
}
