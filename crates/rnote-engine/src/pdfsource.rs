//! Pdf documents shared by all [crate::strokes::PdfPage] strokes that reference them.
//!
//! One [PdfSource] exists per file and process. Pages are rendered on demand, either whole at a
//! power-of-two resolution kept in a byte-bounded cache, or only the requested region.

use crate::Image;
use crate::image::ImageMemoryFormat;
use anyhow::{Context, anyhow};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::Rect;
use hayro::vello_cpu::color::palette::css::{TRANSPARENT, WHITE};
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use p2d::bounding_volume::Aabb;
use p2d::math::Vector2;
use rnote_compose::shapes::Rectangle;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};

pub use hayro::kurbo::Affine as PdfAffine;

/// Longest side of a whole-page image. Above it only the visible region is rendered.
pub const WHOLE_PAGE_MAX_PIXELS: f64 = 4096.0;
/// Byte budget of the whole-page images kept per [PdfSource].
pub const DEFAULT_CACHE_BUDGET: usize = 512 * 1024 * 1024;
/// Longest side of a region buffer (256 MiB). Viewports stay far below it; vello panics near
/// `u16::MAX` because it snaps sizes up to whole tiles.
const MAX_BUFFER_SIDE: u32 = 8192;
/// 1/64 px per point: a page is still a few pixels wide.
const MIN_LEVEL: i32 = -6;

static REGISTRY: LazyLock<Mutex<HashMap<PathBuf, Weak<PdfSource>>>> =
    LazyLock::new(Default::default);

/// Identifies the file a page was imported from, so that a replaced file is not rendered as the original.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, rename = "pdf_hash")]
pub struct PdfHash {
    #[serde(rename = "crc32")]
    pub crc32: u32,
    #[serde(rename = "len")]
    pub len: u64,
}

impl PdfHash {
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            crc32: crc32fast::hash(bytes),
            len: bytes.len() as u64,
        }
    }
}

/// Premultiplied Rgba8 pixels of a page, shared with the images handed to the renderer.
#[derive(Clone)]
pub struct PageRaster {
    pub width: u32,
    pub height: u32,
    pub data: glib::Bytes,
}

impl fmt::Debug for PageRaster {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageRaster")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.byte_len())
            .finish()
    }
}

impl PageRaster {
    /// A single gray pixel, stretched over pages whose Pdf can not be rendered.
    pub fn placeholder() -> Self {
        Self {
            width: 1,
            height: 1,
            data: glib::Bytes::from_static(&[200, 200, 200, 255]),
        }
    }

    pub fn byte_len(&self) -> usize {
        self.data.len()
    }

    pub fn into_image(self, rectangle: Rectangle) -> Image {
        Image {
            data: self.data,
            rectangle,
            pixel_width: self.width,
            pixel_height: self.height,
            memory_format: ImageMemoryFormat::R8g8b8a8Premultiplied,
        }
    }

    fn from_pixmap(pixmap: Pixmap) -> Self {
        let (width, height) = (pixmap.width() as u32, pixmap.height() as u32);
        let data = pixmap.take_rgba8(hayro::vello_cpu::peniko::ImageAlphaType::AlphaPremultiplied);
        Self {
            width,
            height,
            data: glib::Bytes::from_owned(data),
        }
    }
}

type CacheKey = (usize, i32);

/// Whole-page images keyed by page index and level, evicted least-recently-used first.
struct PageImageCache {
    budget: usize,
    used: usize,
    tick: u64,
    entries: HashMap<CacheKey, (PageRaster, u64)>,
}

impl PageImageCache {
    fn new(budget: usize) -> Self {
        Self {
            budget,
            used: 0,
            tick: 0,
            entries: HashMap::new(),
        }
    }

    fn used(&self) -> usize {
        self.used
    }

