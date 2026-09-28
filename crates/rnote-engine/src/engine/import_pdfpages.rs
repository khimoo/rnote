//! Lays out referenced Pdf pages in a grid, see [crate::strokes::PdfPage].

use crate::Engine;
use crate::pdfsource::PdfSource;
use crate::store::chrono_comp::StrokeLayer;
use crate::strokes::{PdfPage, Stroke};
use anyhow::bail;
use p2d::math::Vector2;
use rnote_compose::shapes::Rectangle;
use std::ops::Range;
use std::sync::Arc;

/// Top-left offsets of pages laid out in rows of `columns`. Every column is as wide as the
/// widest page and every row as tall as its tallest page, with `gap` between them.
pub fn grid_offsets(sizes: &[Vector2], columns: usize, gap: f64) -> Vec<Vector2> {
    let columns = columns.max(1);
    let column_width = sizes.iter().map(|size| size[0]).fold(0.0, f64::max);
    let mut offsets = Vec::with_capacity(sizes.len());
    let mut row_top = 0.0;
    for row in sizes.chunks(columns) {
        for column in 0..row.len() {
            offsets.push(Vector2::new(column as f64 * (column_width + gap), row_top));
        }
        row_top += row.iter().map(|size| size[1]).fold(0.0, f64::max) + gap;
    }
    offsets
}

impl Engine {
    /// Page strokes referencing `pages` of `source`, scaled so the first page spans the document
    /// width and laid out by [grid_offsets] from `insert_pos`. `gap_ratio` is relative to the
    /// column width.
    pub fn generate_pdfpage_strokes(
        &self,
        source: &Arc<PdfSource>,
        pages: Range<usize>,
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
        let offsets = grid_offsets(&sizes, columns, column_width * gap_ratio);
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
        let offsets = grid_offsets(&sizes, 2, 1.0);
        assert_eq!(
            offsets,
            vec![v(0.0, 0.0), v(31.0, 0.0), v(0.0, 21.0), v(31.0, 21.0)]
        );
    }

    #[test]
    fn grid_with_zero_columns_uses_one() {
        let offsets = grid_offsets(&[v(1.0, 1.0), v(1.0, 1.0)], 0, 0.0);
        assert_eq!(offsets, vec![v(0.0, 0.0), v(0.0, 1.0)]);
    }

    #[test]
    fn generated_pages_scale_to_the_document_width_in_a_grid() {
        let source = PdfSource::open(&fixture("mixed.pdf")).unwrap();
        let engine = Engine::default();
        let width = engine.document.config.format.width();
        let strokes = engine
            .generate_pdfpage_strokes(&source, 0..3, 2, 0.1, v(0.0, 0.0))
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
                .generate_pdfpage_strokes(&source, 2..4, 8, 0.1, v(0.0, 0.0))
                .is_err()
        );
        assert!(
            engine
                .generate_pdfpage_strokes(&source, 1..1, 8, 0.1, v(0.0, 0.0))
                .is_err()
        );
    }
}
