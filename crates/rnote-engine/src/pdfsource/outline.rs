//! Reads the outline (bookmarks) of a Pdf, see [super::PdfSource::outline].

use anyhow::Context;
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::{
    Array, Dict, MaybeRef, Name, ObjRef, Object, ObjectIdentifier, String as PdfString,
};
use hayro::hayro_syntax::xref::XRef;
use std::collections::{HashMap, HashSet};

/// Stops reading outlines and name trees of broken files that never end.
const MAX_ITEMS: usize = 10_000;

/// An outline item that points to a page of the same Pdf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    pub title: String,
    /// 0-based.
    pub page_index: usize,
    pub depth: usize,
}

/// The outline items in document order. Items whose destination is not a page of this Pdf are
/// left out and their children take their depth.
pub(super) fn read(pdf: &Pdf) -> anyhow::Result<Vec<OutlineEntry>> {
    let xref = pdf.xref();
    let catalog = xref
        .get::<Dict<'_>>(xref.root_id())
        .context("reading the catalog")?;
    let Some(first) = catalog
        .get::<Dict<'_>>(b"Outlines")
        .and_then(|outlines| outlines.get_ref(b"First"))
    else {
        return Ok(Vec::new());
    };
    let destinations = Destinations::new(pdf, &catalog);
    let mut entries = Vec::new();
    let mut visited = HashSet::new();
    // The child is pushed after the next sibling so that it is visited first.
    let mut stack: Vec<(ObjRef, usize)> = vec![(first, 0)];
    while let Some((item_ref, depth)) = stack.pop() {
        if visited.len() >= MAX_ITEMS {
            break;
        }
        if !visited.insert(item_ref) {
            continue;
        }
        let Some(item) = xref.get::<Dict<'_>>(item_ref.into()) else {
            continue;
        };
        if let Some(next) = item.get_ref(b"Next") {
            stack.push((next, depth));
        }
        let child_depth = match destinations.page_of_item(&item) {
            Some(page_index) => {
                entries.push(OutlineEntry {
                    title: item
                        .get::<PdfString<'_>>(b"Title")
                        .map(|title| decode_text(title.as_bytes()))
                        .unwrap_or_default(),
                    page_index,
                    depth,
                });
                depth + 1
            }
            None => depth,
        };
        if let Some(child) = item.get_ref(b"First") {
            stack.push((child, child_depth));
        }
    }
    Ok(entries)
}

/// Resolves destinations to page indices.
struct Destinations<'a> {
    pages: HashMap<ObjectIdentifier, usize>,
    /// The Pdf 1.1 `/Dests` dictionary, keyed by name.
    by_name: Option<Dict<'a>>,
    /// The leaves of the `/Names /Dests` name tree, keyed by string.
    by_string: HashMap<Vec<u8>, Object<'a>>,
}

impl<'a> Destinations<'a> {
    fn new(pdf: &'a Pdf, catalog: &Dict<'a>) -> Self {
        let pages = pdf
            .pages()
            .iter()
            .enumerate()
            .filter_map(|(index, page)| Some((page.raw().obj_id()?, index)))
            .collect();
        let mut by_string = HashMap::new();
        if let Some(root) = catalog
            .get::<Dict<'a>>(b"Names")
            .and_then(|names| names.get::<Dict<'a>>(b"Dests"))
        {
            collect_name_tree(pdf.xref(), root, &mut by_string);
        }
        Self {
            pages,
            by_name: catalog.get::<Dict<'a>>(b"Dests"),
            by_string,
        }
    }

    fn page_of_item(&self, item: &Dict<'a>) -> Option<usize> {
        if let Some(dest) = item.get::<Object<'a>>(b"Dest") {
            return self.page_of(dest);
        }
        let action = item.get::<Dict<'a>>(b"A")?;
        if action.get::<Name<'a>>(b"S")?.as_str() != "GoTo" {
            return None;
        }
        self.page_of(action.get::<Object<'a>>(b"D")?)
    }

