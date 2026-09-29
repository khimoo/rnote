//! Imports a Pdf as page references, see [rnote_engine::strokes::PdfPage].

use crate::appwindow::RnAppWindow;
use crate::canvas::RnCanvas;
use adw::prelude::*;
use gtk4::{FileDialog, FileFilter, gio, glib, glib::clone};
use p2d::math::Vector2;
use rnote_compose::ext::Vector2Ext;
use rnote_engine::engine::import_pdfpages::sections::{
    Section, SectionsError, outline_text, parse_sections,
};
use rnote_engine::pdfsource::PdfSource;
use rnote_engine::strokes::Stroke;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use tracing::debug;

struct Options {
    pages: Range<usize>,
    sections: Vec<Section>,
    columns: usize,
    gap_ratio: f64,
}

pub(crate) async fn import_pdf_pages(appwindow: &RnAppWindow) {
    let Some(path) = choose_pdf(appwindow).await else {
        return;
    };
    let opened = gio::spawn_blocking(move || {
        PdfSource::open(&path).map(|source| {
            let outline = outline_text(&source.outline());
            (source, outline)
        })
    })
    .await;
    let (source, outline) = match opened {
        Ok(Ok(opened)) => opened,
        Ok(Err(e)) => {
            appwindow
                .overlays()
                .dispatch_toast_error(&format!("Opening the PDF failed: {e:#}"));
            return;
        }
        Err(_) => {
            appwindow
                .overlays()
                .dispatch_toast_error("Opening the PDF failed");
            return;
        }
    };
    let Some(options) = ask_options(appwindow, &source, &outline).await else {
        return;
    };
    let Some(canvas) = appwindow.active_tab_canvas() else {
        return;
    };
    let insert_pos = default_insert_pos(&canvas);
    let generated = canvas.engine_ref().generate_pdfpage_strokes(
        &source,
        options.pages,
        &options.sections,
        options.columns,
        options.gap_ratio,
        insert_pos,
    );
    match generated {
        Ok(strokes) => {
            let widget_flags = canvas.engine_mut().import_generated_content(strokes, false);
            appwindow.handle_widget_flags(widget_flags, &canvas);
        }
        Err(e) => {
            appwindow
                .overlays()
                .dispatch_toast_error(&format!("Importing the PDF failed: {e:#}"));
        }
    }
}

async fn choose_pdf(appwindow: &RnAppWindow) -> Option<PathBuf> {
    let filter = FileFilter::new();
    filter.add_mime_type("application/pdf");
    filter.add_suffix("pdf");
    filter.set_name(Some("PDF"));
    let filters = gio::ListStore::new::<FileFilter>();
    filters.append(&filter);
    let dialog = FileDialog::builder()
        .title("Import PDF as Page References")
        .modal(true)
        .accept_label("Import")
        .filters(&filters)
        .default_filter(&filter)
        .build();
    match dialog.open_future(Some(appwindow)).await {
        Ok(file) => file.path(),
        Err(e) => {
            debug!("Did not import PDF pages (Error or dialog dismissed by user), Err: {e:?}");
            None
        }
    }
}

