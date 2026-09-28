//! Measures what Pdf page strokes allocate for a large Pdf.
//!
//! typst compile --root crates/rnote-engine crates/rnote-engine/examples/pdfpage_bench.typ /tmp/bench.pdf
//! cargo run -p rnote-engine --example pdfpage_bench -- /tmp/bench.pdf [--bitmap-baseline]

use parry2d_f64::bounding_volume::{Aabb, BoundingVolume};
use parry2d_f64::math::Vector2;
use rnote_compose::shapes::{Rectangle, Shapeable};
use rnote_engine::document::Format;
use rnote_engine::engine::import::PdfImportPrefs;
use rnote_engine::engine::import_pdfpages::grid_offsets;
use rnote_engine::pdfsource::PdfSource;
use rnote_engine::strokes::{BitmapImage, Content, PdfPage};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

struct Tracking;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Tracking = Tracking;

fn mib(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// Prints what is allocated now and the peak and time since the previous report, above `base`.
fn report(label: &str, base: usize, since: &mut Instant) {
    println!(
        "{label:<34} retained {:>8.1} MiB  peak {:>8.1} MiB  {:>7.2} s",
        mib(CURRENT.load(Ordering::Relaxed).saturating_sub(base)),
        mib(PEAK.load(Ordering::Relaxed).saturating_sub(base)),
        since.elapsed().as_secs_f64(),
    );
    PEAK.store(CURRENT.load(Ordering::Relaxed), Ordering::Relaxed);
    *since = Instant::now();
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .expect("usage: pdfpage_bench <file.pdf> [--bitmap-baseline]");
    let base = CURRENT.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let mut since = Instant::now();

    let source = PdfSource::open(path.as_ref())?;
    let zoom = Format::default().width() / source.page_size(0).unwrap()[0];
    let sizes: Vec<Vector2> = (0..source.page_count())
        .map(|i| source.page_size(i).unwrap() * zoom)
        .collect();
    let offsets = grid_offsets(&sizes, 8, sizes[0][0] * 0.1);
    let pages: Vec<PdfPage> = sizes
        .iter()
        .zip(&offsets)
        .enumerate()
        .map(|(i, (size, offset))| {
            PdfPage::new(
                source.clone(),
                i,
                Rectangle::from_corners(*offset, *offset + *size),
            )
        })
        .collect();
    report(&format!("import, {} pages", pages.len()), base, &mut since);

    // The whole grid on a 1920 px wide screen, with every page's image kept like the renderer does.
    let all = pages
        .iter()
        .map(|page| page.bounds())
        .reduce(|a, b| a.merged(&b))
        .unwrap();
    let overview_scale = 1920.0 / all.extents()[0];
    let kept: Vec<_> = pages
        .iter()
        .map(|page| page.gen_images(all, overview_scale))
        .collect::<Result<_, _>>()?;
    report("overview, all pages kept", base, &mut since);
    drop(kept);

    // Zoom from the overview into the first page and back, three times.
    let first = pages[0].bounds();
    for _ in 0..3 {
        for step in (0..12).chain((0..12).rev()) {
            let factor = 2f64.powi(step);
            let half = first.extents() / (2.0 * factor);
            let viewport = Aabb::new(first.center() - half, first.center() + half);
            let images: Vec<_> = pages[..16]
                .iter()
                .map(|page| page.gen_images(viewport, overview_scale * factor))
                .collect::<Result<_, _>>()?;
            drop(images);
        }
    }
    report("zoom sweep x3, first 16 pages", base, &mut since);
    println!("page image cache {:.1} MiB", mib(source.cache_used_bytes()));
    drop(pages);
    drop(source);
    report("pages dropped", base, &mut since);

    if std::env::args().any(|arg| arg == "--bitmap-baseline") {
        let bytes = std::fs::read(&path)?;
        let images = BitmapImage::from_pdf_bytes(
            &bytes,
            PdfImportPrefs::default(),
            Vector2::ZERO,
            None,
            &Format::default(),
            None,
        )?;
        drop(bytes);
        report("upstream bitmap import", base, &mut since);
        drop(images);
    }
    Ok(())
}
