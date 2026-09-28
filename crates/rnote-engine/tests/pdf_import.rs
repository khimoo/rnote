//! Pins the behaviour of the upstream Pdf import so the hayro update cannot change it unnoticed.

use parry2d_f64::math::Vector2;
use rnote_compose::shapes::Shapeable;
use rnote_engine::document::Format;
use rnote_engine::engine::import::PdfImportPrefs;
use rnote_engine::strokes::{BitmapImage, VectorImage};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/pdf/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path} failed: {e}"))
}

fn assert_a4_portrait(width: f64, height: f64) {
    let ratio = height / width;
    assert!((ratio - 297.0 / 210.0).abs() < 0.01, "ratio {ratio}");
}

#[test]
fn bitmap_import_creates_one_image_per_page() {
    let images = BitmapImage::from_pdf_bytes(
        &fixture("vector.pdf"),
        PdfImportPrefs::default(),
        Vector2::ZERO,
        None,
        &Format::default(),
        None,
    )
    .unwrap();
    assert_eq!(images.len(), 3);
    for image in &images {
        let extents = image.bounds().extents();
        assert_a4_portrait(extents[0], extents[1]);
        assert!(image.image.pixel_width > 100);
        assert_a4_portrait(
            image.image.pixel_width as f64,
            image.image.pixel_height as f64,
        );
    }
}

#[test]
fn vector_import_creates_one_svg_per_page() {
    let images = VectorImage::from_pdf_bytes(
        &fixture("scan.pdf"),
        PdfImportPrefs::default(),
        Vector2::ZERO,
        None,
        &Format::default(),
        None,
    )
    .unwrap();
    assert_eq!(images.len(), 2);
    for image in &images {
        let extents = image.bounds().extents();
        assert_a4_portrait(extents[0], extents[1]);
        assert!(image.svg_data.contains("<image"));
    }
}
