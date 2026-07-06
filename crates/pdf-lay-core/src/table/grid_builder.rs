//! Grid structure builder.
//!
//! Converts a `TableRegion` and its associated text blocks into a structured `TableGrid`.

use crate::types::{Rect, TextBlock};

/// Grid structure representing a parsed table.
pub struct TableGrid {
    /// Header rows (typically the first row).
    pub header: Vec<Vec<String>>,
    /// Data rows (all rows after the header).
    pub rows: Vec<Vec<String>>,
    /// Number of columns in the grid.
    pub column_count: usize,
    /// True if the table has multiple header rows.
    pub has_multi_header: bool,
    /// Cell-level view of `header`, with best-effort inferred colspan/rowspan
    /// (see [`GridBuilder::infer_header_spans`]). One `Vec<Cell>` per header
    /// row, one `Cell` per grid column. Derivable from `header` (every
    /// `Cell::text` matches the corresponding `header` string), so this is
    /// an additive, non-breaking field.
    pub header_rows: Vec<Vec<Cell>>,
}

/// A single table cell, tracking how many grid columns/rows it visually
/// spans.
///
/// Defaults to `colspan = 1, rowspan = 1` — an ordinary, unmerged cell. A
/// cell that has been merged into a preceding cell's span (see
/// [`GridBuilder::infer_header_spans`]) is represented with `colspan = 0,
/// rowspan = 0` as a "covered by a previous cell — do not render on its own"
/// sentinel; every other cell has `colspan >= 1` and `rowspan >= 1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// The cell's text content.
    pub text: String,
    /// Number of grid columns this cell spans (`1` = normal, `0` = covered
    /// by a preceding cell's span).
    pub colspan: usize,
    /// Number of grid rows this cell spans (`1` = normal, `0` = covered by a
    /// preceding cell's span).
    pub rowspan: usize,
}

impl Cell {
    /// Build an ordinary (unmerged) cell: `colspan = 1, rowspan = 1`.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            colspan: 1,
            rowspan: 1,
        }
    }
}

/// Builds a `TableGrid` from table region data.
pub struct GridBuilder;

impl GridBuilder {
    /// Build a grid from block indices and the text blocks.
    ///
    /// # Arguments
    ///
    /// * `block_indices` - Indices into `blocks` that belong to this table region.
    /// * `blocks` - The full slice of all text blocks in the document.
    /// * `table_bbox` - Bounding box of the table region (used for context).
    /// * `has_rules` - Whether the table has visible rule lines.
    pub fn build(
        block_indices: &[usize],
        blocks: &[TextBlock],
        _table_bbox: &Rect,
        _has_rules: bool,
    ) -> TableGrid {
        // 1. Collect the text blocks that belong to this table.
        let table_blocks: Vec<&TextBlock> = block_indices
            .iter()
            .filter_map(|&idx| blocks.get(idx))
            .collect();

        if table_blocks.is_empty() {
            return TableGrid {
                header: vec![],
                rows: vec![],
                column_count: 0,
                has_multi_header: false,
                header_rows: vec![],
            };
        }

        // 2. Determine column boundaries by clustering left-X coordinates.
        let column_boundaries = Self::detect_columns(&table_blocks);
        let column_count = column_boundaries.len();

        if column_count == 0 {
            return TableGrid {
                header: vec![],
                rows: vec![],
                column_count: 0,
                has_multi_header: false,
                header_rows: vec![],
            };
        }

        // 3. Determine row boundaries by clustering Y-coordinates (center_y of blocks).
        //    Sorted descending (top of page first in PDF Y-up coordinates).
        let row_boundaries = Self::detect_rows(&table_blocks);

        if row_boundaries.is_empty() {
            return TableGrid {
                header: vec![],
                rows: vec![],
                column_count: 0,
                has_multi_header: false,
                header_rows: vec![],
            };
        }

        // 4. Assign each block to a (row, col) cell.
        let mut grid: Vec<Vec<String>> = row_boundaries
            .iter()
            .map(|_| vec![String::new(); column_count])
            .collect();

        for block in &table_blocks {
            let col = Self::find_column(block.bbox.center_x(), &column_boundaries);
            let row = Self::find_row(block.bbox.center_y(), &row_boundaries);
            if row < grid.len() && col < column_count {
                if !grid[row][col].is_empty() {
                    grid[row][col].push(' ');
                }
                grid[row][col].push_str(block.text.trim());
            }
        }

        // 5. First row is the header; remaining rows are data rows.
        let (header, rows) = if grid.is_empty() {
            (vec![], vec![])
        } else {
            (vec![grid[0].clone()], grid[1..].to_vec())
        };

        let mut table_grid = TableGrid {
            header,
            rows,
            column_count,
            has_multi_header: false,
            header_rows: vec![],
        };

        // 6. Check for multi-row headers using the text blocks.
        Self::detect_multi_header(&table_blocks, &mut table_grid);

        // 7. Best-effort colspan/rowspan inference over the (possibly
        //    multi-row, post-promotion) header. Must run last, since step 6
        //    can change `table_grid.header`.
        table_grid.header_rows = Self::infer_header_spans(&table_grid.header);

        table_grid
    }

