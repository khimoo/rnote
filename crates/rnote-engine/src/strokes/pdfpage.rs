//! A Pdf page referenced by path and rendered on demand, see [crate::pdfsource].

use super::Content;
use super::content::GeneratedContentImages;
use crate::pdfsource::{PageRaster, PdfAffine, PdfHash, PdfSource};
use crate::{Drawable, Image, Svg};
use kurbo::Shape;
use p2d::bounding_volume::Aabb;
use p2d::math::Vector2;
use rnote_compose::Transformable;
use rnote_compose::ext::{AabbExt, DAffine2Ext};
use rnote_compose::shapes::{Rectangle, Shapeable};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use tracing::warn;

/// Paths already reported as unrenderable, so a missing book logs once instead of once per page.
static REPORTED: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Default::default);

type Resolved = Result<Arc<PdfSource>, String>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename = "pdfpage")]
pub struct PdfPage {
    /// Absolute path of the referenced Pdf.
    #[serde(rename = "pdf_path")]
    pub pdf_path: PathBuf,
    #[serde(rename = "pdf_hash")]
    pub pdf_hash: PdfHash,
    #[serde(rename = "page_index")]
    pub page_index: usize,
    #[serde(rename = "rectangle")]
    pub rectangle: Rectangle,
    /// Shared between clones, which the history creates on every modification.
    #[serde(skip)]
    source: Arc<OnceLock<Resolved>>,
}

impl PdfPage {
    pub fn new(source: Arc<PdfSource>, page_index: usize, rectangle: Rectangle) -> Self {
        let pdf_path = source.path().to_path_buf();
        let pdf_hash = source.hash();
        Self {
            pdf_path,
            pdf_hash,
            page_index,
            rectangle,
            source: Arc::new(OnceLock::from(Ok(source))),
        }
    }

    fn source(&self) -> Result<&Arc<PdfSource>, &str> {
        self.source
            .get_or_init(|| {
                let resolved = self.resolve();
                if let Err(reason) = &resolved
                    && REPORTED.lock().unwrap().insert(self.pdf_path.clone())
                {
                    warn!(
                        "Pdf pages referencing '{}' are not rendered: {reason}",
                        self.pdf_path.display()
                    );
                }
                resolved
            })
            .as_ref()
            .map_err(String::as_str)
    }

    fn resolve(&self) -> Resolved {
        let source = PdfSource::open(&self.pdf_path).map_err(|e| format!("{e:#}"))?;
        if source.hash() != self.pdf_hash {
            return Err("the file differs from the one that was imported".to_string());
        }
        if self.page_index >= source.page_count() {
            return Err(format!("the file has no page {}", self.page_index + 1));
        }
        Ok(source)
    }

    /// Maps the page, in points with y pointing down, onto the rectangle in the document.
    fn page_to_doc(&self, page_size: Vector2) -> PdfAffine {
        let half = self.rectangle.cuboid.half_extents;
        let a = self.rectangle.affine;
        let rectangle_to_doc = PdfAffine::new([
            a.matrix2.x_axis.x,
            a.matrix2.x_axis.y,
            a.matrix2.y_axis.x,
            a.matrix2.y_axis.y,
            a.translation.x,
            a.translation.y,
        ]);
        rectangle_to_doc
            * PdfAffine::translate((-half[0], -half[1]))
            * PdfAffine::scale_non_uniform(
                2.0 * half[0] / page_size[0],
                2.0 * half[1] / page_size[1],
            )
    }

    fn pixels_per_point(&self, page_size: Vector2, image_scale: f64) -> f64 {
        let half = self.rectangle.cuboid.half_extents;
        image_scale * (2.0 * half[0] / page_size[0]).max(2.0 * half[1] / page_size[1])
    }

    fn placeholder(&self) -> Image {
        PageRaster::placeholder().into_image(self.rectangle)
    }
}

impl Content for PdfPage {
    fn gen_svg(&self) -> Result<Svg, anyhow::Error> {
        let source = self.source().map_err(|e| anyhow::anyhow!("{e}"))?;
        let page_size = source.page_size(self.page_index).unwrap_or_default();
        let half = self.rectangle.cuboid.half_extents;
        let svg_root = svg::node::element::SVG::new()
            .set("x", -half[0])
            .set("y", -half[1])
            .set("width", 2.0 * half[0])
            .set("height", 2.0 * half[1])
            .set(
                "viewBox",
                format!("0 0 {:.3} {:.3}", page_size[0], page_size[1]),
            )
            .set("preserveAspectRatio", "none")
            .add(svg::node::Blob::new(source.page_svg(self.page_index)?));
        let group = svg::node::element::Group::new()
            .set(
                "transform",
                self.rectangle.affine.to_svg_transform_attr_str(),
            )
            .add(svg_root);
        Ok(Svg {
            bounds: self.rectangle.bounds(),
            svg_data: rnote_compose::utils::svg_node_to_string(&group)?,
        })
    }

