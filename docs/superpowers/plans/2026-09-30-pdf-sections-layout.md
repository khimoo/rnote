# PDF のセクションに沿ってページを並べる機能 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** PDF をページ参照として取り込むとき、しおりか手で書いたテキストのセクションごとに行を改め、入れ子の深さをページ 1 枚分の字下げで表して並べる。

**Architecture:** しおりは `pdfsource/outline.rs` が hayro-syntax の辞書を直接たどって（見出し、ページ番号、深さ）の一覧にする。ダイアログはその一覧をテキストにして欄に入れ、利用者が直したテキストを `engine/import_pdfpages/sections.rs` の `parse_sections` が `Section`（PDF の開始ページと深さ）の一覧にする。配置は `grid_offsets` を置き換える `section_offsets` が行い、セクションが空なら今の格子と同じ位置になる。

**Tech Stack:** Rust 2024（rustc 1.92 以上）、hayro（main `ae09437`）の `hayro_syntax`、GTK4 0.10、libadwaita 0.8（`v1_7`）、Typst（テスト用の PDF）、bash

**Spec:** [docs/superpowers/specs/2026-09-30-pdf-sections-layout-design.md](../specs/2026-09-30-pdf-sections-layout-design.md)

## Global Constraints

- flxzt/rnote には PR、issue、コメントを出さない。push 先は `origin`（khimoo/rnote）の `main` だけ
- 本家のファイルへの変更は、`crates/rnote-engine/src/meson.build` への 2 行の追記だけにする。`lib.rs` と `engine/mod.rs` は変えない
- hayro と hayro-svg は `ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09` のまま変えない
- UI の文字列は英語で、翻訳は付けない
- ダイアログの欄: `Book Page 1 Is PDF Page` は 1 からページ数まで、既定値 1。`Max Depth` は 1 から 16 まで、既定値 2。`Columns` は 1 からページ数まで、既定値は 8 とページ数の小さいほう
- しおりの項目数と name tree の節の数は、それぞれ 10000 で打ち切る
- ビルドとテストはデスクトップ（`ssh desktop-ts`）の `/tmp/rnote-work` に作業ツリーを複製して行う。手元のノートでは cargo も nix build も実行しない（bash のスクリプトは手元で実行してよい）
- コミットメッセージは英語の Conventional Commits で、末尾に `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` を付ける

## 作業ツリーの同期とコマンド

編集は手元の `~/sagyo/rnote` で行い、テストの前に毎回デスクトップへ同期する。

```bash
rsync -a --delete --exclude /target --exclude /_mesonbuild ~/sagyo/rnote/ desktop-ts:/tmp/rnote-work/
ssh desktop-ts 'cd /tmp/rnote-work && nix develop -c <command>'
```

`--delete` を付けているので、デスクトップで作ったファイル（Typst で作る PDF など）は、次に同期する前に手元へ `scp` で取り戻す。
flake は git の管理下のファイルしか見ない。
以下、`remote <command>` と書いたら上の 2 行を実行することを指す。

## Review Focus

- Windows の改行（CRLF）を含むテキストを貼り付けた場合: 改行の違いを無視して解析する。Task 2 のテストで確かめる
- 日本語入力のまま全角の空白で字下げしたり全角の数字を書いたりした場合: 黙って別の意味に読まず、その行の番号付きのエラーになる。番号と見出しの間の全角の空白は区切りとして受け付ける。Task 2 のテストで確かめる
- 桁あふれするほど大きなページ番号: パニックせず、範囲外のエラーになる。Task 2 のテストで確かめる
- ページ範囲がセクションの途中から始まる場合: 範囲の最初のページは、そのページを含むセクションの字下げで並ぶ。Task 3 のテストで確かめる
- しおりのない PDF: テキスト欄が空になり、今の格子と同じ配置で取り込める。Task 1 と Task 3 のテストで確かめる

---

### Task 1: しおりの読み取り