    /// Infer header-cell colspan/rowspan from runs of empty grid cells
    /// (best-effort; see `docs/refactor/phase2_llm_output.md#P2-8`).
    ///
    /// Academic tables commonly have a spanning/grouped header label (e.g.
    /// "Metrics" over "Accuracy" and "Precision") where only *one* grid
    /// slot actually receives a text block and the other slots it visually
    /// covers are left blank — both horizontally (empty cells to the right,
    /// in the same header row) and vertically (empty cells below, in the
    /// same column, across header rows). This function promotes those runs
    /// of adjacent empty cells into `colspan`/`rowspan` on the preceding
    /// non-empty cell.
    ///
    /// The two inferences are applied in a fixed order — rowspan first,
    /// then colspan — and a cell that already gained `rowspan > 1` is never
    /// also treated as a colspan-merge starting point. This keeps the
    /// result a single, non-overlapping 2-D layout instead of two
    /// independently-inferred spans clashing over the same grid slot.
    /// When no run is found, every cell keeps the `Cell::new` default of
    /// `colspan = 1, rowspan = 1`.
    fn infer_header_spans(header: &[Vec<String>]) -> Vec<Vec<Cell>> {
        if header.is_empty() {
            return Vec::new();
        }

        let mut cells: Vec<Vec<Cell>> = header
            .iter()
            .map(|row| row.iter().map(|s| Cell::new(s.clone())).collect())
            .collect();

        // Vertical merge (rowspan): a non-empty cell followed by empty
        // cells directly below it (same column) is assumed to span those
        // rows.
        let column_count = cells.first().map(|r| r.len()).unwrap_or(0);
        for col in 0..column_count {
            let mut row = 0;
            while row < cells.len() {
                let is_start = cells[row]
                    .get(col)
                    .map(|c| !c.text.is_empty())
                    .unwrap_or(false);
                if !is_start {
                    row += 1;
                    continue;
                }
                let mut next = row + 1;
                while next < cells.len()
                    && cells[next]
                        .get(col)
                        .map(|c| c.text.is_empty())
                        .unwrap_or(false)
                {
                    next += 1;
                }
                if next > row + 1 {
                    if let Some(cell) = cells[row].get_mut(col) {
                        cell.rowspan = next - row;
                    }
                    for covered_row in cells.iter_mut().take(next).skip(row + 1) {
                        if let Some(cell) = covered_row.get_mut(col) {
                            cell.colspan = 0;
                            cell.rowspan = 0;
                        }
                    }
                }
                row = next;
            }
        }

        // Horizontal merge (colspan): within a header row, a non-empty,
        // not-yet-covered cell that has not already gained a rowspan is
        // followed by empty, not-yet-covered cells — assumed to span those
        // columns.
        for row in cells.iter_mut() {
            let mut col = 0;
            while col < row.len() {
                let is_start =
                    row[col].colspan != 0 && row[col].rowspan == 1 && !row[col].text.is_empty();
                if !is_start {
                    col += 1;
                    continue;
                }
                let mut next = col + 1;
                while next < row.len() && row[next].colspan != 0 && row[next].text.is_empty() {
                    next += 1;
                }
                if next > col + 1 {
                    row[col].colspan = next - col;
                    for covered in row.iter_mut().take(next).skip(col + 1) {
                        covered.colspan = 0;
                        covered.rowspan = 0;
                    }
                }
                col = next;
            }
        }

        cells
    }

