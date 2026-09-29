//! Lays out referenced Pdf pages in rows per section, see [crate::strokes::PdfPage].

use crate::Engine;
use crate::pdfsource::PdfSource;
use crate::store::chrono_comp::StrokeLayer;
use crate::strokes::{PdfPage, Stroke};
use anyhow::bail;
use p2d::math::Vector2;
use rnote_compose::shapes::Rectangle;
use sections::Section;
use std::ops::Range;
use std::sync::Arc;

pub mod sections;

/// Top-left offsets of `sizes`, the Pdf pages `first_page..`, laid out in one run of rows per
/// section and indented by the section depth in columns. A page belongs to the last section
/// starting at or before it, except that of several sections starting on it the first one takes
/// it, so a parent keeps the page it shares with its first child. Pages before all sections run
/// at depth 0, so without sections this is a plain grid. Runs wrap after `columns` pages; every
/// column is as wide as the widest page and every row as tall as its tallest page, with `gap`
/// between them. `sections` must be sorted by start, as [sections::parse_sections] returns them.
pub fn section_offsets(
    sizes: &[Vector2],
    first_page: usize,
    sections: &[Section],
    columns: usize,
    gap: f64,
) -> Vec<Vector2> {
    let columns = columns.max(1);
    let column_step = sizes.iter().map(|size| size[0]).fold(0.0, f64::max) + gap;
    let owner = |page: usize| -> Option<usize> {
        let started = sections.partition_point(|section| section.start <= page);
        let last = started.checked_sub(1)?;
        if sections[last].start == page {
            Some(sections.partition_point(|section| section.start < page))
        } else {
            Some(last)
        }
    };
    let mut offsets = Vec::with_capacity(sizes.len());
    let mut row_top = 0.0;
    let mut start = 0;
    while start < sizes.len() {
        let run_owner = owner(first_page + start);
        let end = (start..sizes.len())
            .find(|&i| owner(first_page + i) != run_owner)
            .unwrap_or(sizes.len());
        let indent = run_owner.map_or(0, |i| sections[i].depth) as f64 * column_step;
        for row in sizes[start..end].chunks(columns) {
            for column in 0..row.len() {
                offsets.push(Vector2::new(indent + column as f64 * column_step, row_top));
            }
            row_top += row.iter().map(|size| size[1]).fold(0.0, f64::max) + gap;
        }
        start = end;
    }
    offsets
}