    fn gen_images(
        &self,
        viewport: Aabb,
        image_scale: f64,
    ) -> Result<GeneratedContentImages, anyhow::Error> {
        let Some(region) = viewport.intersection(&self.bounds()) else {
            return Ok(GeneratedContentImages::Partial {
                images: vec![],
                viewport,
            });
        };
        let Ok(source) = self.source() else {
            return Ok(GeneratedContentImages::Full(vec![self.placeholder()]));
        };
        let page_size = source.page_size(self.page_index).unwrap_or_default();
        let pixels_per_point = self.pixels_per_point(page_size, image_scale);
        match source.whole_page_level(self.page_index, pixels_per_point) {
            Some(level) => {
                let raster = source.page_raster(self.page_index, level)?;
                Ok(GeneratedContentImages::Full(vec![
                    raster.into_image(self.rectangle),
                ]))
            }
            None => {
                let raster = source.render_region(
                    self.page_index,
                    self.page_to_doc(page_size),
                    region,
                    image_scale,
                )?;
                Ok(GeneratedContentImages::Partial {
                    images: vec![raster.into_image(Rectangle::from_p2d_aabb(region))],
                    viewport,
                })
            }
        }
    }

    fn update_geometry(&mut self) {}
}

impl Drawable for PdfPage {
    fn draw(&self, cx: &mut impl piet::RenderContext, image_scale: f64) -> anyhow::Result<()> {
        let raster = match self.source() {
            Ok(source) => {
                let page_size = source.page_size(self.page_index).unwrap_or_default();
                let level = source
                    .whole_page_level(
                        self.page_index,
                        self.pixels_per_point(page_size, image_scale),
                    )
                    .unwrap_or_else(|| source.max_whole_page_level(self.page_index));
                source.page_raster(self.page_index, level)?
            }
            Err(_) => PageRaster::placeholder(),
        };
        cx.save().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        cx.transform(self.rectangle.affine.to_kurbo());
        let image = cx
            .make_image(
                raster.width as usize,
                raster.height as usize,
                &raster.data,
                piet::ImageFormat::RgbaPremul,
            )
            .map_err(|e| {
                anyhow::anyhow!("Make piet image in PdfPage draw impl failed, Err: {e:?}")
            })?;
        cx.draw_image(
            &image,
            self.rectangle.cuboid.local_aabb().to_kurbo_rect(),
            piet::InterpolationMode::Bilinear,
        );
        cx.restore().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

impl Shapeable for PdfPage {
    fn bounds(&self) -> Aabb {
        self.rectangle.bounds()
    }

    fn hitboxes(&self) -> Vec<Aabb> {
        vec![self.bounds()]
    }

    fn outline_path(&self) -> kurbo::BezPath {
        self.bounds().to_kurbo_rect().to_path(0.25)
    }
}

impl Transformable for PdfPage {
    fn translate(&mut self, offset: Vector2) {
        self.rectangle.translate(offset);
    }

    fn rotate(&mut self, angle: f64, center: Vector2) {
        self.rectangle.rotate(angle, center);
    }

    fn scale(&mut self, scale: Vector2) {
        self.rectangle.scale(scale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strokes::Stroke;
    use crate::strokes::content::GeneratedContentImages;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pdf")
            .join(name)
    }

    /// Page 0 of `name` placed with its top-left at the origin, one doc unit per point.
    fn page(name: &str) -> PdfPage {
        let source = PdfSource::open(&fixture(name)).unwrap();
        let size = source.page_size(0).unwrap();
        PdfPage::new(source, 0, Rectangle::from_corners(Vector2::ZERO, size))
    }

    fn images(generated: GeneratedContentImages) -> Vec<Image> {
        match generated {
            GeneratedContentImages::Full(images) => images,
            GeneratedContentImages::Partial { images, .. } => images,
        }
    }

    #[test]
    fn serde_round_trip_keeps_reference() {
        let page = page("vector.pdf");
        let json = serde_json::to_string(&page).unwrap();
        let loaded: PdfPage = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.pdf_path, page.pdf_path);
        assert_eq!(loaded.pdf_hash, page.pdf_hash);
        assert_eq!(loaded.page_index, 0);
        // Rnote serializes rectangles with three decimal places.
        let (loaded_bounds, bounds) = (loaded.rectangle.bounds(), page.rectangle.bounds());
        assert!((loaded_bounds.mins - bounds.mins).length() < 1e-3);
        assert!((loaded_bounds.maxs - bounds.maxs).length() < 1e-3);
        assert!(
            json.len() < 1000,
            "must not embed the Pdf: {} bytes",
            json.len()
        );
    }

    #[test]
    fn zoomed_out_returns_cached_whole_page() {
        let page = page("vector.pdf");
        let generated = page.gen_images(page.bounds(), 0.25).unwrap();
        assert!(matches!(generated, GeneratedContentImages::Full(_)));
        let image = &images(generated)[0];
        // 0.25 px/pt is exactly level -2.
        assert_eq!(image.pixel_width, (595.28f64 * 0.25) as u32);
        assert_eq!(image.rectangle.bounds(), page.bounds());
    }

    #[test]
    fn zoomed_in_renders_only_the_viewport() {
        let page = page("vector.pdf");
        let viewport = Aabb::new(Vector2::new(100.0, 100.0), Vector2::new(200.0, 150.0));
        let generated = page.gen_images(viewport, 10.0).unwrap();
        assert!(matches!(generated, GeneratedContentImages::Partial { .. }));
        let image = &images(generated)[0];
        assert_eq!((image.pixel_width, image.pixel_height), (1000, 500));
        assert_eq!(image.rectangle.bounds(), viewport);
    }

    #[test]
    fn rotated_page_region_covers_the_viewport() {
        let mut page = page("vector.pdf");
        let center = page.bounds().center();
        page.rotate(std::f64::consts::FRAC_PI_2, center);
        let bounds = page.bounds();
        let viewport = Aabb::new(bounds.mins, bounds.mins + Vector2::new(50.0, 50.0));
        let image = &images(page.gen_images(viewport, 20.0).unwrap())[0];
        assert_eq!((image.pixel_width, image.pixel_height), (1000, 1000));
    }

    #[test]
    fn rotated_page_is_drawn_only_where_it_lies() {
        let mut page = page("vector.pdf");
        let center = page.bounds().center();
        page.rotate(std::f64::consts::FRAC_PI_4, center);
        let bounds = page.bounds();
        let square = |min: Vector2| Aabb::new(min, min + Vector2::new(100.0, 100.0));
        let alphas = |viewport: Aabb| -> Vec<u8> {
            let image = images(page.gen_images(viewport, 5.0).unwrap()).remove(0);
            image.data.chunks(4).map(|px| px[3]).collect()
        };
        // The corner of the bounds lies outside the page rotated by 45 degrees.
        assert!(alphas(square(bounds.mins)).iter().all(|&a| a == 0));
        assert!(
            alphas(square(center - Vector2::new(50.0, 50.0)))
                .iter()
                .all(|&a| a == 255)
        );
    }

    #[test]
    fn missing_pdf_renders_placeholder() {
        let mut page = page("vector.pdf");
        page.pdf_path = PathBuf::from("/nonexistent/book.pdf");
        let loaded: PdfPage = serde_json::from_str(&serde_json::to_string(&page).unwrap()).unwrap();
        let image = &images(loaded.gen_images(loaded.bounds(), 1.0).unwrap())[0];
        assert_eq!((image.pixel_width, image.pixel_height), (1, 1));
    }

    #[test]
    fn replaced_pdf_renders_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.pdf");
        std::fs::copy(fixture("vector.pdf"), &path).unwrap();
        let source = PdfSource::open(&path).unwrap();
        let size = source.page_size(0).unwrap();
        let page = PdfPage::new(source, 0, Rectangle::from_corners(Vector2::ZERO, size));
        let json = serde_json::to_string(&page).unwrap();
        drop(page);
        std::fs::copy(fixture("scan.pdf"), &path).unwrap();
        let loaded: PdfPage = serde_json::from_str(&json).unwrap();
        let image = &images(loaded.gen_images(loaded.bounds(), 1.0).unwrap())[0];
        assert_eq!((image.pixel_width, image.pixel_height), (1, 1));
    }

    #[test]
    fn stroke_serializes_under_its_own_tag() {
        let stroke = Stroke::PdfPage(page("scan.pdf"));
        let json = serde_json::to_string(&stroke).unwrap();
        assert!(json.contains("\"pdfpage\""));
        assert!(matches!(
            serde_json::from_str::<Stroke>(&json).unwrap(),
            Stroke::PdfPage(_)
        ));
    }

    #[test]
    fn svg_export_wraps_the_page() {
        let svg = page("vector.pdf").gen_svg().unwrap();
        assert!(svg.svg_data.contains("<svg"));
        assert!(svg.svg_data.contains("viewBox"));
    }
}
