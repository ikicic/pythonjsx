// FIXME: positions are wrong for non-ASCII source.  We treat
// `Position.character` as UTF-8 bytes, but LSP defaults to UTF-16 code
// units (and we don't negotiate `positionEncoding`).  Files with emoji /
// CJK / accented chars get misaligned hovers, jumps, and underlines.
// Fix: declare `positionEncoding: "utf-16"` and translate at the LSP
// boundary in `LineColumnMap`.

use crate::compiler::sourcemap::{LineColumnMap, SourceMap};
use crate::lsp::types::{Location, Position, Range};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug)]
pub struct DocumentMapping {
    pub original_uri: String,
    pub py_uri: String,
    pub source_map: Option<SourceMap>,
    pub px_line_map: Option<LineColumnMap>,
    pub py_line_map: LineColumnMap,
    pub original_text: String,
    pub py_text: String,
}

impl DocumentMapping {
    pub fn new(
        original_uri: String,
        py_uri: String,
        source_map: Option<SourceMap>,
        original_text: String,
        py_text: String,
    ) -> Self {
        let px_line_map = Some(LineColumnMap::new(&original_text));
        let py_line_map = LineColumnMap::new(&py_text);
        Self {
            original_uri,
            py_uri,
            source_map,
            px_line_map,
            py_line_map,
            original_text,
            py_text,
        }
    }

    pub fn map_px_position_to_py_position(&self, position: &Position) -> Option<Position> {
        let px_line_map = self.px_line_map.as_ref()?;
        let source_map = self.source_map.as_ref()?;

        let px_offset = px_line_map.line_col_to_byte(position.line as usize, position.character as usize);
        let result = source_map.px_to_py(px_offset);

        let (line, col) = self.py_line_map.byte_to_line_col(result.start);
        Some(Position {
            line: line as u32,
            character: col as u32,
        })
    }

    pub fn map_py_range_to_px_range(&self, range: &Range) -> Option<Range> {
        let px_line_map = self.px_line_map.as_ref()?;
        let source_map = self.source_map.as_ref()?;

        let start_offset = self.py_line_map.line_col_to_byte(range.start.line as usize, range.start.character as usize);
        let end_offset = self.py_line_map.line_col_to_byte(range.end.line as usize, range.end.character as usize);

        let start_res = source_map.py_to_px(start_offset);
        let end_res = source_map.py_to_px(end_offset);

        let (start_line, start_col) = px_line_map.byte_to_line_col(start_res.start);
        let (end_line, end_col) = px_line_map.byte_to_line_col(end_res.start);

        Some(Range {
            start: Position {
                line: start_line as u32,
                character: start_col as u32,
            },
            end: Position {
                line: end_line as u32,
                character: end_col as u32,
            },
        })
    }

    pub fn map_py_location_to_px_location(&self, location: &Location) -> Option<Location> {
        let range = self.map_py_range_to_px_range(&location.range)?;
        Some(Location {
            uri: self.original_uri.clone(),
            range,
        })
    }
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    if let Ok(url) = Url::parse(uri) {
        if url.scheme() == "file" {
            if let Ok(path) = url.to_file_path() {
                return Some(path);
            }
        }
    }
    None
}

pub fn path_to_uri(path: &Path) -> String {
    if let Ok(url) = Url::from_file_path(path) {
        return url.to_string();
    }
    format!("file://{}", path.display())
}