    /// Detect multi-row headers (e.g., spanning header + sub-headers).
    ///
    /// If the first data row is also bold (all non-empty cells contain bold text),
    /// move it from `rows` into `header`. Sets `has_multi_header` to `true` if
    /// the header contains 2 or more rows after promotion.
    pub fn detect_multi_header(blocks: &[&TextBlock], grid: &mut TableGrid) {
        if grid.rows.is_empty() {
            // Already no data rows; nothing to promote.
            grid.has_multi_header = grid.header.len() > 1;
            return;
        }

        // Check whether every non-empty cell in the first data row comes from a bold block.
        // We match by text content: if a block's text matches a non-empty cell and is bold,
        // that cell is considered "bold".
        let first_data_row = &grid.rows[0];
        let non_empty_cells: Vec<&String> =
            first_data_row.iter().filter(|s| !s.is_empty()).collect();

        if non_empty_cells.is_empty() {
            grid.has_multi_header = grid.header.len() > 1;
            return;
        }

        let all_bold = non_empty_cells.iter().all(|cell| {
            blocks
                .iter()
                .any(|b| b.text.trim() == cell.as_str() && b.is_bold())
        });

        if all_bold {
            // Promote first data row to header.
            let promoted = grid.rows.remove(0);
            grid.header.push(promoted);
        }

        grid.has_multi_header = grid.header.len() > 1;
    }

    /// Cluster left-X coordinates to detect columns.
    ///
    /// Blocks whose left-X values are within 5pt of each other are collapsed
    /// into the same column. Returns sorted unique representative X values.
    fn detect_columns(blocks: &[&TextBlock]) -> Vec<f64> {
        let mut xs: Vec<f64> = blocks.iter().map(|b| b.bbox.left).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // Cluster: keep one representative per cluster (the first encountered value).
        let mut representatives: Vec<f64> = Vec::new();
        for x in xs {
            if representatives
                .last()
                .is_none_or(|&rep| (x - rep).abs() >= 5.0)
            {
                representatives.push(x);
            }
        }
        representatives
    }

    /// Cluster Y-coordinates to detect rows (top to bottom in reading order).
    ///
    /// In PDF coordinates Y increases upward, so higher Y values appear higher
    /// on the page. The returned slice is sorted descending (highest Y first),
    /// which corresponds to top-to-bottom reading order.
    fn detect_rows(blocks: &[&TextBlock]) -> Vec<f64> {
        let mut ys: Vec<f64> = blocks.iter().map(|b| b.bbox.center_y()).collect();
        // Sort descending: highest Y (top of page) first.
        ys.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        // Cluster: keep one representative per cluster.
        let mut representatives: Vec<f64> = Vec::new();
        for y in ys {
            if representatives
                .last()
                .is_none_or(|&rep| (y - rep).abs() >= 5.0)
            {
                representatives.push(y);
            }
        }
        representatives
    }