    fn get(&mut self, key: &CacheKey) -> Option<PageRaster> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(key).map(|(raster, last_used)| {
            *last_used = tick;
            raster.clone()
        })
    }

    fn insert(&mut self, key: CacheKey, raster: PageRaster) {
        if raster.byte_len() > self.budget {
            return;
        }
        if let Some((old, _)) = self.entries.remove(&key) {
            self.used -= old.byte_len();
        }
        while self.used + raster.byte_len() > self.budget {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, last_used))| *last_used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some((evicted, _)) = self.entries.remove(&oldest) {
                self.used -= evicted.byte_len();
            }
        }
        self.tick += 1;
        self.used += raster.byte_len();
        self.entries.insert(key, (raster, self.tick));
    }
}

/// A Pdf file opened once per process and shared by all pages that reference it.
pub struct PdfSource {
    path: PathBuf,
    hash: PdfHash,
    pdf: Pdf,
    page_sizes: Vec<Vector2>,
    cache: Mutex<PageImageCache>,
}

impl fmt::Debug for PdfSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PdfSource")
            .field("path", &self.path)
            .field("hash", &self.hash)
            .field("pages", &self.page_sizes.len())
            .finish()
    }
}

impl PdfSource {
    /// The shared source for the file at `path`, opening it when no page references it yet.
    pub fn open(path: &Path) -> anyhow::Result<Arc<Self>> {
        let path = std::fs::canonicalize(path)
            .with_context(|| format!("resolving '{}'", path.display()))?;
        let mut registry = REGISTRY.lock().unwrap();
        if let Some(source) = registry.get(&path).and_then(Weak::upgrade) {
            return Ok(source);
        }
        registry.retain(|_, source| source.strong_count() > 0);
        let source = Arc::new(Self::load(path.clone())?);
        registry.insert(path, Arc::downgrade(&source));
        Ok(source)
    }