impl Engine {
    /// Page strokes referencing `pages` of `source`, scaled so the first page spans the document
    /// width and laid out by [section_offsets] from `insert_pos`. `gap_ratio` is relative to the
    /// column width.
    pub fn generate_pdfpage_strokes(
        &self,
        source: &Arc<PdfSource>,
        pages: Range<usize>,
        sections: &[Section],
        columns: usize,
        gap_ratio: f64,
        insert_pos: Vector2,
    ) -> anyhow::Result<Vec<(Stroke, Option<StrokeLayer>)>> {
        if pages.is_empty() || pages.end > source.page_count() {
            bail!(
                "pages {}..{} are outside of the {} pages of '{}'",
                pages.start + 1,
                pages.end,
                source.page_count(),
                source.path().display()
            );
        }
        let first = source.page_size(pages.start).unwrap_or_default();
        let zoom = self.document.config.format.width() / first[0];
        let sizes: Vec<Vector2> = pages
            .clone()
            .map(|i| source.page_size(i).unwrap_or_default() * zoom)
            .collect();
        let column_width = sizes.iter().map(|size| size[0]).fold(0.0, f64::max);
        let offsets = section_offsets(
            &sizes,
            pages.start,
            sections,
            columns,
            column_width * gap_ratio,
        );
        Ok(pages
            .zip(sizes.into_iter().zip(offsets))
            .map(|(page_index, (size, offset))| {
                let top_left = insert_pos + offset;
                let rectangle = Rectangle::from_corners(top_left, top_left + size);
                (
                    Stroke::PdfPage(PdfPage::new(Arc::clone(source), page_index, rectangle)),
                    Some(StrokeLayer::Document),
                )
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::sections::Section;
    use super::*;
    use rnote_compose::shapes::Shapeable;
    use std::path::PathBuf;

    fn v(x: f64, y: f64) -> Vector2 {
        Vector2::new(x, y)
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pdf")
            .join(name)
    }

    #[test]
    fn grid_rows_take_the_tallest_page_and_columns_the_widest() {
        let sizes = [v(10.0, 20.0), v(30.0, 10.0), v(10.0, 5.0), v(10.0, 20.0)];
        let offsets = section_offsets(&sizes, 0, &[], 2, 1.0);
        assert_eq!(
            offsets,
            vec![v(0.0, 0.0), v(31.0, 0.0), v(0.0, 21.0), v(31.0, 21.0)]
        );
    }

    #[test]
    fn grid_with_zero_columns_uses_one() {
        let offsets = section_offsets(&[v(1.0, 1.0), v(1.0, 1.0)], 0, &[], 0, 0.0);
        assert_eq!(offsets, vec![v(0.0, 0.0), v(0.0, 1.0)]);
    }

    #[test]
    fn generated_pages_scale_to_the_document_width_in_a_grid() {
        let source = PdfSource::open(&fixture("mixed.pdf")).unwrap();
        let engine = Engine::default();
        let width = engine.document.config.format.width();
        let strokes = engine
            .generate_pdfpage_strokes(&source, 0..3, &[], 2, 0.1, v(0.0, 0.0))
            .unwrap();
        assert_eq!(strokes.len(), 3);
        let pages: Vec<&PdfPage> = strokes
            .iter()
            .map(|(stroke, layer)| {
                assert!(matches!(layer, Some(StrokeLayer::Document)));
                match stroke {
                    Stroke::PdfPage(page) => page,
                    other => panic!("unexpected {other:?}"),
                }
            })
            .collect();
        let first = pages[0].bounds();
        assert!((first.extents()[0] - width).abs() < 1e-6);
        // The landscape page is the widest, so it sets the column width.
        let zoom = width / 595.28;
        let column = 841.89 * zoom;
        assert!((pages[1].bounds().mins[0] - column * 1.1).abs() < 1.0);
        assert!(pages[2].bounds().mins[1] > first.maxs[1]);
    }

    #[test]
    fn out_of_range_pages_are_rejected() {
        let source = PdfSource::open(&fixture("vector.pdf")).unwrap();
        let engine = Engine::default();
        assert!(
            engine
                .generate_pdfpage_strokes(&source, 2..4, &[], 8, 0.1, v(0.0, 0.0))
                .is_err()
        );
        assert!(
            engine
                .generate_pdfpage_strokes(&source, 1..1, &[], 8, 0.1, v(0.0, 0.0))
                .is_err()
        );
    }

    fn sec(start: usize, depth: usize) -> Section {
        Section { start, depth }
    }

    #[test]
    fn sections_start_new_rows_indented_by_their_depth() {
        let sizes = [v(1.0, 1.0); 6];
        let sections = [sec(0, 0), sec(2, 1), sec(5, 0)];
        assert_eq!(
            section_offsets(&sizes, 0, &sections, 2, 0.0),
            vec![
                v(0.0, 0.0),
                v(1.0, 0.0),
                // The child wraps at its own indent.
                v(1.0, 1.0),
                v(2.0, 1.0),
                v(1.0, 2.0),
                v(0.0, 3.0),
            ]
        );
    }

    #[test]
    fn a_page_shared_by_parent_and_child_stays_with_the_parent() {
        let sizes = [v(1.0, 1.0); 3];
        let sections = [sec(0, 0), sec(0, 1), sec(2, 1)];
        assert_eq!(
            section_offsets(&sizes, 0, &sections, 4, 0.0),
            vec![v(0.0, 0.0), v(1.0, 1.0), v(1.0, 2.0)]
        );
    }

    #[test]
    fn sections_without_pages_leave_no_row() {
        let sizes = [v(1.0, 1.0); 3];
        // The depth-2 section starts on page 1 too, so the depth-1 one takes that page.
        let sections = [sec(0, 0), sec(1, 1), sec(1, 2), sec(2, 1)];
        assert_eq!(
            section_offsets(&sizes, 0, &sections, 4, 0.0),
            vec![v(0.0, 0.0), v(1.0, 1.0), v(1.0, 2.0)]
        );
    }

    #[test]
    fn pages_before_the_first_section_form_a_row_at_depth_zero() {
        let sizes = [v(1.0, 1.0); 3];
        assert_eq!(
            section_offsets(&sizes, 0, &[sec(2, 1)], 4, 0.0),
            vec![v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0)]
        );
    }

    #[test]
    fn a_range_starting_mid_section_keeps_that_sections_indent() {
        // Pdf pages 10..13; the sections at 0 and 20 lie outside of them and make no row.
        let sizes = [v(1.0, 1.0); 3];
        let sections = [sec(0, 0), sec(5, 1), sec(12, 2), sec(20, 0)];
        assert_eq!(
            section_offsets(&sizes, 10, &sections, 4, 0.0),
            vec![v(1.0, 0.0), v(2.0, 0.0), v(2.0, 1.0)]
        );
    }

    #[test]
    fn the_indent_step_includes_the_gap() {
        let sizes = [v(2.0, 1.0), v(2.0, 1.0)];
        assert_eq!(
            section_offsets(&sizes, 0, &[sec(0, 0), sec(1, 1)], 4, 0.5),
            vec![v(0.0, 0.0), v(2.5, 1.5)]
        );
    }

    #[test]
    fn generated_pages_follow_the_sections() {
        let source = PdfSource::open(&fixture("vector.pdf")).unwrap();
        let engine = Engine::default();
        let strokes = engine
            .generate_pdfpage_strokes(&source, 0..3, &[sec(0, 0), sec(1, 1)], 8, 0.1, v(0.0, 0.0))
            .unwrap();
        let bounds: Vec<_> = strokes.iter().map(|(stroke, _)| stroke.bounds()).collect();
        // Page 1 starts an indented row below page 0, and page 2 continues that row.
        assert!(bounds[1].mins[0] > bounds[0].maxs[0]);
        assert!(bounds[1].mins[1] > bounds[0].maxs[1]);
        assert!((bounds[2].mins[1] - bounds[1].mins[1]).abs() < 1e-9);
        assert!(bounds[2].mins[0] > bounds[1].maxs[0]);
    }
}