**Files:**
- Create: `crates/rnote-engine/tests/fixtures/pdf/outline.typ`
- Create: `crates/rnote-engine/tests/fixtures/pdf/make-outline-edge.sh`
- Create: `crates/rnote-engine/tests/fixtures/pdf/outline.pdf`（生成物）
- Create: `crates/rnote-engine/tests/fixtures/pdf/outline-edge.pdf`（生成物）
- Create: `crates/rnote-engine/src/pdfsource/outline.rs`
- Modify: `crates/rnote-engine/tests/fixtures/pdf/make.sh`（末尾に 2 行）
- Modify: `crates/rnote-engine/src/pdfsource.rs`（`mod outline;`、`pub use`、`outline` メソッド）
- Modify: `crates/rnote-engine/src/meson.build`（`'pdfsource.rs',` の直後に `'pdfsource/outline.rs',`）

**Interfaces:**
- Consumes: `PdfSource`（`pdf: Pdf` と `path: PathBuf` のフィールド）
- Produces: `rnote_engine::pdfsource::OutlineEntry { pub title: String, pub page_index: usize, pub depth: usize }`（`Debug, Clone, PartialEq, Eq`。`page_index` は 0 から数える）、`PdfSource::outline(&self) -> Vec<OutlineEntry>`（しおりがないか読めなければ空）

- [ ] **Step 1: デスクトップの作業ツリーを作り、変更前のテストが通ることを確かめる**

`/tmp/rnote-work` はデスクトップの再起動で消えている。

Run: `remote 'meson setup _mesonbuild -Dprofile=devel -Dcli=true 2>&1 | tail -3 && cargo test -p rnote-engine 2>&1 | grep "test result"'`
Expected: すべての `test result:` が `ok`。

- [ ] **Step 2: テスト用の PDF の元を書く**

`crates/rnote-engine/tests/fixtures/pdf/outline.typ`:

```typst
#set page(paper: "a6", margin: 1cm)
= Chapter One
#pagebreak()
== Section One A
#pagebreak()
Still in Section One A.
#pagebreak()
= Über Zwei
== Section Two A
#pagebreak()
=== Detail Two A1
#pagebreak()
== Section Two B
```

6 ページで、4 ページ目に「Über Zwei」と「Section Two A」が同じページから始まる。

`crates/rnote-engine/tests/fixtures/pdf/make-outline-edge.sh`:

```bash
#!/usr/bin/env bash
# Writes outline-edge.pdf, whose outline uses forms Typst does not emit: a named and a string
# destination, a GoTo action, a Utf-16 title, an item pointing to a Url and an item that loops.
set -euo pipefail
cd "$(dirname "$0")"

objects=(
  '<< /Type /Catalog /Pages 2 0 R /Outlines 6 0 R /Dests 7 0 R /Names 8 0 R >>'
  '<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>'
  '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>'
  '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>'
  '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>'
  '<< /Type /Outlines /First 11 0 R /Last 17 0 R /Count 6 >>'
  '<< /N1 [4 0 R /Fit] >>'
  '<< /Dests 9 0 R >>'
  '<< /Kids [10 0 R] >>'
  '<< /Limits [(S1) (S1)] /Names [(S1) << /D [5 0 R /Fit] >>] >>'
  '<< /Title (Named) /Parent 6 0 R /Next 12 0 R /Dest /N1 >>'
  '<< /Title (Tree) /Parent 6 0 R /Prev 11 0 R /Next 13 0 R /Dest (S1) >>'
  '<< /Title (GoTo) /Parent 6 0 R /Prev 12 0 R /Next 15 0 R /First 14 0 R /Last 14 0 R /Count 1 /A << /S /GoTo /D [3 0 R /Fit] >> >>'
  '<< /Title <FEFF7B2C00317AE0> /Parent 13 0 R /Dest [3 0 R /XYZ null null null] >>'
  '<< /Title (Link) /Parent 6 0 R /Prev 13 0 R /Next 17 0 R /First 16 0 R /Last 16 0 R /Count 1 /A << /S /URI /URI (https://example.com) >> >>'
  '<< /Title (Promoted) /Parent 15 0 R /Dest [4 0 R /Fit] >>'
  '<< /Title (Loop) /Parent 6 0 R /Prev 15 0 R /Next 17 0 R /Dest [5 0 R /Fit] >>'
)

# Every byte is Ascii, so string lengths are byte offsets.
pdf=$'%PDF-1.7\n'
offsets=()
for i in "${!objects[@]}"; do
  offsets+=("${#pdf}")
  pdf+="$((i + 1)) 0 obj"$'\n'"${objects[$i]}"$'\nendobj\n'
done
xref_start=${#pdf}
size=$((${#objects[@]} + 1))
pdf+="xref"$'\n'"0 $size"$'\n'"0000000000 65535 f "$'\n'
for offset in "${offsets[@]}"; do
  printf -v entry '%010d 00000 n \n' "$offset"
  pdf+=$entry
done
pdf+="trailer"$'\n'"<< /Size $size /Root 1 0 R >>"$'\n'"startxref"$'\n'"$xref_start"$'\n'"%%EOF"$'\n'
printf '%s' "$pdf" > outline-edge.pdf
```

