//! Row layout of the virtualized grid (SPEC §9.3).

use crate::search::Group;
use crate::viewer::RectF;
use std::ops::Range;

/// Height of a group header row in pixels.
pub const HEADER_HEIGHT: f32 = 36.0;
/// Space between cells and rows in pixels.
pub const GAP: f32 = 8.0;
/// Smallest and largest cell size of the zoom slider.
pub const ZOOM_RANGE: (u32, u32) = (96, 320);

/// One row of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridRow {
    /// The header of group `group`.
    Header {
        /// Index into the result's groups.
        group: usize,
    },
    /// A run of cells of one group.
    Cells {
        /// Index into the result's groups.
        group: usize,
        /// Index of the first cell within the group.
        start: usize,
        /// Number of cells in the row.
        len: usize,
    },
}

/// Rows and their vertical positions for a given width and cell size.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridLayout {
    /// Rows in display order.
    pub rows: Vec<GridRow>,
    /// Cells per row.
    pub columns: usize,
    /// Cell edge in pixels.
    pub cell: f32,
    tops: Vec<f32>,
    height: f32,
}

/// Cells per row for a viewport `width` wide.
pub fn columns_for(width: f32, cell: u32) -> usize {
    let cell = cell as f32;
    (((width + GAP) / (cell + GAP)).floor() as usize).max(1)
}

impl GridLayout {
    /// Lays out `groups`: a header row per non-empty group, then rows of `columns` cells.
    pub fn build(groups: &[Group], columns: usize, cell: f32) -> Self {
        let columns = columns.max(1);
        let mut rows = Vec::new();
        for (g, group) in groups.iter().enumerate() {
            if group.ids.is_empty() {
                continue;
            }
            rows.push(GridRow::Header { group: g });
            let mut start = 0;
            while start < group.ids.len() {
                let len = columns.min(group.ids.len() - start);
                rows.push(GridRow::Cells {
                    group: g,
                    start,
                    len,
                });
                start += len;
            }
        }
        let mut layout = Self {
            rows,
            columns,
            cell,
            tops: Vec::new(),
            height: 0.0,
        };
        let mut y = 0.0;
        let mut tops = Vec::with_capacity(layout.rows.len());
        for row in &layout.rows {
            tops.push(y);
            y += layout.height_of(row);
        }
        layout.tops = tops;
        layout.height = y;
        layout
    }

    fn height_of(&self, row: &GridRow) -> f32 {
        match row {
            GridRow::Header { .. } => HEADER_HEIGHT,
            GridRow::Cells { .. } => self.cell + GAP,
        }
    }

    /// Distance of row `i` from the top of the content.
    pub fn row_top(&self, i: usize) -> f32 {
        self.tops.get(i).copied().unwrap_or(self.height)
    }

    /// Height of row `i`.
    pub fn row_height(&self, i: usize) -> f32 {
        self.rows.get(i).map_or(0.0, |r| self.height_of(r))
    }

    /// Height of all rows.
    pub fn total_height(&self) -> f32 {
        self.height
    }

    /// Rows intersecting `[scroll_y, scroll_y + viewport_h]` extended by `overscan` pixels on
    /// both sides.
    pub fn visible_rows(&self, scroll_y: f32, viewport_h: f32, overscan: f32) -> Range<usize> {
        let top = (scroll_y - overscan).max(0.0);
        let bottom = scroll_y + viewport_h + overscan;
        let first = self.tops.partition_point(|&t| t < top).saturating_sub(1);
        let end = self.tops.partition_point(|&t| t < bottom);
        first.min(self.rows.len())..end.min(self.rows.len())
    }

    /// Cells `(group, index)` whose square touches the rectangle with corners `(x0, y0)` and
    /// `(x1, y1)`, given in content coordinates (origin at the top-left cell, y growing down).
    /// Used by rubber-band selection (SPEC §9.3).
    pub fn cells_in_rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<(usize, usize)> {
        let (left, right) = (x0.min(x1), x0.max(x1));
        let (top, bottom) = (y0.min(y1), y0.max(y1));
        let pitch = self.cell + GAP;
        let mut hits = Vec::new();
        for (i, row) in self.rows.iter().enumerate() {
            let GridRow::Cells { group, start, len } = *row else {
                continue;
            };
            let row_top = self.tops[i];
            if row_top > bottom || row_top + self.cell < top {
                continue;
            }
            for col in 0..len {
                let cell_left = col as f32 * pitch;
                if cell_left <= right && cell_left + self.cell >= left {
                    hits.push((group, start + col));
                }
            }
        }
        hits
    }

    /// Content-space rectangle of the cell `(group, index)` (SPEC-VIEWER §V4.2).
    ///
    /// The origin is the top-left of the first cell, not the window. `None` when that photo
    /// is not in the layout.
    pub fn cell_rect(&self, group: usize, index: usize) -> Option<RectF> {
        let row = self.row_of(group, index)?;
        let GridRow::Cells { start, len, .. } = self.rows[row] else {
            return None;
        };
        if index < start || index >= start + len {
            return None;
        }
        let col = index - start;
        let pitch = self.cell + GAP;
        Some(RectF {
            x: col as f32 * pitch,
            y: self.row_top(row),
            w: self.cell,
            h: self.cell,
        })
    }

    /// The row holding cell `index` of `group`.
    pub fn row_of(&self, group: usize, index: usize) -> Option<usize> {
        self.rows.iter().position(|r| {
            matches!(r, GridRow::Cells { group: g, start, len }
                if *g == group && (*start..start + len).contains(&index))
        })
    }
}

/// Duration badge text: `m:ss`, or `h:mm:ss` from one hour on. Rounds to the nearest second.
pub fn duration_label(duration_ms: u64) -> String {
    let secs = (duration_ms + 500) / 1000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::duration_label;

    #[test]
    fn duration_label_table() {
        for (ms, want) in [
            (0, "0:00"),
            (499, "0:00"),
            (500, "0:01"),
            (59_400, "0:59"),
            (59_500, "1:00"),
            (754_000, "12:34"),
            (3_600_000, "1:00:00"),
            (3_725_000, "1:02:05"),
        ] {
            assert_eq!(duration_label(ms), want, "{ms} ms");
        }
    }
}