    /// Find the index of the column boundary closest to `x`.
    fn find_column(x: f64, boundaries: &[f64]) -> usize {
        boundaries
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (x - **a)
                    .abs()
                    .partial_cmp(&(x - **b).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    /// Find the index of the row boundary closest to `y`.
    ///
    /// `boundaries` are sorted descending (top of page first).
    fn find_row(y: f64, boundaries: &[f64]) -> usize {
        boundaries
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (y - **a)
                    .abs()
                    .partial_cmp(&(y - **b).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{make_block_from_line, make_bold_line, make_line};

    /// Helper: create a TextBlock at the given (left, top) with the given text and index.
    fn make_cell(text: &str, left: f64, top: f64, idx: usize) -> TextBlock {
        let line = make_line(text, left, top, 10.0, 0);
        make_block_from_line(line, idx)
    }

    /// A simple 2x2 grid: 2 header cells and 2 data cells.
    ///
    /// Layout (Y-up coordinates):
    ///   Row 0 (header): "Name" at (50, 100), "Value" at (150, 100)
    ///   Row 1 (data):   "Alice" at (50,  80), "42"    at (150,  80)
    #[test]
    fn test_build_simple_2x2_grid() {
        let b0 = make_cell("Name", 50.0, 100.0, 0);
        let b1 = make_cell("Value", 150.0, 100.0, 1);
        let b2 = make_cell("Alice", 50.0, 80.0, 2);
        let b3 = make_cell("42", 150.0, 80.0, 3);

        let blocks = vec![b0, b1, b2, b3];
        let indices: Vec<usize> = (0..4).collect();
        let table_bbox = Rect::new(40.0, 110.0, 220.0, 70.0);

        let grid = GridBuilder::build(&indices, &blocks, &table_bbox, false);

        assert_eq!(grid.column_count, 2, "expected 2 columns");
        assert_eq!(grid.header.len(), 1, "expected 1 header row");
        assert_eq!(grid.rows.len(), 1, "expected 1 data row");

        // Header row should contain "Name" and "Value" (order matches column sort).
        assert!(
            grid.header[0].contains(&"Name".to_string())
                || grid.header[0].contains(&"Value".to_string()),
            "header should contain Name or Value"
        );

        // Data row should contain "Alice" and "42".
        assert!(
            grid.rows[0].contains(&"Alice".to_string()) || grid.rows[0].contains(&"42".to_string()),
            "data row should contain Alice or 42"
        );
    }

    /// Empty block_indices must produce a zero-column empty grid.
    #[test]
    fn test_empty_blocks_returns_empty_grid() {
        let table_bbox = Rect::new(0.0, 100.0, 200.0, 0.0);
        let grid = GridBuilder::build(&[], &[], &table_bbox, false);

        assert_eq!(grid.column_count, 0);
        assert!(grid.header.is_empty());
        assert!(grid.rows.is_empty());
        assert!(!grid.has_multi_header);
    }

    /// Blocks at x=50 and x=52 (within 5pt tolerance) belong to the same column.
    /// A block at x=150 (>5pt away) belongs to a different column.
    #[test]
    fn test_column_detection_with_tolerance() {
        // Two blocks close together in X → same column.
        // One block far away → second column.
        let b0 = make_cell("A", 50.0, 100.0, 0);
        let b1 = make_cell("B", 52.0, 80.0, 1); // within 5pt of 50.0
        let b2 = make_cell("C", 150.0, 100.0, 2); // different column
        let b3 = make_cell("D", 150.0, 80.0, 3); // same column as C

        let blocks = vec![b0, b1, b2, b3];
        let indices: Vec<usize> = (0..4).collect();
        let table_bbox = Rect::new(40.0, 110.0, 200.0, 70.0);

        let grid = GridBuilder::build(&indices, &blocks, &table_bbox, false);

        assert_eq!(
            grid.column_count, 2,
            "x=50 and x=52 collapse to 1 column; x=150 is another"
        );
    }

    /// A single-row table: all blocks at the same Y become the header row with no data rows.
    #[test]
    fn test_single_row_becomes_header_only() {
        let b0 = make_cell("Col1", 50.0, 100.0, 0);
        let b1 = make_cell("Col2", 150.0, 100.0, 1);
        let b2 = make_cell("Col3", 250.0, 100.0, 2);

        let blocks = vec![b0, b1, b2];
        let indices: Vec<usize> = (0..3).collect();
        let table_bbox = Rect::new(40.0, 110.0, 300.0, 85.0);

        let grid = GridBuilder::build(&indices, &blocks, &table_bbox, false);

        assert_eq!(grid.column_count, 3);
        assert_eq!(grid.header.len(), 1, "single row should become the header");
        assert!(grid.rows.is_empty(), "no data rows for single-row table");
    }

    /// Verify that cell text is placed in the correct column slots for a 3-column table.
    #[test]
    fn test_3x2_grid_cell_placement() {
        // Row 0 (header): "Name", "Score", "Grade" at y=200
        // Row 1 (data):   "Bob",  "95",    "A"     at y=180
        let b0 = make_cell("Name", 50.0, 200.0, 0);
        let b1 = make_cell("Score", 150.0, 200.0, 1);
        let b2 = make_cell("Grade", 250.0, 200.0, 2);
        let b3 = make_cell("Bob", 50.0, 180.0, 3);
        let b4 = make_cell("95", 150.0, 180.0, 4);
        let b5 = make_cell("A", 250.0, 180.0, 5);

        let blocks = vec![b0, b1, b2, b3, b4, b5];
        let indices: Vec<usize> = (0..6).collect();
        let table_bbox = Rect::new(40.0, 210.0, 310.0, 170.0);

        let grid = GridBuilder::build(&indices, &blocks, &table_bbox, false);

        assert_eq!(grid.column_count, 3);
        assert_eq!(grid.header.len(), 1);
        assert_eq!(grid.rows.len(), 1);
        assert_eq!(grid.header[0].len(), 3);
        assert_eq!(grid.rows[0].len(), 3);

        // Column 0 should be "Name" / "Bob"
        assert_eq!(grid.header[0][0], "Name");
        assert_eq!(grid.rows[0][0], "Bob");

        // Column 1 should be "Score" / "95"
        assert_eq!(grid.header[0][1], "Score");
        assert_eq!(grid.rows[0][1], "95");

        // Column 2 should be "Grade" / "A"
        assert_eq!(grid.header[0][2], "Grade");
        assert_eq!(grid.rows[0][2], "A");
    }

    /// A table whose first data row is entirely bold should be promoted to a second header row,
    /// and `has_multi_header` should be set to `true`.
    #[test]
    fn test_multi_header_flag_promoted_when_first_data_row_is_bold() {
        // Row 0 (header, normal): "Group", "Metric" at y=200
        // Row 1 (sub-header, bold): "A", "B" at y=180  ← should be promoted
        // Row 2 (data, normal): "1", "2" at y=160
        let b0 = make_block_from_line(make_line("Group", 50.0, 200.0, 10.0, 0), 0);
        let b1 = make_block_from_line(make_line("Metric", 150.0, 200.0, 10.0, 0), 1);
        let b2 = make_block_from_line(make_bold_line("A", 50.0, 180.0, 10.0, 0), 2);
        let b3 = make_block_from_line(make_bold_line("B", 150.0, 180.0, 10.0, 0), 3);
        let b4 = make_block_from_line(make_line("1", 50.0, 160.0, 10.0, 0), 4);
        let b5 = make_block_from_line(make_line("2", 150.0, 160.0, 10.0, 0), 5);

        let blocks = vec![b0, b1, b2, b3, b4, b5];
        let indices: Vec<usize> = (0..6).collect();
        let table_bbox = Rect::new(40.0, 210.0, 220.0, 150.0);

        let grid = GridBuilder::build(&indices, &blocks, &table_bbox, false);

        // The bold row should be promoted to the header.
        assert_eq!(
            grid.header.len(),
            2,
            "bold sub-header row should be promoted"
        );
        assert!(grid.has_multi_header, "has_multi_header should be true");
        // Data rows should only include the non-bold row.
        assert_eq!(grid.rows.len(), 1, "only one non-bold data row remaining");
    }

    /// `has_multi_header` should be `false` when the first data row is NOT bold.
    #[test]
    fn test_multi_header_flag_not_set_when_data_row_not_bold() {
        // Row 0 (header): "Name", "Value" at y=100
        // Row 1 (data, not bold): "Alice", "42" at y=80
        let b0 = make_block_from_line(make_line("Name", 50.0, 100.0, 10.0, 0), 0);
        let b1 = make_block_from_line(make_line("Value", 150.0, 100.0, 10.0, 0), 1);
        let b2 = make_block_from_line(make_line("Alice", 50.0, 80.0, 10.0, 0), 2);
        let b3 = make_block_from_line(make_line("42", 150.0, 80.0, 10.0, 0), 3);

        let blocks = vec![b0, b1, b2, b3];
        let indices: Vec<usize> = (0..4).collect();
        let table_bbox = Rect::new(40.0, 110.0, 220.0, 70.0);

        let grid = GridBuilder::build(&indices, &blocks, &table_bbox, false);

        assert_eq!(grid.header.len(), 1, "only one header row");
        assert!(!grid.has_multi_header, "has_multi_header should be false");
        assert_eq!(grid.rows.len(), 1, "data row should remain");
    }

    /// A freshly built `Cell` defaults to an unmerged `colspan = 1, rowspan = 1`.
    #[test]
    fn cell_default_span_is_one() {
        let cell = Cell::new("x");
        assert_eq!(cell.text, "x");
        assert_eq!(cell.colspan, 1);
        assert_eq!(cell.rowspan, 1);
    }

    /// P2-8 best-effort colspan inference: a header cell ("Group") is placed
    /// in only one column, leaving the other columns in its row blank — the
    /// common "grouped header" layout where a single spanning label is
    /// visually centered/left over several sub-columns. The blank run to
    /// its right should be folded into its `colspan`.
    #[test]
    fn colspan_inferred_when_span_wide() {
        // Row 0 (header): "Group" only at column 0; columns 1/2 blank under it.
        // Row 1 (sub-header, bold, promoted): "A", "B", "C".
        // Row 2 (data): "1", "2", "3".
        let group = make_block_from_line(make_line("Group", 50.0, 200.0, 10.0, 0), 0);
        let a = make_block_from_line(make_bold_line("A", 50.0, 180.0, 10.0, 0), 1);
        let b = make_block_from_line(make_bold_line("B", 150.0, 180.0, 10.0, 0), 2);
        let c = make_block_from_line(make_bold_line("C", 250.0, 180.0, 10.0, 0), 3);
        let d1 = make_block_from_line(make_line("1", 50.0, 160.0, 10.0, 0), 4);
        let d2 = make_block_from_line(make_line("2", 150.0, 160.0, 10.0, 0), 5);
        let d3 = make_block_from_line(make_line("3", 250.0, 160.0, 10.0, 0), 6);

        let blocks = vec![group, a, b, c, d1, d2, d3];
        let indices: Vec<usize> = (0..7).collect();
        let table_bbox = Rect::new(40.0, 210.0, 300.0, 150.0);

        let grid = GridBuilder::build(&indices, &blocks, &table_bbox, false);

        assert_eq!(
            grid.header.len(),
            2,
            "bold sub-header row should be promoted"
        );
        assert_eq!(grid.header_rows.len(), 2);
        assert_eq!(
            grid.header_rows[0][0].colspan, 3,
            "Group cell should span all 3 columns"
        );
        assert_eq!(grid.header_rows[0][1].colspan, 0, "covered by Group's span");
        assert_eq!(grid.header_rows[0][2].colspan, 0, "covered by Group's span");
        // The sub-header row itself is fully populated, so it stays unmerged.
        for cell in &grid.header_rows[1] {
            assert_eq!(cell.colspan, 1);
            assert_eq!(cell.rowspan, 1);
        }
    }

    /// When no header row has an empty-cell run, every inferred `Cell` stays
    /// at the default `colspan = 1, rowspan = 1` (the common, non-merged case).
    #[test]
    fn no_span_inferred_when_header_fully_populated() {
        let grid = GridBuilder::build(
            &(0..4).collect::<Vec<usize>>(),
            &[
                make_cell("Name", 50.0, 100.0, 0),
                make_cell("Value", 150.0, 100.0, 1),
                make_cell("Alice", 50.0, 80.0, 2),
                make_cell("42", 150.0, 80.0, 3),
            ],
            &Rect::new(40.0, 110.0, 220.0, 70.0),
            false,
        );

        assert_eq!(grid.header_rows.len(), 1);
        for cell in &grid.header_rows[0] {
            assert_eq!(cell.colspan, 1);
            assert_eq!(cell.rowspan, 1);
        }
    }
}