`<FEFF7B2C00317AE0>` は BOM 付き UTF-16BE の「第1章」だ。
Link の項目は URL へ移動するので解決できず、その子の Promoted は深さ 0 に繰り上がる。
Loop の項目は `/Next` で自分自身を指す。

`make.sh` の末尾に足す。

```bash
typst compile outline.typ outline.pdf
./make-outline-edge.sh
```

- [ ] **Step 3: テスト用の PDF を作る**

```bash
chmod +x crates/rnote-engine/tests/fixtures/pdf/make-outline-edge.sh
crates/rnote-engine/tests/fixtures/pdf/make-outline-edge.sh
```

Run: `remote 'cd crates/rnote-engine/tests/fixtures/pdf && typst compile outline.typ outline.pdf'`

```bash
scp desktop-ts:/tmp/rnote-work/crates/rnote-engine/tests/fixtures/pdf/outline.pdf crates/rnote-engine/tests/fixtures/pdf/
head -c 9 crates/rnote-engine/tests/fixtures/pdf/outline-edge.pdf
```

Expected: 2 つの PDF が手元にあり、`outline-edge.pdf` の先頭が `%PDF-1.7`。

- [ ] **Step 4: 失敗するテストを書く**

`crates/rnote-engine/src/pdfsource/outline.rs` を、次のテストだけを持つファイルとして作る。

```rust
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
```

`pdfsource.rs` の `use` の並びの後に足す。

```rust
mod outline;

pub use outline::OutlineEntry;
```

- [ ] **Step 5: テストが失敗することを確かめる**

Run: `remote 'cargo test -p rnote-engine pdfsource::outline 2>&1 | tail -20'`
Expected: `OutlineEntry`、`decode_text`、`outline` が未定義でコンパイルエラー。

- [ ] **Step 6: 実装する**

`outline.rs` のテストの上に書く。

```rust
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

fn collect_name_tree<'a>(xref: &'a XRef, root: Dict<'a>, leaves: &mut HashMap<Vec<u8>, Object<'a>>) {
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
        for kid in node.get::<Array<'a>>(b"Kids").iter().flat_map(Array::raw_iter) {
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
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else if let Some(utf8) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(utf8).into_owned()
    } else {
        bytes.iter().map(|&byte| char::from(byte)).collect()
    }
}
```

`pdfsource.rs` の `impl PdfSource` の `page_count` の後に足す。

```rust
    /// The outline (bookmarks) in document order. Empty when the Pdf has none or it can not be read.
    pub fn outline(&self) -> Vec<OutlineEntry> {
        outline::read(&self.pdf).unwrap_or_else(|e| {
            tracing::warn!(
                "Reading the outline of '{}' failed, Err: {e:?}",
                self.path.display()
            );
            Vec::new()
        })
    }
```

`crates/rnote-engine/src/meson.build` の `'pdfsource.rs',` の次の行に `'pdfsource/outline.rs',` を足す。

hayro-syntax の型の制約（`ObjectLike` が `Object` に実装されていない、など）でコンパイルが通らない場合は、hayro-syntax の中で同じ値を取り出している箇所（`hayro-syntax/src/page.rs` の `resolve_pages` など）の書き方に合わせる。
`Page::raw().obj_id()` が `None` になりページの表が空になる場合は、catalog の `/Pages` から `/Kids` を `get_ref` でたどって表を作る関数に差し替え、設計文書の「要検証」の節にその結果を書く。

- [ ] **Step 7: テストを通す**

Run: `remote 'cargo test -p rnote-engine pdfsource 2>&1 | tail -20'`
Expected: 新しい 4 件と既存の `pdfsource` のテストがすべて PASS。
`nested_headings_become_entries_in_document_order` だけが落ちる場合は、Typst が書いたしおりを `remote 'cargo test -p rnote-engine nested_headings -- --nocapture'` の差分で確かめる。見出しの前後に空白が入っているなど、読み取りでなく期待値の側の問題なら期待値を直す。