async fn ask_options(
    appwindow: &RnAppWindow,
    source: &Arc<PdfSource>,
    outline: &str,
) -> Option<Options> {
    let page_count = source.page_count();
    let file_name = source
        .path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dialog = adw::AlertDialog::new(
        Some("Import PDF as Page References"),
        Some(&format!(
            "{file_name} ({page_count} pages)\nPages keep referring to this file. Keep it at the same path."
        )),
    );
    dialog.add_responses(&[("cancel", "Cancel"), ("import", "Import")]);
    dialog.set_response_appearance("import", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("import"));
    dialog.set_close_response("cancel");
    dialog.set_prefer_wide_layout(true);

    let first = adw::SpinRow::with_range(1.0, page_count as f64, 1.0);
    first.set_title("First Page");
    first.set_value(1.0);
    let last = adw::SpinRow::with_range(1.0, page_count as f64, 1.0);
    last.set_title("Last Page");
    last.set_value(page_count as f64);
    let columns = adw::SpinRow::with_range(1.0, page_count as f64, 1.0);
    columns.set_title("Columns");
    columns.set_value(8.0_f64.min(page_count as f64));
    let gap = adw::SpinRow::with_range(0.0, 200.0, 5.0);
    gap.set_title("Gap Between Pages (%)");
    gap.set_value(10.0);
    let book_page_one = adw::SpinRow::with_range(1.0, page_count as f64, 1.0);
    book_page_one.set_title("Book Page 1 Is PDF Page");
    book_page_one.set_value(1.0);
    let max_depth = adw::SpinRow::with_range(1.0, 16.0, 1.0);
    max_depth.set_title("Max Depth");
    max_depth.set_value(2.0);

    let rows = gtk4::ListBox::new();
    rows.add_css_class("boxed-list");
    rows.set_selection_mode(gtk4::SelectionMode::None);
    for row in [&first, &last, &columns, &gap, &book_page_one, &max_depth] {
        rows.append(row);
    }

    let sections_view = gtk4::TextView::builder()
        .monospace(true)
        .top_margin(6)
        .bottom_margin(6)
        .left_margin(6)
        .right_margin(6)
        .build();
    let buffer = sections_view.buffer();
    buffer.set_text(outline);
    let sections_scroll = gtk4::ScrolledWindow::builder()
        .min_content_height(200)
        .child(&sections_view)
        .build();
    sections_scroll.add_css_class("card");
    let sections_status = gtk4::Label::builder().xalign(0.0).wrap(true).build();

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    content.append(&rows);
    content.append(
        &gtk4::Label::builder()
            .label("Sections: one per line, the book page where it starts and an optional title. Indent two spaces per level.")
            .xalign(0.0)
            .wrap(true)
            .build(),
    );
    content.append(&sections_scroll);
    content.append(&sections_status);
    dialog.set_extra_child(Some(&content));

    let validate = Rc::new(clone!(
        #[weak]
        dialog,
        #[weak]
        buffer,
        #[weak]
        book_page_one,
        #[weak]
        max_depth,
        #[weak]
        sections_status,
        move || {
            let parsed = parse(&buffer, &book_page_one, &max_depth, page_count);
            match &parsed {
                Ok(sections) if sections.is_empty() => {
                    sections_status.set_text("No sections: pages are laid out in a grid.")
                }
                Ok(sections) => sections_status.set_text(&format!("{} section(s)", sections.len())),
                Err(e) => sections_status.set_text(&e.to_string()),
            }
            if parsed.is_ok() {
                sections_status.remove_css_class("error");
            } else {
                sections_status.add_css_class("error");
            }
            dialog.set_response_enabled("import", parsed.is_ok());
        }
    ));
    buffer.connect_changed(clone!(
        #[strong]
        validate,
        move |_| validate()
    ));
    for row in [&book_page_one, &max_depth] {
        row.connect_notify_local(
            Some("value"),
            clone!(
                #[strong]
                validate,
                move |_, _| validate()
            ),
        );
    }
    validate();

    if dialog.choose_future(Some(appwindow)).await.as_str() != "import" {
        return None;
    }
    // Import is only enabled while the sections parse.
    let sections = parse(&buffer, &book_page_one, &max_depth, page_count).ok()?;
    let first = first.value() as usize;
    let last = (last.value() as usize).max(first);
    Some(Options {
        pages: first - 1..last,
        sections,
        columns: columns.value() as usize,
        gap_ratio: gap.value() / 100.0,
    })
}

fn parse(
    buffer: &gtk4::TextBuffer,
    book_page_one: &adw::SpinRow,
    max_depth: &adw::SpinRow,
    page_count: usize,
) -> Result<Vec<Section>, SectionsError> {
    let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
    parse_sections(
        &text,
        book_page_one.value() as usize,
        max_depth.value() as usize,
        page_count,
    )
}

/// Same position as the upstream importer. `RnCanvas::determine_stroke_import_pos` is private.
fn default_insert_pos(canvas: &RnCanvas) -> Vector2 {
    let engine = canvas.engine_ref();
    engine
        .camera
        .transform()
        .inverse()
        .transform_point2(Stroke::IMPORT_OFFSET_DEFAULT)
        .maxs(&Vector2::new(engine.document.x, engine.document.y))
}
