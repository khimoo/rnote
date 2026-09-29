//! Sections that split imported Pdf pages into indented runs of rows, see
//! [super::section_offsets]. Users write them as text, one section per line.

use crate::pdfsource::OutlineEntry;
use std::fmt;

/// The pages from `start` (0-based Pdf page) up to the next section, nested `depth` levels deep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub start: usize,
    pub depth: usize,
}

/// The first line of the sections text that can not be used. `line` counts from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionsError {
    pub line: usize,
    pub kind: SectionsErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionsErrorKind {
    NoPageNumber,
    UnevenIndent,
    TooDeep,
    OutOfRange { pdf_page: usize, page_count: usize },
    BeforePrevious,
}

impl fmt::Display for SectionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Line {}: ", self.line)?;
        match &self.kind {
            SectionsErrorKind::NoPageNumber => {
                write!(f, "start with a page number, then a space and the title")
            }
            SectionsErrorKind::UnevenIndent => {
                write!(f, "indent by two spaces or a tab per level")
            }
            SectionsErrorKind::TooDeep => {
                write!(f, "indented more than one level deeper than the line above")
            }
            SectionsErrorKind::OutOfRange {
                pdf_page,
                page_count,
            } => write!(
                f,
                "PDF page {pdf_page} is outside of the {page_count} pages"
            ),
            SectionsErrorKind::BeforePrevious => write!(f, "starts before the line above"),
        }
    }
}

impl std::error::Error for SectionsError {}

/// Parses one section per line: an indent of two spaces or a tab per level, the printed page
/// where the section starts, and optionally a space and a title, which is ignored. Printed
/// page 1 is Pdf page `book_page_one`, counting from 1. Lines nested `max_depth` levels or deeper
/// are checked like the others but left out, so their pages stay in the section above.
pub fn parse_sections(
    text: &str,
    book_page_one: usize,
    max_depth: usize,
    page_count: usize,
) -> Result<Vec<Section>, SectionsError> {
    let mut sections = Vec::new();
    // Depth and 1-based Pdf page of the last line that is not blank.
    let mut previous: Option<(usize, usize)> = None;
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let error = |kind: SectionsErrorKind| SectionsError {
            line: index + 1,
            kind,
        };
        let rest = line.trim_start_matches([' ', '\t']);
        let indent: usize = line[..line.len() - rest.len()]
            .chars()
            .map(|c| if c == '\t' { 2 } else { 1 })
            .sum();
        if !indent.is_multiple_of(2) {
            return Err(error(SectionsErrorKind::UnevenIndent));
        }
        let depth = indent / 2;
        let digits_end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let separated = rest[digits_end..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace);
        if digits_end == 0 || !separated {
            return Err(error(SectionsErrorKind::NoPageNumber));
        }
        if depth > previous.map_or(0, |(depth, _)| depth + 1) {
            return Err(error(SectionsErrorKind::TooDeep));
        }
        let printed: usize = rest[..digits_end].parse().unwrap_or(usize::MAX);
        let pdf_page = printed.saturating_add(book_page_one.max(1) - 1);
        if pdf_page == 0 || pdf_page > page_count {
            return Err(error(SectionsErrorKind::OutOfRange {
                pdf_page,
                page_count,
            }));
        }
        if previous.is_some_and(|(_, page)| pdf_page < page) {
            return Err(error(SectionsErrorKind::BeforePrevious));
        }
        previous = Some((depth, pdf_page));
        if depth < max_depth {
            sections.push(Section {
                start: pdf_page - 1,
                depth,
            });
        }
    }
    Ok(sections)
}