- [ ] **Step 8: clippy**

Run: `remote 'cargo clippy -p rnote-engine --all-targets 2>&1 | tail -10'`
Expected: 新しい警告なし。

- [ ] **Step 9: Commit**

```bash
git add crates/rnote-engine/tests/fixtures/pdf crates/rnote-engine/src/pdfsource.rs crates/rnote-engine/src/pdfsource crates/rnote-engine/src/meson.build
git commit -m "feat(engine): read pdf outline for page references" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: セクションのテキスト

**Files:**
- Create: `crates/rnote-engine/src/engine/import_pdfpages/sections.rs`
- Modify: `crates/rnote-engine/src/engine/import_pdfpages.rs`（`use` の並びの後に `pub mod sections;`）
- Modify: `crates/rnote-engine/src/meson.build`（`'engine/import_pdfpages.rs',` の直後に `'engine/import_pdfpages/sections.rs',`）

**Interfaces:**
- Consumes: `rnote_engine::pdfsource::OutlineEntry`（Task 1）
- Produces:
  - `rnote_engine::engine::import_pdfpages::sections::Section { pub start: usize, pub depth: usize }`（`Debug, Clone, Copy, PartialEq, Eq`。`start` は 0 から数えた PDF のページ番号）
  - `parse_sections(text: &str, book_page_one: usize, max_depth: usize, page_count: usize) -> Result<Vec<Section>, SectionsError>`（`book_page_one` は 1 から数えた PDF のページ番号。返す一覧は `start` の順に並ぶ）
  - `SectionsError { pub line: usize, pub kind: SectionsErrorKind }`（`line` は 1 から数える。`Display` は `Line {line}: {理由}`。`std::error::Error` を実装）
  - `SectionsErrorKind::{NoPageNumber, UnevenIndent, TooDeep, OutOfRange { pdf_page: usize, page_count: usize }, BeforePrevious}`
  - `outline_text(entries: &[OutlineEntry]) -> String`

- [ ] **Step 1: 失敗するテストを書く**

`crates/rnote-engine/src/engine/import_pdfpages/sections.rs` を、次のテストだけを持つファイルとして作り、`import_pdfpages.rs` に `pub mod sections;` を足す。

```rust
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
            (2, OutOfRange { pdf_page: 101, page_count: 100 })
        );
        assert_eq!(
            error("0"),
            (1, OutOfRange { pdf_page: 0, page_count: 100 })
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
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `remote 'cargo test -p rnote-engine sections 2>&1 | tail -20'`
Expected: `Section`、`parse_sections` などが未定義でコンパイルエラー。

- [ ] **Step 3: 実装する**

`sections.rs` のテストの上に書く。

```rust
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
            } => write!(f, "PDF page {pdf_page} is outside of the {page_count} pages"),
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
        if indent % 2 != 0 {
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
pub fn outline_text(entries: &[OutlineEntry]) -> String {
    let mut text = String::new();
    for entry in entries {
        text.push_str(&"  ".repeat(entry.depth));
        text.push_str(&(entry.page_index + 1).to_string());
        let title = entry.title.split_whitespace().collect::<Vec<_>>().join(" ");
        if !title.is_empty() {
            text.push(' ');
            text.push_str(&title);
        }
        text.push('\n');
    }
    text
}
```

`crates/rnote-engine/src/meson.build` の `'engine/import_pdfpages.rs',` の次の行に `'engine/import_pdfpages/sections.rs',` を足す。

- [ ] **Step 4: テストを通す**

Run: `remote 'cargo test -p rnote-engine sections 2>&1 | tail -20 && cargo clippy -p rnote-engine --all-targets 2>&1 | tail -10'`
Expected: 11 件すべて PASS。clippy に新しい警告なし。

- [ ] **Step 5: Commit**

```bash
git add crates/rnote-engine/src/engine/import_pdfpages.rs crates/rnote-engine/src/engine/import_pdfpages crates/rnote-engine/src/meson.build
git commit -m "feat(engine): parse section text for pdf page layout" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: セクションに沿った配置

**Files:**
- Modify: `crates/rnote-engine/src/engine/import_pdfpages.rs`（`grid_offsets` を `section_offsets` に置き換え、`generate_pdfpage_strokes` に引数を足し、テストを直す）
- Modify: `crates/rnote-engine/examples/pdfpage_bench.rs:11,71`（`grid_offsets` の呼び出し）
- Modify: `crates/rnote-ui/src/dialogs/import_pdfpages.rs:48-54`（引数 `&[]` を足すだけ。ダイアログは Task 4 で作り直す）

**Interfaces:**
- Consumes: `Section`（Task 2）
- Produces:
  - `rnote_engine::engine::import_pdfpages::section_offsets(sizes: &[Vector2], first_page: usize, sections: &[Section], columns: usize, gap: f64) -> Vec<Vector2>`（`sizes[i]` は PDF のページ `first_page + i` の大きさ。`sections` は `start` の順に並んでいること）
  - `Engine::generate_pdfpage_strokes(&self, source: &Arc<PdfSource>, pages: Range<usize>, sections: &[Section], columns: usize, gap_ratio: f64, insert_pos: Vector2) -> anyhow::Result<Vec<(Stroke, Option<StrokeLayer>)>>`

- [ ] **Step 1: テストを書き換え、足す**

`import_pdfpages.rs` のテストで、`grid_offsets(&sizes, 2, 1.0)` を `section_offsets(&sizes, 0, &[], 2, 1.0)` に、`grid_offsets(&[v(1.0, 1.0), v(1.0, 1.0)], 0, 0.0)` を `section_offsets(&[v(1.0, 1.0), v(1.0, 1.0)], 0, &[], 0, 0.0)` に変える。
`generate_pdfpage_strokes` の 3 箇所の呼び出しでは、ページ範囲の次に `&[]` を足す。
`use super::*;` の次に `use super::sections::Section;` を足し、次のテストを足す。

```rust
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
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `remote 'cargo test -p rnote-engine import_pdfpages 2>&1 | tail -20'`
Expected: `section_offsets` が未定義で、`generate_pdfpage_strokes` の引数の数が合わずにコンパイルエラー。

- [ ] **Step 3: 実装する**

`import_pdfpages.rs` の先頭の doc コメントを `//! Lays out referenced Pdf pages in rows per section, see [crate::strokes::PdfPage].` に変え、`use` に `use sections::Section;` を足す。
`grid_offsets` を次の関数に置き換える。

```rust
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
```

`generate_pdfpage_strokes` は、doc コメントの `laid out by [grid_offsets]` を `laid out by [section_offsets]` に変え、`pages: Range<usize>,` の次に引数 `sections: &[Section],` を足し、配置の行を次に変える。

```rust
        let offsets = section_offsets(
            &sizes,
            pages.start,
            sections,
            columns,
            column_width * gap_ratio,
        );
```

`examples/pdfpage_bench.rs` は、`use` を `use rnote_engine::engine::import_pdfpages::section_offsets;` に、71 行目を `let offsets = section_offsets(&sizes, 0, &[], 8, sizes[0][0] * 0.1);` に変える。

`crates/rnote-ui/src/dialogs/import_pdfpages.rs` の `generate_pdfpage_strokes` の呼び出しでは、`options.pages,` の次に `&[],` を足す。

- [ ] **Step 4: テストを通す**

Run: `remote 'cargo test -p rnote-engine import_pdfpages 2>&1 | tail -20'`
Expected: 既存の 4 件と新しい 7 件がすべて PASS。既存の格子のテストが同じ期待値のまま通ることで、セクションが空なら今の格子と同じ位置になることを確かめる。

- [ ] **Step 5: 全体のテスト、clippy、UI の型検査**

Run: `remote 'cargo test -p rnote-engine 2>&1 | grep "test result" && cargo clippy -p rnote-engine --all-targets 2>&1 | tail -5 && meson compile ui-cargo-check -C _mesonbuild 2>&1 | tail -5'`
Expected: すべての `test result:` が `ok`、clippy に新しい警告なし、UI の型検査もエラーなし。

- [ ] **Step 6: Commit**

```bash
git add crates/rnote-engine/src/engine/import_pdfpages.rs crates/rnote-engine/examples/pdfpage_bench.rs crates/rnote-ui/src/dialogs/import_pdfpages.rs
git commit -m "feat(engine): indent pdf page rows by section depth" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: ダイアログ

**Files:**
- Modify: `crates/rnote-ui/src/dialogs/import_pdfpages.rs`（全体を書き直す）
- Modify: `docs/pdf-page-references.md`（「手動確認」の節）
- Modify: `docs/superpowers/specs/2026-09-30-pdf-sections-layout-design.md`（状態の行）

**Interfaces:**
- Consumes: `PdfSource::outline`（Task 1）、`outline_text`、`parse_sections`、`Section`（Task 2）、`Engine::generate_pdfpage_strokes`（Task 3）
- Produces: なし（アクション `win.import-pdf-pages` は既存のまま）

UI は単体テストを持たないので、型検査と手動の起動で確かめる。

- [ ] **Step 1: ダイアログを書き直す**

`crates/rnote-ui/src/dialogs/import_pdfpages.rs` の全体:

```rust
//! Imports a Pdf as page references, see [rnote_engine::strokes::PdfPage].

use crate::appwindow::RnAppWindow;
use crate::canvas::RnCanvas;
use adw::prelude::*;
use gtk4::{FileDialog, FileFilter, gio, glib::clone};
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
                Ok(sections) => {
                    sections_status.set_text(&format!("{} section(s)", sections.len()))
                }
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
```

`#[weak]` の参照にしているのは、`buffer` や `SpinRow` のシグナルにつないだ閉包がダイアログを強参照すると、ダイアログを閉じても解放されなくなるからだ。

- [ ] **Step 2: 型検査と clippy**

Run: `remote 'meson compile ui-cargo-check -C _mesonbuild 2>&1 | tail -20 && meson compile ui-cargo-clippy -C _mesonbuild 2>&1 | tail -10'`
Expected: エラーも新しい警告もない。`clone!` の書き方でエラーが出たら、`crates/rnote-ui/src/dialogs/mod.rs` の `clone!` の使い方に合わせる。`set_prefer_wide_layout` が見つからない場合はその行を消す。

- [ ] **Step 3: 使い方の文書と設計文書を更新する**

`docs/pdf-page-references.md` の「手動確認」の節で、`ページ範囲、列数、間隔を指定して取り込む。` を次に置き換える。

```markdown
ページ範囲、列数、間隔を指定して取り込む。
しおりのある PDF では、セクションの欄にしおりが入る。
しおりのない PDF では、目次を見ながら 1 行に 1 セクションずつ、開始ページの印刷番号と見出しを書き、空白 2 つで 1 段字下げする。
本の 1 ページ目が PDF の何ページ目かを `Book Page 1 Is PDF Page` に、使う階層の数を `Max Depth` に指定する。
```

`crates/rnote-engine/tests/fixtures/pdf/mixed.pdf` の行の後に `- crates/rnote-engine/tests/fixtures/pdf/outline.pdf` を足す。
確認項目の 1 つ目の後に次の 3 項目を足す。

```markdown
- [ ] `outline.pdf` を取り込むと、セクションの欄にしおりが入り、章ごとに行が改まり、節が 1 ページ分字下げされる。
- [ ] セクションの欄の字下げを崩すと、行番号付きのエラーが出て Import を押せなくなり、直すと押せるようになる。
- [ ] セクションの欄を空にすると、今までどおり格子状に並ぶ。
```

設計文書 `docs/superpowers/specs/2026-09-30-pdf-sections-layout-design.md` の `- 状態: 設計のみ` を `- 状態: 実装済み。UI での手動確認はまだ` に変える。
Task 1 で `Page::raw().obj_id()` が使えたなら、「要検証」の節を消す。

- [ ] **Step 4: 起動して取り込めることを確かめる**

Run: `remote 'meson compile ui-cargo-build -C _mesonbuild 2>&1 | tail -3'`
Expected: ビルドが成功する。

利用者に、`docs/pdf-page-references.md` の「手動確認」の新しい 3 項目を確かめてもらう。
アプリの起動方法は同じ文書の「ビルド済みのアプリを試す」の節にある。

- [ ] **Step 5: Commit**

```bash
git add crates/rnote-ui/src/dialogs/import_pdfpages.rs docs/pdf-page-references.md docs/superpowers/specs/2026-09-30-pdf-sections-layout-design.md
git commit -m "feat(ui): lay out imported pdf pages by sections" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