    fn load(path: PathBuf) -> anyhow::Result<Self> {
        let bytes =
            std::fs::read(&path).with_context(|| format!("reading '{}'", path.display()))?;
        let hash = PdfHash::of(&bytes);
        let pdf = Pdf::new(Arc::new(bytes))
            .map_err(|e| anyhow!("parsing '{}' failed, Err: {e:?}", path.display()))?;
        let page_sizes = pdf
            .pages()
            .iter()
            .map(|page| {
                let (width, height) = page.render_dimensions();
                Vector2::new(width as f64, height as f64)
            })
            .collect();
        Ok(Self {
            path,
            hash,
            pdf,
            page_sizes,
            cache: Mutex::new(PageImageCache::new(DEFAULT_CACHE_BUDGET)),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn hash(&self) -> PdfHash {
        self.hash
    }

    pub fn page_count(&self) -> usize {
        self.page_sizes.len()
    }

    /// Size of the rendered page in points, with the page rotation applied.
    pub fn page_size(&self, page_index: usize) -> Option<Vector2> {
        self.page_sizes.get(page_index).copied()
    }

    /// The cache level for rendering the whole page at `pixels_per_point`, or `None` when that
    /// image would exceed [WHOLE_PAGE_MAX_PIXELS] and only the visible region should be rendered.
    pub fn whole_page_level(&self, page_index: usize, pixels_per_point: f64) -> Option<i32> {
        let level = (pixels_per_point.log2().ceil() as i32).max(MIN_LEVEL);
        (level <= self.max_whole_page_level(page_index)).then_some(level)
    }

    /// The highest level whose whole-page image stays within [WHOLE_PAGE_MAX_PIXELS].
    pub fn max_whole_page_level(&self, page_index: usize) -> i32 {
        let long_side = self
            .page_size(page_index)
            .map(|size| size.x.max(size.y))
            .unwrap_or(1.0);
        ((WHOLE_PAGE_MAX_PIXELS / long_side).log2().floor() as i32).max(MIN_LEVEL)
    }

    /// The whole page rendered at `2^level` pixels per point, from the cache when possible.
    pub fn page_raster(&self, page_index: usize, level: i32) -> anyhow::Result<PageRaster> {
        let key = (page_index, level);
        if let Some(raster) = self.cache.lock().unwrap().get(&key) {
            return Ok(raster);
        }
        let page = self.page(page_index)?;
        let scale = 2f32.powi(level);
        let cache = hayro::RenderCache::new();
        let pixmap = hayro::render(
            page,
            &cache,
            &InterpreterSettings::default(),
            &hayro::RenderSettings {
                x_scale: scale,
                y_scale: scale,
                bg_color: WHITE,
            },
        );
        let raster = PageRaster::from_pixmap(pixmap);
        self.cache.lock().unwrap().insert(key, raster.clone());
        Ok(raster)
    }

    /// Renders the part of the page that lies in `region` (document coordinates) at `image_scale`
    /// pixels per document unit. `page_to_doc` maps the page, in points with y pointing down, into
    /// the document. The result is not cached.
    pub fn render_region(
        &self,
        page_index: usize,
        page_to_doc: PdfAffine,
        region: Aabb,
        image_scale: f64,
    ) -> anyhow::Result<PageRaster> {
        let page = self.page(page_index)?;
        let extents = region.extents();
        let longest = extents[0].max(extents[1]) * image_scale;
        let image_scale = if longest > MAX_BUFFER_SIDE as f64 {
            image_scale * MAX_BUFFER_SIDE as f64 / longest
        } else {
            image_scale
        };
        let width = ((extents[0] * image_scale).ceil() as u32).clamp(1, MAX_BUFFER_SIDE) as u16;
        let height = ((extents[1] * image_scale).ceil() as u32).clamp(1, MAX_BUFFER_SIDE) as u16;
        let doc_to_buffer = PdfAffine::scale(image_scale)
            * PdfAffine::translate((-region.mins[0], -region.mins[1]));
        let page_to_buffer = doc_to_buffer * page_to_doc;
        let size = self.page_size(page_index).unwrap_or_default();

        let mut ctx = RenderContext::new(width, height);
        // The paper is white; render_into only paints the page content.
        ctx.set_transform(page_to_buffer);
        ctx.set_paint(WHITE);
        ctx.fill_rect(&Rect::new(0.0, 0.0, size.x, size.y));
        let cache = hayro::RenderCache::new();
        hayro::render_into(
            page,
            &cache,
            &InterpreterSettings::default(),
            &mut ctx,
            page_to_buffer * page.initial_transform(true).to_kurbo(),
        );
        ctx.flush();
        let mut pixmap = Pixmap::new(width, height);
        ctx.render_with(
            &mut pixmap,
            &mut Resources::default(),
            RasterizerSettings {
                target_init: TargetInit::Clear(TRANSPARENT),
                ..Default::default()
            },
        );
        Ok(PageRaster::from_pixmap(pixmap))
    }

    /// The page as an Svg document without Xml header, in points.
    pub fn page_svg(&self, page_index: usize) -> anyhow::Result<String> {
        let page = self.page(page_index)?;
        let cache = hayro_svg::RenderCache::new();
        let svg = hayro_svg::convert(
            page,
            &cache,
            &InterpreterSettings::default(),
            &hayro_svg::SvgRenderSettings {
                bg_color: [255, 255, 255, 255],
            },
        );
        let svg = match svg.find("?>") {
            Some(end) if svg.starts_with("<?xml") => svg[end + 2..].trim_start().to_string(),
            _ => svg,
        };
        Ok(svg)
    }

    pub fn cache_used_bytes(&self) -> usize {
        self.cache.lock().unwrap().used()
    }

    fn page(&self, page_index: usize) -> anyhow::Result<&Page<'_>> {
        self.pdf
            .pages()
            .get(page_index)
            .ok_or_else(|| anyhow!("'{}' has no page {page_index}", self.path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pdf")
            .join(name)
    }

    #[test]
    fn open_shares_one_source_per_file() {
        let a = PdfSource::open(&fixture("vector.pdf")).unwrap();
        let b = PdfSource::open(&fixture("vector.pdf")).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.page_count(), 3);
    }

    #[test]
    fn page_sizes_are_in_points_after_rotation() {
        let source = PdfSource::open(&fixture("mixed.pdf")).unwrap();
        let a4 = source.page_size(0).unwrap();
        let landscape = source.page_size(1).unwrap();
        let a5 = source.page_size(2).unwrap();
        assert!((a4.x - 595.28).abs() < 0.5 && (a4.y - 841.89).abs() < 0.5);
        assert!((landscape.x - 841.89).abs() < 0.5 && (landscape.y - 595.28).abs() < 0.5);
        assert!((a5.x - 419.53).abs() < 0.5);
        assert!(source.page_size(3).is_none());
    }

    #[test]
    fn hash_changes_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("copy.pdf");
        std::fs::copy(fixture("vector.pdf"), &path).unwrap();
        let before = PdfSource::open(&path).unwrap().hash();
        std::fs::copy(fixture("scan.pdf"), &path).unwrap();
        assert_ne!(before, PdfHash::of(&std::fs::read(&path).unwrap()));
    }

    #[test]
    fn whole_page_level_switches_to_region_rendering_above_limit() {
        let source = PdfSource::open(&fixture("vector.pdf")).unwrap();
        // A4 is 841.89 pt tall: level 2 (4 px/pt) gives 3368 px, level 3 gives 6735 px.
        assert_eq!(source.whole_page_level(0, 3.5), Some(2));
        assert_eq!(source.whole_page_level(0, 4.5), None);
        assert_eq!(source.max_whole_page_level(0), 2);
        assert_eq!(source.whole_page_level(0, 0.2), Some(-2));
    }

    #[test]
    fn page_raster_is_cached_per_level() {
        let source = PdfSource::open(&fixture("scan.pdf")).unwrap();
        let first = source.page_raster(1, -1).unwrap();
        assert_eq!((first.width, first.height), (297, 420));
        let used = source.cache_used_bytes();
        assert_eq!(used, first.byte_len());
        let second = source.page_raster(1, -1).unwrap();
        assert_eq!(first.data.as_ptr(), second.data.as_ptr());
        assert_eq!(source.cache_used_bytes(), used);
    }

    #[test]
    fn cache_evicts_least_recently_used_over_budget() {
        let mut cache = PageImageCache::new(100);
        let raster = |len: usize| PageRaster {
            width: 1,
            height: 1,
            data: glib::Bytes::from_owned(vec![0u8; len]),
        };
        cache.insert((0, 0), raster(40));
        cache.insert((1, 0), raster(40));
        assert!(cache.get(&(0, 0)).is_some());
        cache.insert((2, 0), raster(40));
        assert!(cache.get(&(1, 0)).is_none());
        assert!(cache.get(&(0, 0)).is_some());
        assert_eq!(cache.used(), 80);
        cache.insert((3, 0), raster(101));
        assert!(cache.get(&(3, 0)).is_none());
        assert_eq!(cache.used(), 80);
    }

    #[test]
    fn region_is_rendered_at_the_requested_scale() {
        let source = PdfSource::open(&fixture("vector.pdf")).unwrap();
        let page = source.page_size(0).unwrap();
        // Page placed at the document origin at 1 doc unit per point.
        let region = Aabb::new(point(100.0, 200.0), point(300.0, 250.0));
        let raster = source
            .render_region(0, PdfAffine::IDENTITY, region, 8.0)
            .unwrap();
        assert_eq!((raster.width, raster.height), (1600, 400));
        assert!(page.x > 300.0);
    }

    #[test]
    fn region_scale_is_reduced_to_fit_u16() {
        let source = PdfSource::open(&fixture("vector.pdf")).unwrap();
        let region = Aabb::new(point(0.0, 0.0), point(500.0, 500.0));
        let raster = source
            .render_region(0, PdfAffine::IDENTITY, region, 200.0)
            .unwrap();
        assert!(raster.width <= MAX_BUFFER_SIDE && raster.height <= MAX_BUFFER_SIDE);
    }

    #[test]
    fn page_svg_contains_the_page() {
        let source = PdfSource::open(&fixture("vector.pdf")).unwrap();
        let svg = source.page_svg(0).unwrap();
        assert!(svg.contains("<svg"));
        assert!(!svg.starts_with("<?xml"));
    }

    fn point(x: f64, y: f64) -> Vector2 {
        Vector2::new(x, y)
    }
}