/// The sections text for `entries`, numbered by Pdf page so that it parses with printed page 1
/// on Pdf page 1.
/// Control characters in titles become spaces, as GTK rejects text containing a NUL.
pub fn outline_text(entries: &[OutlineEntry]) -> String {
    let mut text = String::new();
    for entry in entries {
        text.push_str(&"  ".repeat(entry.depth));
        text.push_str(&(entry.page_index + 1).to_string());
        let title = entry
            .title
            .split(|c: char| c.is_whitespace() || c.is_control())
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !title.is_empty() {
            text.push(' ');
            text.push_str(&title);
        }
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::SectionsErrorKind::*;
    use super::*;

    fn s(start: usize, depth: usize) -> Section {
        Section { start, depth }
    }

    fn error(text: &str) -> (usize, SectionsErrorKind) {
        let e = parse_sections(text, 1, 16, 100).unwrap_err();
        (e.line, e.kind)
    }

    #[test]
    fn indent_sets_the_depth_and_titles_are_ignored() {
        let text = "1 Chapter 1\n  3  1.1 Sums\n\t9 1.2 Products\n    10 1.2.1\n\n21 Chapter 2\n";
        assert_eq!(
            parse_sections(text, 1, 16, 30).unwrap(),
            vec![s(0, 0), s(2, 1), s(8, 1), s(9, 2), s(20, 0)]
        );
    }

    #[test]
    fn book_page_one_shifts_printed_pages_to_pdf_pages() {
        assert_eq!(
            parse_sections("1\n  9\n", 15, 16, 30).unwrap(),
            vec![s(14, 0), s(22, 1)]
        );
    }

    #[test]
    fn lines_at_max_depth_or_deeper_are_dropped() {
        assert_eq!(
            parse_sections("1\n  3\n    4\n  9\n", 1, 2, 30).unwrap(),
            vec![s(0, 0), s(2, 1), s(8, 1)]
        );
    }

    #[test]
    fn dropped_lines_are_still_checked() {
        let e = parse_sections("1\n  3\n    2\n", 1, 2, 30).unwrap_err();
        assert_eq!((e.line, e.kind), (3, BeforePrevious));
    }

    #[test]
    fn blank_text_has_no_sections() {
        assert_eq!(parse_sections(" \n\n\t\n", 1, 2, 30).unwrap(), vec![]);
    }

    #[test]
    fn each_rule_reports_its_line() {
        assert_eq!(error("1\nChapter 2"), (2, NoPageNumber));
        assert_eq!(error("1\n1.1 3"), (2, NoPageNumber));
        assert_eq!(error("1\n 3"), (2, UnevenIndent));
        assert_eq!(error("1\n    3"), (2, TooDeep));
        assert_eq!(error("  1"), (1, TooDeep));
        assert_eq!(error("5\n3"), (2, BeforePrevious));
        assert_eq!(
            error("1\n101"),
            (
                2,
                OutOfRange {
                    pdf_page: 101,
                    page_count: 100
                }
            )
        );
        assert_eq!(
            error("0"),
            (
                1,
                OutOfRange {
                    pdf_page: 0,
                    page_count: 100
                }
            )
        );
    }

    #[test]
    fn the_message_names_the_line() {
        assert_eq!(
            parse_sections("1\n 3", 1, 2, 30).unwrap_err().to_string(),
            "Line 2: indent by two spaces or a tab per level"
        );
    }

    #[test]
    fn windows_line_endings_parse() {
        assert_eq!(
            parse_sections("1\r\n  3 Sums\r\n", 1, 2, 30).unwrap(),
            vec![s(0, 0), s(2, 1)]
        );
    }

    #[test]
    fn full_width_indent_and_digits_are_rejected_with_their_line() {
        assert_eq!(error("1\n\u{3000}3"), (2, NoPageNumber));
        assert_eq!(error("1\n１２"), (2, NoPageNumber));
        // A full-width space between the number and the title separates them.
        assert_eq!(
            parse_sections("1\u{3000}第1章", 1, 2, 30).unwrap(),
            vec![s(0, 0)]
        );
    }

    #[test]
    fn huge_page_numbers_are_out_of_range() {
        let e = parse_sections("99999999999999999999999", 1, 2, 30).unwrap_err();
        assert!(matches!(e.kind, OutOfRange { page_count: 30, .. }));
    }

    #[test]
    fn control_characters_in_titles_do_not_reach_the_text() {
        // GTK drops the whole text when it contains a NUL, which Utf-16 titles often end with.
        let entries = [OutlineEntry {
            title: "A\0B\u{1}\0".to_string(),
            page_index: 0,
            depth: 0,
        }];
        assert_eq!(outline_text(&entries), "1 A B\n");
    }

    #[test]
    fn outline_text_parses_back_to_the_outline() {
        let entries = [
            OutlineEntry {
                title: "Chapter\n One".to_string(),
                page_index: 0,
                depth: 0,
            },
            OutlineEntry {
                title: String::new(),
                page_index: 2,
                depth: 1,
            },
            OutlineEntry {
                title: "Über".to_string(),
                page_index: 3,
                depth: 0,
            },
        ];
        let text = outline_text(&entries);
        assert_eq!(text, "1 Chapter One\n  3\n4 Über\n");
        assert_eq!(
            parse_sections(&text, 1, 16, 10).unwrap(),
            vec![s(0, 0), s(2, 1), s(3, 0)]
        );
    }
}