    fn page_of(&self, dest: Object<'a>) -> Option<usize> {
        let explicit = match dest {
            Object::Name(name) => self.by_name.as_ref()?.get::<Object<'a>>(&*name)?,
            Object::String(string) => self.by_string.get(string.as_bytes())?.clone(),
            other => other,
        };
        let array = match explicit {
            Object::Array(array) => array,
            Object::Dict(dict) => dict.get::<Array<'a>>(b"D")?,
            _ => return None,
        };
        let page_ref = array.raw_iter().next()?.as_obj_ref()?;
        self.pages.get(&ObjectIdentifier::from(page_ref)).copied()
    }
}

fn collect_name_tree<'a>(
    xref: &'a XRef,
    root: Dict<'a>,
    leaves: &mut HashMap<Vec<u8>, Object<'a>>,
) {
    let mut visited = HashSet::new();
    let mut nodes = vec![root];
    while let Some(node) = nodes.pop() {
        if let Some(names) = node.get::<Array<'a>>(b"Names") {
            let mut items = names.raw_iter();
            while let (Some(key), Some(value)) = (items.next(), items.next()) {
                let MaybeRef::NotRef(Object::String(key)) = key else {
                    continue;
                };
                let value = match value {
                    MaybeRef::Ref(value) => xref.get::<Object<'a>>(value.into()),
                    MaybeRef::NotRef(value) => Some(value),
                };
                if let Some(value) = value {
                    leaves.insert(key.as_bytes().to_vec(), value);
                }
            }
        }
        for kid in node
            .get::<Array<'a>>(b"Kids")
            .iter()
            .flat_map(Array::raw_iter)
        {
            let Some(kid) = kid.as_obj_ref() else {
                continue;
            };
            if visited.len() < MAX_ITEMS
                && visited.insert(kid)
                && let Some(kid) = xref.get::<Dict<'a>>(kid.into())
            {
                nodes.push(kid);
            }
        }
    }
}

/// Pdf text strings are Utf-16BE or Utf-8 behind a byte order mark, and PDFDocEncoding
/// otherwise, which is read as Latin-1 because the two only differ in a few symbols.
fn decode_text(bytes: &[u8]) -> String {
    if let Some(utf16) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = utf16
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_be_bytes(pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else if let Some(utf8) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(utf8).into_owned()
    } else {
        bytes.iter().map(|&byte| char::from(byte)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdfsource::PdfSource;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pdf")
            .join(name)
    }

    fn entry(title: &str, page_index: usize, depth: usize) -> OutlineEntry {
        OutlineEntry {
            title: title.to_string(),
            page_index,
            depth,
        }
    }

    #[test]
    fn nested_headings_become_entries_in_document_order() {
        let source = PdfSource::open(&fixture("outline.pdf")).unwrap();
        assert_eq!(
            source.outline(),
            vec![
                entry("Chapter One", 0, 0),
                entry("Section One A", 1, 1),
                entry("Über Zwei", 3, 0),
                entry("Section Two A", 3, 1),
                entry("Detail Two A1", 4, 2),
                entry("Section Two B", 5, 1),
            ]
        );
    }

    #[test]
    fn named_and_action_destinations_resolve_and_broken_items_are_skipped() {
        let source = PdfSource::open(&fixture("outline-edge.pdf")).unwrap();
        assert_eq!(
            source.outline(),
            vec![
                entry("Named", 1, 0),
                entry("Tree", 2, 0),
                entry("GoTo", 0, 0),
                entry("第1章", 0, 1),
                entry("Promoted", 1, 0),
                entry("Loop", 2, 0),
            ]
        );
    }

    #[test]
    fn pdf_without_outline_has_no_entries() {
        let source = PdfSource::open(&fixture("scan.pdf")).unwrap();
        assert_eq!(source.outline(), vec![]);
    }

    #[test]
    fn text_strings_are_decoded_by_their_byte_order_mark() {
        assert_eq!(decode_text(&[0xFE, 0xFF, 0x7B, 0x2C, 0x00, 0x31]), "第1");
        assert_eq!(decode_text(&[0xEF, 0xBB, 0xBF, 0xE7, 0xAB, 0xA0]), "章");
        assert_eq!(decode_text(b"\xDCber"), "Über");
    }
}
