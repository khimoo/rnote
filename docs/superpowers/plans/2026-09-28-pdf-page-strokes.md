# PDF のページを参照で取り込む機能 実装計画

> 2026-09-29 引き継ぎ状況: Task 1〜7 は実装・自動検証済み。Task 8 は個人 fork への公開と GitHub flake の評価まで完了。Task 6 Step 4 の実機操作と液タブ確認は未実施。下のチェックボックスは当初の計画のままで、検証結果と残作業は [利用・確認手順](../../pdf-page-references.md) を参照。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** PDF の全ページを無限キャンバスに格子状に並べても、表示範囲のページだけを必要な解像度で描く要素 `PdfPage` を Rnote に足す。

**Architecture:** PDF ごとに 1 つの `PdfSource`（バイト列、hayro の解析結果、ページ画像の LRU キャッシュ）をプロセス全体で共有し、各ページは `Stroke::PdfPage` として PDF の絶対パス、ハッシュ、ページ番号、`Rectangle` だけを保存する。描画は既存の `gen_images(viewport, image_scale)` の仕組みに乗せ、縮小時はキャッシュしたページ全体の画像、拡大時は表示範囲だけを `hayro::render_into` で描く。取り込みは別のメニュー項目とダイアログから行い、既存の取り込み経路には手を入れない。

**Tech Stack:** Rust 2024（rustc 1.92 以上）、hayro（main `ae09437`）、GTK4 0.10、libadwaita 0.8、meson、Nix flake（nixos-unstable）

**Spec:** [docs/superpowers/specs/2026-09-28-pdf-page-strokes-design.md](../specs/2026-09-28-pdf-page-strokes-design.md)

## Global Constraints

- flxzt/rnote には PR、issue、コメントを出さない。push 先は `origin`（khimoo/rnote）の `main` だけ
- 本家のファイルへの変更は、分岐の追加、`mod` の宣言、ファイル一覧への追記、hayro の呼び出しの差し替えに限る。新しいロジックは新しいファイルに置く
- 既存の `engine/import.rs`、`dialogs/import.rs`、`canvas/imexport.rs`、`engine/snapshot.rs`、`fileformats/` は変更しない
- hayro と hayro-svg は `ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09` に固定する
- 全体モードの上限は長辺 4096 画素、ページ画像キャッシュの上限は 512 MB（計測後に見直す）
- UI の文字列は英語で、翻訳は付けない
- ビルドとテストはデスクトップ（`ssh desktop-ts`）の `/tmp/rnote-work` に作業ツリーを複製して行う。手元のノートでは cargo も nix build も実行しない
- コミットメッセージは英語の Conventional Commits で、末尾に `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` を付ける

## 作業ツリーの同期とコマンド

編集は手元の `~/sagyo/rnote` で行い、テストの前に毎回デスクトップへ同期する。

```bash
rsync -a --delete --exclude /target --exclude /_mesonbuild ~/sagyo/rnote/ desktop-ts:/tmp/rnote-work/
ssh desktop-ts 'cd /tmp/rnote-work && nix develop -c <command>'
```

flake は git の管理下のファイルしか見ない。
新しく作った Nix のファイルは、`nix develop` や `nix build` の前に `git add` しておく（`.git` ごと同期しているので、手元で add すればよい）。

以下、`remote <command>` と書いたら上の 2 行を実行することを指す。

## Review Focus

- ページサイズがページごとに違う PDF（横長の図版ページが混ざる教科書）: 格子が崩れず、行の高さがその行でいちばん高いページに合う。Task 5 のテストで確かめる
- 取り込んだあとに PDF を移動したり上書きしたりした場合: 起動が止まらず、該当ページが灰色の矩形になる。Task 4 のテストで確かめる
- 回転や拡大縮小をした `PdfPage`: 全体モードでも部分モードでも位置がずれない。Task 4 の部分モードのテストに、回転した要素の場合を入れる
- 極端に拡大したとき（部分モードの描画領域が巨大になる場合）: `u16` の上限を超えるバッファを作らない。Task 3 のテストで確かめる
- 同じ PDF を 2 回取り込んだ場合: `PdfSource` が 1 つだけ作られ、キャッシュも共有される。Task 3 のテストで確かめる

---

### Task 1: Nix の開発環境と、変更前のテストの確認

**Files:**
- Create: `flake.nix`
- Create: `nix/package.nix`
- Modify: `AGENTS.md`

**Interfaces:**
- Produces: `nix develop` で rustc 1.92 以上、meson、GTK4、libadwaita、typst、imagemagick が使える開発環境。`packages.<system>.default`（ハッシュは Task 8 で埋める）

- [ ] **Step 1: `nix/package.nix` を書く**

nixpkgs の `pkgs/by-name/rn/rnote/package.nix`（0.14.2）を元に、`src` を引数で受け取る。

```nix
{
  lib,
  stdenv,
  src,
  alsa-lib,
  appstream,
  appstream-glib,
  cargo,
  cmake,
  desktop-file-utils,
  dos2unix,
  glib,
  gst_all_1,
  gtk4,
  libadwaita,
  libxml2,
  meson,
  ninja,
  pkg-config,
  python3,
  rustPlatform,
  rustc,
  shared-mime-info,
  wrapGAppsHook4,
}:

stdenv.mkDerivation (finalAttrs: {
  pname = "rnote";
  version = (lib.importTOML ../Cargo.toml).workspace.package.version;
  inherit src;

  cargoDeps = rustPlatform.fetchCargoVendor {
    inherit (finalAttrs) pname version src;
    hash = lib.fakeHash;
  };

  nativeBuildInputs = [
    appstream-glib
    cmake
    desktop-file-utils
    dos2unix
    meson
    ninja
    pkg-config
    python3
    rustPlatform.bindgenHook
    rustPlatform.cargoSetupHook
    cargo
    rustc
    shared-mime-info
    wrapGAppsHook4
  ];

  dontUseCmakeConfigure = true;

  mesonFlags = [ (lib.mesonBool "cli" true) ];

  buildInputs = [
    appstream
    glib
    gst_all_1.gstreamer
    gtk4
    libadwaita
    libxml2
  ]
  ++ lib.optionals stdenv.hostPlatform.isLinux [ alsa-lib ];

  postPatch = ''
    chmod +x build-aux/*.py
    patchShebangs build-aux
  '';

  postInstall = ''
    substituteInPlace $out/share/thumbnailers/rnote.thumbnailer \
      --replace-fail "TryExec=rnote-cli" "TryExec=$out/bin/rnote-cli" \
      --replace-fail "Exec=rnote-cli" "Exec=$out/bin/rnote-cli"
  '';

  meta = {
    description = "Rnote with PDF page references (personal fork of flxzt/rnote)";
    homepage = "https://github.com/khimoo/rnote";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "rnote";
  };
})
```

- [ ] **Step 2: `flake.nix` を書く**

```nix
{
  description = "Rnote with PDF page references (personal fork of flxzt/rnote)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      forAllSystems = nixpkgs.lib.genAttrs [
        "x86_64-linux"
        "aarch64-linux"
      ];
    in
    {
      packages = forAllSystems (system: {
        default = nixpkgs.legacyPackages.${system}.callPackage ./nix/package.nix { src = self; };
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.default ];
            packages = [
              pkgs.clippy
              pkgs.rustfmt
              pkgs.typst
              pkgs.imagemagick
            ];
          };
        }
      );
    };
}
```

- [ ] **Step 3: lock を作り、開発環境の rustc を確かめる**

```bash
cd ~/sagyo/rnote && git add flake.nix nix/package.nix
remote 'nix flake lock && rustc --version && meson --version'
```

Expected: `rustc 1.92` 以上。`flake.lock` はデスクトップ側にできるので、`rsync -a desktop-ts:/tmp/rnote-work/flake.lock ~/sagyo/rnote/` で手元に戻して `git add` する。

- [ ] **Step 4: 変更前の engine のテストを通す**

Run: `remote 'cargo test -p rnote-engine 2>&1 | tail -20'`
Expected: すべて PASS。通ったテストの数を控えておく（Task 2 以降で減っていないことを確かめるため）。

- [ ] **Step 5: meson で UI の型検査が通ることを確かめる**

Run: `remote 'meson setup _mesonbuild -Dprofile=devel -Dcli=true 2>&1 | tail -3 && meson compile ui-cargo-check -C _mesonbuild 2>&1 | tail -5'`
Expected: エラーなし。

- [ ] **Step 6: `AGENTS.md` に開発環境の注意を足す**

「flake」の節の末尾に次を足す。

```markdown
開発環境は `nix develop` で入る。
flake は git の管理下のファイルしか見ないので、新しく作った Nix のファイルやソースは `nix develop` や `nix build` の前に `git add` しておく。
ビルドとテストはデスクトップ（`ssh desktop-ts`）の `/tmp/rnote-work` に作業ツリーを複製して行う（手順は実装計画の「作業ツリーの同期とコマンド」）。
```

- [ ] **Step 7: Commit**

```bash
git add flake.nix flake.lock nix/package.nix AGENTS.md
git commit -m "build: add nix flake for dev shell and package" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: テスト用の PDF と、既存の PDF 取り込みの特性テスト

hayro を上げる前に、既存のビットマップとベクターの取り込みの振る舞いをテストで固定する。

**Files:**
- Create: `crates/rnote-engine/tests/fixtures/pdf/make.sh`
- Create: `crates/rnote-engine/tests/fixtures/pdf/vector.typ`
- Create: `crates/rnote-engine/tests/fixtures/pdf/scan.typ`
- Create（生成物）: `crates/rnote-engine/tests/fixtures/pdf/{vector.pdf,scan.pdf,scan-page.jpg,mixed.pdf}`
- Create: `crates/rnote-engine/tests/fixtures/pdf/mixed.typ`
- Create: `crates/rnote-engine/tests/pdf_import.rs`

**Interfaces:**
- Produces: `tests/fixtures/pdf/vector.pdf`（A4 ベクター 3 ページ）、`scan.pdf`（A4 の JPEG だけのページ 2 枚）、`mixed.pdf`（A4 縦、A4 横、A5 縦の 3 ページ）。以降のタスクのテストが使う

- [ ] **Step 1: Typst の原稿と生成スクリプトを書く**

`vector.typ`:

```typst
#set page(paper: "a4", margin: 2cm)
#set text(size: 32pt)
#for i in range(1, 4) [
  = Page #i
  #lorem(40)
  #if i < 3 { pagebreak() }
]
```

`scan.typ`:

```typst
#set page(paper: "a4", margin: 0pt)
#image("scan-page.jpg", width: 100%, height: 100%, fit: "stretch")
#pagebreak()
#image("scan-page.jpg", width: 100%, height: 100%, fit: "stretch")
```

`mixed.typ`:

```typst
#set text(size: 32pt)
#page(paper: "a4")[Portrait A4]
#page(paper: "a4", flipped: true)[Landscape A4]
#page(paper: "a5")[Portrait A5]
```

`make.sh`:

```bash
#!/usr/bin/env bash
# Regenerates the Pdf fixtures. Run inside `nix develop`.
set -euo pipefail
cd "$(dirname "$0")"
magick -size 1240x1754 gradient:white-gray40 -quality 85 scan-page.jpg
typst compile vector.typ vector.pdf
typst compile scan.typ scan.pdf
typst compile mixed.typ mixed.pdf
```

- [ ] **Step 2: 生成して手元に戻す**

```bash
chmod +x crates/rnote-engine/tests/fixtures/pdf/make.sh
remote 'crates/rnote-engine/tests/fixtures/pdf/make.sh && ls -l crates/rnote-engine/tests/fixtures/pdf'
rsync -a desktop-ts:/tmp/rnote-work/crates/rnote-engine/tests/fixtures/pdf/ ~/sagyo/rnote/crates/rnote-engine/tests/fixtures/pdf/
```

Expected: 3 つの PDF がそれぞれ 200 KB 未満。

- [ ] **Step 3: 既存の取り込みの特性テストを書く**

`crates/rnote-engine/tests/pdf_import.rs`:

```rust
//! Pins the behaviour of the upstream Pdf import so the hayro update cannot change it unnoticed.

use p2d::math::Vector2;
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
        assert_a4_portrait(image.image.pixel_width as f64, image.image.pixel_height as f64);
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
```

- [ ] **Step 4: テストを実行する**

Run: `remote 'cargo test -p rnote-engine --test pdf_import'`
Expected: 2 件とも PASS（既存の振る舞いを固定するテストなので、最初から通る）。通らない場合は、`svg_data` に画像が `<image` 以外の形で入っていないか、`pixel_width` の丸めを確かめ、既存の振る舞いに合わせてテストの期待値を直す。

- [ ] **Step 5: Commit**

```bash
git add crates/rnote-engine/tests
git commit -m "test: pin upstream pdf import behavior with fixtures" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: hayro の更新と PdfSource

**Files:**
- Modify: `Cargo.toml`（`[workspace.dependencies]` の `hayro`、`hayro-svg`、`crc32fast` の追加）
- Modify: `crates/rnote-engine/Cargo.toml`（`crc32fast`）
- Modify: `Cargo.lock`
- Modify: `crates/rnote-engine/src/strokes/bitmapimage.rs`（hayro の呼び出しだけ）
- Modify: `crates/rnote-engine/src/strokes/vectorimage.rs`（hayro の呼び出しだけ）
- Create: `crates/rnote-engine/src/pdfsource.rs`
- Modify: `crates/rnote-engine/src/lib.rs`（`pub mod pdfsource;`）
- Modify: `crates/rnote-engine/src/meson.build`（`'pdfsource.rs',`）

**Interfaces:**
- Produces（`rnote_engine::pdfsource`）:
  - `pub struct PdfHash { pub crc32: u32, pub len: u64 }`（`Copy + Eq + Serialize + Deserialize + Default`）、`PdfHash::of(&[u8]) -> PdfHash`
  - `pub struct PageRaster { pub width: u32, pub height: u32, pub data: glib::Bytes }`、`PageRaster::placeholder() -> PageRaster`、`PageRaster::into_image(self, rectangle: Rectangle) -> Image`、`PageRaster::byte_len(&self) -> usize`
  - `pub struct PdfSource`、`PdfSource::open(&Path) -> anyhow::Result<Arc<PdfSource>>`、`path(&self) -> &Path`、`hash(&self) -> PdfHash`、`page_count(&self) -> usize`、`page_size(&self, usize) -> Option<Vector2>`（ポイント）、`whole_page_level(&self, usize, f64) -> Option<i32>`、`max_whole_page_level(&self, usize) -> i32`、`page_raster(&self, usize, i32) -> anyhow::Result<PageRaster>`、`render_region(&self, usize, Affine, Aabb, f64) -> anyhow::Result<PageRaster>`、`page_svg(&self, usize) -> anyhow::Result<String>`、`cache_used_bytes(&self) -> usize`
  - `pub use hayro::kurbo::Affine as PdfAffine`（`render_region` の引数の型。Rnote の kurbo と版が違う場合があるので、呼ぶ側はこの名前で作る）
  - 定数 `WHOLE_PAGE_MAX_PIXELS: f64 = 4096.0`、`DEFAULT_CACHE_BUDGET: usize = 512 * 1024 * 1024`

- [ ] **Step 1: hayro を上げる**

`Cargo.toml` の 2 行を差し替え、`crc32fast` を足す（アルファベット順の位置に）。

```toml
crc32fast = "1.5"
hayro = { git = "https://github.com/LaurenzV/hayro", rev = "ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09" }
hayro-svg = { git = "https://github.com/LaurenzV/hayro", rev = "ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09" }
```

`crates/rnote-engine/Cargo.toml` の `[dependencies]` に `crc32fast = { workspace = true }` を足す。

- [ ] **Step 2: 既存の呼び出しを新しい API に合わせる**

`bitmapimage.rs` の `from_pdf_bytes` で、`interpreter_settings` の直後にキャッシュを作り、描画設定と呼び出しを差し替える。

```rust
        let interpreter_settings = hayro_interpret::InterpreterSettings::default();
        let render_cache = hayro::RenderCache::new();
```

```rust
                let render_settings = hayro::RenderSettings {
                    x_scale: (pdf_import_prefs.bitmap_scalefactor * page_zoom) as f32,
                    y_scale: (pdf_import_prefs.bitmap_scalefactor * page_zoom) as f32,
                    bg_color: vello_cpu::color::AlphaColor::WHITE,
                };

                // TODO: implement drawing page borders.
                // Possibly with vello-cpu, since it already is a dependency of hayro
                let pixmap =
                    hayro::render(page, &render_cache, &interpreter_settings, &render_settings);
```

`vectorimage.rs` の `from_pdf_bytes` も同様に、`let render_cache = hayro_svg::RenderCache::new();` を足し、`hayro_svg::convert(page, &render_cache, &interpreter_settings, &render_settings)` にする。

- [ ] **Step 3: 既存のテストが通ることを確かめる**

Run: `remote 'cargo test -p rnote-engine 2>&1 | tail -20'`
Expected: Task 1 で控えた数と Task 2 の 2 件がすべて PASS。コンパイルエラーが `dialogs/import.rs`（rnote-ui）で出た場合は、エラーが指す hayro の型名だけを直す。`Cargo.lock` は cargo が更新する。

- [ ] **Step 4: PdfSource の失敗するテストを書く**

`crates/rnote-engine/src/pdfsource.rs` を作り、まずテストだけを置く（本体は Step 6）。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pdf").join(name)
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
        let region = Aabb::new(na_point(100.0, 200.0), na_point(300.0, 250.0));
        let raster = source
            .render_region(0, PdfAffine::IDENTITY, region, 8.0)
            .unwrap();
        assert_eq!((raster.width, raster.height), (1600, 400));
        assert!(page.x > 300.0);
    }

    #[test]
    fn region_scale_is_reduced_to_fit_u16() {
        let source = PdfSource::open(&fixture("vector.pdf")).unwrap();
        let region = Aabb::new(na_point(0.0, 0.0), na_point(500.0, 500.0));
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

    fn na_point(x: f64, y: f64) -> p2d::math::Vector2 {
        p2d::math::Vector2::new(x, y)
    }
}
```

`lib.rs` に `pub mod pdfsource;` を、`crates/rnote-engine/src/meson.build` のファイル一覧に `'pdfsource.rs',` を足す（一覧はアルファベット順なので `'lib.rs',` の近くの位置に）。

- [ ] **Step 5: テストが失敗することを確かめる**

Run: `remote 'cargo test -p rnote-engine pdfsource 2>&1 | tail -20'`
Expected: `PdfSource` などが未定義でコンパイルエラー。

- [ ] **Step 6: PdfSource を実装する**

テストの前（ファイルの先頭）に本体を書く。

```rust
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
use hayro::kurbo::Rect;
use hayro::vello_cpu::color::palette::css::WHITE;
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
/// Pixmaps are addressed with `u16`.
const MAX_BUFFER_SIDE: u32 = u16::MAX as u32;
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
        let bytes = std::fs::read(&path).with_context(|| format!("reading '{}'", path.display()))?;
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
                target_init: TargetInit::Clear(hayro::vello_cpu::color::palette::css::TRANSPARENT),
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

    fn page(&self, page_index: usize) -> anyhow::Result<&hayro::hayro_syntax::page::Page<'_>> {
        self.pdf
            .pages()
            .get(page_index)
            .ok_or_else(|| anyhow!("'{}' has no page {page_index}", self.path.display()))
    }
}
```

コンパイルエラーが出たら、エラーの指す型のパス（`ImageAlphaType` の場所、`RenderContext` のメソッド名、`Page` の型の場所）を hayro の `ae09437` と vello の `a9f11bbe` のソースで確かめて直す。ロジックは変えない。

- [ ] **Step 7: テストを通す**

Run: `remote 'cargo test -p rnote-engine pdfsource 2>&1 | tail -20'`
Expected: 9 件すべて PASS。`page_raster_is_cached_per_level` の画素数（A4 を 0.5 px/pt で描いて 297×420）が 1 画素ずれる場合は、hayro の丸め（`as u16` の切り捨て）に合わせて期待値を直す。

- [ ] **Step 8: 全体のテストと UI の型検査**

Run: `remote 'cargo test -p rnote-engine 2>&1 | tail -5 && meson compile ui-cargo-check -C _mesonbuild 2>&1 | tail -5'`
Expected: すべて PASS、型検査もエラーなし。

- [ ] **Step 9: Commit**

hayro の更新と PdfSource は分けてコミットする。

```bash
git add Cargo.toml Cargo.lock crates/rnote-engine/Cargo.toml crates/rnote-engine/src/strokes/bitmapimage.rs crates/rnote-engine/src/strokes/vectorimage.rs
git commit -m "build: update hayro to ae09437 for render_into" -m "Needed to render only the visible part of a Pdf page. Upstream pins an
older revision; rebases conflict on the hayro call sites." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git add crates/rnote-engine/src/pdfsource.rs crates/rnote-engine/src/lib.rs crates/rnote-engine/src/meson.build
git commit -m "feat(engine): add shared pdf source with page image cache" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: PdfPage 要素

**Files:**
- Create: `crates/rnote-engine/src/strokes/pdfpage.rs`
- Modify: `crates/rnote-engine/src/strokes/mod.rs`（`pub mod pdfpage;` と `pub use pdfpage::PdfPage;`）
- Modify: `crates/rnote-engine/src/strokes/stroke.rs`（variant と各 `match`）
- Modify: `crates/rnote-engine/src/store/trash_comp.rs`（2 箇所）
- Modify: `crates/rnote-engine/src/store/render_comp.rs`（1 箇所）
- Modify: `crates/rnote-engine/src/engine/strokecontent.rs`（1 箇所）
- Modify: `crates/rnote-engine/src/meson.build`（`'strokes/pdfpage.rs',`）

**Interfaces:**
- Consumes: Task 3 の `PdfSource`、`PdfHash`、`PageRaster`、`PdfAffine`
- Produces: `rnote_engine::strokes::PdfPage`、`PdfPage::new(source: Arc<PdfSource>, page_index: usize, rectangle: Rectangle) -> PdfPage`、フィールド `pdf_path: PathBuf`、`pdf_hash: PdfHash`、`page_index: usize`、`rectangle: Rectangle`（いずれも `pub`）、`Stroke::PdfPage(PdfPage)`

- [ ] **Step 1: 失敗するテストを書く**

`pdfpage.rs` の末尾に置く。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::strokes::content::GeneratedContentImages;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pdf").join(name)
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
        assert_eq!(loaded.rectangle.bounds(), page.rectangle.bounds());
        assert!(json.len() < 1000, "must not embed the Pdf: {} bytes", json.len());
    }

    #[test]
    fn zoomed_out_returns_cached_whole_page() {
        let page = page("vector.pdf");
        let generated = page.gen_images(page.bounds(), 0.25).unwrap();
        assert!(matches!(generated, GeneratedContentImages::Full(_)));
        let image = &images(generated)[0];
        // 0.25 px/pt rounds up to level -2.
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
        let center = page.bounds().center().coords;
        page.rotate(std::f64::consts::FRAC_PI_2, center);
        let bounds = page.bounds();
        let viewport = Aabb::new(bounds.mins, bounds.mins + Vector2::new(50.0, 50.0));
        let image = &images(page.gen_images(viewport, 20.0).unwrap())[0];
        assert_eq!((image.pixel_width, image.pixel_height), (1000, 1000));
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
```

`strokes/mod.rs` に宣言を足し、`meson.build` に `'strokes/pdfpage.rs',` を足す。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `remote 'cargo test -p rnote-engine pdfpage 2>&1 | tail -20'`
Expected: `PdfPage` が未定義でコンパイルエラー。

- [ ] **Step 3: PdfPage を実装する**

`pdfpage.rs` の先頭に書く。

```rust
//! A Pdf page referenced by path and rendered on demand, see [crate::pdfsource].

use super::content::GeneratedContentImages;
use super::{Content, Stroke};
use crate::pdfsource::{PageRaster, PdfAffine, PdfHash, PdfSource};
use crate::{Drawable, Image, Svg};
use p2d::bounding_volume::Aabb;
use p2d::math::Vector2;
use rnote_compose::Transformable;
use rnote_compose::ext::AabbExt;
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
        let resolved = OnceLock::new();
        let pdf_path = source.path().to_path_buf();
        let pdf_hash = source.hash();
        let _ = resolved.set(Ok(source));
        Self {
            pdf_path,
            pdf_hash,
            page_index,
            rectangle,
            source: Arc::new(resolved),
        }
    }

    fn source(&self) -> Result<&Arc<PdfSource>, &str> {
        self.source
            .get_or_init(|| {
                let resolved = self.resolve();
                if let Err(reason) = &resolved
                    && REPORTED.lock().unwrap().insert(self.pdf_path.clone())
                {
                    warn!("Pdf pages referencing '{}' are not rendered: {reason}", self.pdf_path.display());
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
            * PdfAffine::scale_non_uniform(2.0 * half[0] / page_size[0], 2.0 * half[1] / page_size[1])
    }

    fn pixels_per_point(&self, page_size: Vector2, image_scale: f64) -> f64 {
        let half = self.rectangle.cuboid.half_extents;
        image_scale * (2.0 * half[0] / page_size[0]).max(2.0 * half[1] / page_size[1])
    }

    fn placeholder(&self) -> Image {
        PageRaster::placeholder().into_image(self.rectangle.clone())
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
            .set("transform", self.rectangle.affine.to_svg_transform_attr_str())
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
                    raster.into_image(self.rectangle.clone()),
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
                    .whole_page_level(self.page_index, self.pixels_per_point(page_size, image_scale))
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
            .map_err(|e| anyhow::anyhow!("Make piet image in PdfPage draw impl failed, Err: {e:?}"))?;
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
```

`use` の過不足（`to_kurbo` や `to_svg_transform_attr_str` の trait、`kurbo::Shape`）は、`bitmapimage.rs` と `vectorimage.rs` の `use` に合わせて直す。

- [ ] **Step 4: Stroke に組み込む**

`stroke.rs`:

```rust
use super::pdfpage::PdfPage;
```

```rust
    #[serde(rename = "bitmapimage")]
    BitmapImage(BitmapImage),
    #[serde(rename = "pdfpage")]
    PdfPage(PdfPage),
```

`gen_svg`、`gen_images`、`draw_highlight`、`update_geometry`、`draw`、`draw_to_cairo`、`bounds`、`hitboxes`、`outline_path`、`translate`、`rotate`、`scale` の各 `match` に、`BitmapImage` の行の直後で同じ形の行を足す（例: `Stroke::PdfPage(pdfpage) => pdfpage.gen_svg(),`）。
その他のメソッドは次のとおり。

```rust
            // extract_default_layer
            Stroke::PdfPage(_) => StrokeLayer::Document,
            // set_to_inverted_brightness_color と set_to_darkest_color
            Stroke::PdfPage(_) => false,
            // into_xopp
            Stroke::PdfPage(_) => None,
```

`trash_comp.rs` の 2 箇所: `Stroke::TextStroke(_) | Stroke::VectorImage(_) | Stroke::BitmapImage(_)` の末尾に `| Stroke::PdfPage(_)` を足す。
`render_comp.rs`: 「regenerate everything」の分岐の末尾に `| Stroke::PdfPage(_)` を足す。
`strokecontent.rs`: `Stroke::VectorImage(image) => Some(image.rectangle.bounds()),` の直後に `Stroke::PdfPage(page) => Some(page.rectangle.bounds()),` を足す。

- [ ] **Step 5: テストを通す**

Run: `remote 'cargo test -p rnote-engine pdfpage 2>&1 | tail -20'`
Expected: 8 件すべて PASS。

- [ ] **Step 6: 全体のテスト、clippy、UI の型検査**

Run: `remote 'cargo test -p rnote-engine 2>&1 | tail -5 && cargo clippy -p rnote-engine --all-targets 2>&1 | tail -5 && meson compile ui-cargo-check -C _mesonbuild 2>&1 | tail -5'`
Expected: テストはすべて PASS、clippy に新しい警告なし、型検査もエラーなし。rnote-ui や rnote-cli で `Stroke` の網羅性エラーが出たら、同じ要領で `PdfPage` の分岐を足す。

- [ ] **Step 7: Commit**

```bash
git add crates/rnote-engine/src
git commit -m "feat(engine): add pdf page stroke rendered on demand" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 格子状の配置と要素の生成

**Files:**
- Create: `crates/rnote-engine/src/engine/import_pdfpages.rs`
- Modify: `crates/rnote-engine/src/engine/mod.rs`（`pub mod import_pdfpages;`）
- Modify: `crates/rnote-engine/src/meson.build`（`'engine/import_pdfpages.rs',`）

**Interfaces:**
- Consumes: `PdfSource`、`PdfPage::new`
- Produces: `rnote_engine::engine::import_pdfpages::grid_offsets(sizes: &[Vector2], columns: usize, gap: f64) -> Vec<Vector2>`、`Engine::generate_pdfpage_strokes(&self, source: &Arc<PdfSource>, pages: Range<usize>, columns: usize, gap_ratio: f64, insert_pos: Vector2) -> anyhow::Result<Vec<(Stroke, Option<StrokeLayer>)>>`（`gap_ratio` は列の幅に対する割合で、0.1 が 10%）

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn v(x: f64, y: f64) -> Vector2 {
        Vector2::new(x, y)
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
    fn generated_pages_share_one_source_and_scale_to_the_document_width() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pdf/mixed.pdf");
        let source = PdfSource::open(&path).unwrap();
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
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pdf/vector.pdf");
        let source = PdfSource::open(&path).unwrap();
        let engine = Engine::default();
        assert!(engine.generate_pdfpage_strokes(&source, 2..4, 8, 0.1, v(0.0, 0.0)).is_err());
        assert!(engine.generate_pdfpage_strokes(&source, 1..1, 8, 0.1, v(0.0, 0.0)).is_err());
    }
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `remote 'cargo test -p rnote-engine import_pdfpages 2>&1 | tail -20'`
Expected: `grid_offsets` などが未定義でコンパイルエラー。

- [ ] **Step 3: 実装する**

```rust
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
```

`engine/mod.rs` と `meson.build` に宣言と一覧を足す。

- [ ] **Step 4: テストを通す**

Run: `remote 'cargo test -p rnote-engine import_pdfpages 2>&1 | tail -20'`
Expected: 4 件すべて PASS。`Engine::default()` が使えない場合は、`engine/mod.rs` のテストが Engine を作る方法に合わせる。

- [ ] **Step 5: Commit**

```bash
git add crates/rnote-engine/src
git commit -m "feat(engine): lay out referenced pdf pages in a grid" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 取り込みのメニュー項目とダイアログ

**Files:**
- Create: `crates/rnote-ui/src/dialogs/import_pdfpages.rs`
- Modify: `crates/rnote-ui/src/dialogs/mod.rs`（`pub(crate) mod import_pdfpages;`）
- Modify: `crates/rnote-ui/src/appwindow/actions.rs`（アクションの作成と接続）
- Modify: `crates/rnote-ui/data/ui/appmenu.ui`（`_Import File` の直後）
- Modify: `crates/rnote-ui/src/meson.build`（`'dialogs/import_pdfpages.rs',`）

**Interfaces:**
- Consumes: `PdfSource::open`、`Engine::generate_pdfpage_strokes`、`Engine::import_generated_content`
- Produces: アクション `win.import-pdf-pages`

UI は単体テストを持たないので、型検査と手動の起動で確かめる。

- [ ] **Step 1: ダイアログを書く**

```rust
//! Imports a Pdf as page references, see [rnote_engine::strokes::PdfPage].

use crate::{RnAppWindow, RnCanvas};
use adw::prelude::*;
use gtk4::{FileDialog, FileFilter, gio};
use p2d::math::Vector2;
use rnote_compose::ext::Vector2Ext;
use rnote_engine::pdfsource::PdfSource;
use rnote_engine::strokes::Stroke;
use std::ops::Range;
use std::sync::Arc;
use tracing::debug;

struct Options {
    pages: Range<usize>,
    columns: usize,
    gap_ratio: f64,
}

pub(crate) async fn import_pdf_pages(appwindow: &RnAppWindow) {
    let Some(path) = choose_pdf(appwindow).await else {
        return;
    };
    let opened = gio::spawn_blocking(move || PdfSource::open(&path)).await;
    let source = match opened {
        Ok(Ok(source)) => source,
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
    let Some(options) = ask_options(appwindow, &source).await else {
        return;
    };
    let Some(canvas) = appwindow.active_tab_canvas() else {
        return;
    };
    let insert_pos = default_insert_pos(&canvas);
    let generated = canvas.engine_ref().generate_pdfpage_strokes(
        &source,
        options.pages,
        options.columns,
        options.gap_ratio,
        insert_pos,
    );
    match generated {
        Ok(strokes) => {
            let widget_flags = canvas.engine_mut().import_generated_content(strokes, false);
            canvas.emit_handle_widget_flags(widget_flags);
        }
        Err(e) => {
            appwindow
                .overlays()
                .dispatch_toast_error(&format!("Importing the PDF failed: {e:#}"));
        }
    }
}

async fn choose_pdf(appwindow: &RnAppWindow) -> Option<std::path::PathBuf> {
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

async fn ask_options(appwindow: &RnAppWindow, source: &Arc<PdfSource>) -> Option<Options> {
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

    let first = adw::SpinRow::with_range(1.0, page_count as f64, 1.0);
    first.set_title("First Page");
    first.set_value(1.0);
    let last = adw::SpinRow::with_range(1.0, page_count as f64, 1.0);
    last.set_title("Last Page");
    last.set_value(page_count as f64);
    let columns = adw::SpinRow::with_range(1.0, 64.0, 1.0);
    columns.set_title("Columns");
    columns.set_value(8.0);
    let gap = adw::SpinRow::with_range(0.0, 200.0, 5.0);
    gap.set_title("Gap Between Pages (%)");
    gap.set_value(10.0);

    let rows = gtk4::ListBox::new();
    rows.add_css_class("boxed-list");
    rows.set_selection_mode(gtk4::SelectionMode::None);
    for row in [&first, &last, &columns, &gap] {
        rows.append(row);
    }
    dialog.set_extra_child(Some(&rows));

    if dialog.choose_future(Some(appwindow)).await != "import" {
        return None;
    }
    let first = first.value() as usize;
    let last = (last.value() as usize).max(first);
    Some(Options {
        pages: first - 1..last,
        columns: columns.value() as usize,
        gap_ratio: gap.value() / 100.0,
    })
}

/// Same position as the upstream importer (`RnCanvas::determine_stroke_import_pos`, which is private).
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

- [ ] **Step 2: アクションとメニューをつなぐ**

`actions.rs` で、`action_import_file` の作成の直後に足す。

```rust
        let action_import_pdf_pages = gio::SimpleAction::new("import-pdf-pages", None);
        self.add_action(&action_import_pdf_pages);
```

`action_import_file.connect_activate(...)` のブロックの直後に足す。

```rust
        // Import Pdf as page references
        action_import_pdf_pages.connect_activate(clone!(
            #[weak(rename_to=appwindow)]
            self,
            move |_, _| {
                glib::spawn_future_local(clone!(
                    #[weak]
                    appwindow,
                    async move {
                        dialogs::import_pdfpages::import_pdf_pages(&appwindow).await;
                    }
                ));
            }
        ));
```

`appmenu.ui` の `_Import File` の `<item>` の直後に足す。

```xml
          <item>
            <attribute name="label">Import PDF as Page _References…</attribute>
            <attribute name="action">win.import-pdf-pages</attribute>
          </item>
```

- [ ] **Step 3: 型検査と clippy**

Run: `remote 'meson compile ui-cargo-check -C _mesonbuild 2>&1 | tail -20 && meson compile ui-cargo-clippy -C _mesonbuild 2>&1 | tail -10'`
Expected: エラーなし。`choose_future` や `open_future` の引数の型が合わない場合は、同じファイル群（`dialogs/import.rs`、`dialogs/mod.rs`）での呼び方に合わせる。`RnCanvas` の `emit_handle_widget_flags` が非公開なら、`imexport.rs` が使っている公開の経路に合わせる。

- [ ] **Step 4: 起動して取り込めることを確かめる**

デスクトップでビルドし、手元のノートで起動する（flake_public の `cargo remote-run` と同じ流れ）。

```bash
remote 'meson compile ui-cargo-build -C _mesonbuild 2>&1 | tail -3'
```

利用者に、アプリのメニューの「Import PDF as Page References…」から `vector.pdf` と `scan.pdf` を取り込み、縮小、拡大、ページの移動、保存と再読み込みができることを確かめてもらう。

- [ ] **Step 5: Commit**

```bash
git add crates/rnote-ui
git commit -m "feat(ui): add dialog to import pdf as page references" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: メモリの計測

**Files:**
- Create: `crates/rnote-engine/examples/pdfpage_bench.rs`
- Create: `crates/rnote-engine/examples/pdfpage_bench.typ`
- Modify: `docs/superpowers/specs/2026-09-28-pdf-page-strokes-design.md`（計測結果の節を足す）

**Interfaces:**
- Consumes: `PdfSource`、`PdfPage`、`grid_offsets`、`BitmapImage::from_pdf_bytes`（比較用）

- [ ] **Step 1: 計測用の PDF の原稿を書く**

`pdfpage_bench.typ`（`scan-page.jpg` を 300 ページに貼る。Typst は同じ画像を 1 回だけ埋め込むので PDF は小さいが、描くたびに展開される点はスキャンと同じ）:

```typst
#set page(paper: "a4", margin: 0pt)
#for i in range(300) {
  image("../tests/fixtures/pdf/scan-page.jpg", width: 100%, height: 100%, fit: "stretch")
  if i < 299 { pagebreak() }
}
```

- [ ] **Step 2: 計測用の example を書く**

```rust
//! Measures what Pdf page strokes allocate for a large Pdf.
//!
//! typst compile crates/rnote-engine/examples/pdfpage_bench.typ /tmp/bench.pdf
//! cargo run --release -p rnote-engine --example pdfpage_bench -- /tmp/bench.pdf

use p2d::bounding_volume::Aabb;
use rnote_compose::shapes::{Rectangle, Shapeable};
use rnote_engine::document::Format;
use rnote_engine::engine::import::PdfImportPrefs;
use rnote_engine::engine::import_pdfpages::grid_offsets;
use rnote_engine::pdfsource::PdfSource;
use rnote_engine::strokes::content::Content;
use rnote_engine::strokes::{BitmapImage, PdfPage};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

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

fn report(label: &str, base: usize) {
    println!(
        "{label:<32} retained {:>8.1} MiB  peak {:>8.1} MiB",
        mib(CURRENT.load(Ordering::Relaxed).saturating_sub(base)),
        mib(PEAK.load(Ordering::Relaxed).saturating_sub(base)),
    );
    PEAK.store(CURRENT.load(Ordering::Relaxed), Ordering::Relaxed);
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("usage: pdfpage_bench <file.pdf>");
    let base = CURRENT.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);

    let source = PdfSource::open(path.as_ref())?;
    let zoom = Format::default().width() / source.page_size(0).unwrap()[0];
    let sizes: Vec<_> = (0..source.page_count())
        .map(|i| source.page_size(i).unwrap() * zoom)
        .collect();
    let offsets = grid_offsets(&sizes, 8, sizes[0][0] * 0.1);
    let pages: Vec<PdfPage> = sizes
        .iter()
        .zip(&offsets)
        .enumerate()
        .map(|(i, (size, offset))| {
            PdfPage::new(source.clone(), i, Rectangle::from_corners(*offset, offset + size))
        })
        .collect();
    report("import", base);

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
    report("overview, all pages kept", base);
    drop(kept);

    let first = pages[0].bounds();
    for _ in 0..3 {
        for step in 0..12 {
            let scale = overview_scale * 2f64.powi(step);
            let half = first.extents() / (2.0 * scale / overview_scale);
            let viewport = Aabb::new(first.center() - half, first.center() + half);
            let images: Vec<_> = pages[..16]
                .iter()
                .map(|page| page.gen_images(viewport, scale))
                .collect::<Result<_, _>>()?;
            drop(images);
        }
    }
    report("zoom sweep x3, first 16 pages", base);
    println!("cache {:.1} MiB", mib(source.cache_used_bytes()));
    drop(pages);
    drop(source);
    report("dropped", base);

    if std::env::args().any(|arg| arg == "--bitmap-baseline") {
        let bytes = std::fs::read(&path)?;
        let images = BitmapImage::from_pdf_bytes(
            &bytes,
            PdfImportPrefs::default(),
            Default::default(),
            None,
            &Format::default(),
            None,
        )?;
        report("upstream bitmap import", base);
        drop(images);
    }
    Ok(())
}
```

`Aabb` の `merged` や `center` の名前が違ったら、parry2d の `BoundingVolume` の API に合わせる。

- [ ] **Step 3: 計測する**

```bash
remote 'typst compile crates/rnote-engine/examples/pdfpage_bench.typ /tmp/bench.pdf && cargo run --release -p rnote-engine --example pdfpage_bench -- /tmp/bench.pdf --bitmap-baseline'
```

Expected: 「import」の retained が PDF のファイルサイズ程度、「overview」が 150 MiB 以内、cache が 512 MiB 以内。本家のビットマップ取り込みの retained がそれより大きいこと。

目標を外れた場合は、原因（どの段で何が確保されたか）を調べてから `WHOLE_PAGE_MAX_PIXELS` と `DEFAULT_CACHE_BUDGET` を見直す。

- [ ] **Step 4: 結果を設計書に書く**

設計書の「要検証」の前に「計測結果」の節を足し、計測したコミット、マシン、PDF、各行の数値を書く。
上限の値を変えた場合は、その理由も書く。

- [ ] **Step 5: Commit**

```bash
git add crates/rnote-engine/examples docs/superpowers/specs
git commit -m "test(engine): add memory benchmark for pdf page strokes" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: パッケージのビルドと公開

**Files:**
- Modify: `nix/package.nix`（`cargoDeps` のハッシュ）

- [ ] **Step 1: パッケージのハッシュを埋める**

```bash
remote 'nix build .#default -L 2>&1 | grep -E "got:|error" | head -5'
```

`got:` に出たハッシュを `nix/package.nix` の `lib.fakeHash` と差し替え、もう一度ビルドする。

Run: `remote 'nix build .#default -L 2>&1 | tail -5 && ls result/bin'`
Expected: `rnote` と `rnote-cli` がある。

- [ ] **Step 2: Commit と push**

```bash
git add nix/package.nix
git commit -m "build: pin cargo vendor hash" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push origin main
```

- [ ] **Step 3: flake_public から取り込めることを確かめる**

flake_public は変更せずに、GitHub の flake を評価できることを確かめる。

Run: `ssh desktop-ts 'nix eval --raw github:khimoo/rnote#packages.x86_64-linux.default.name'`
Expected: `rnote-0.15.0`

flake_public に足す内容（入力と、`modules/home-manager/gui/apps.nix` での使い方）を利用者に示す。

---

## Self-Review

- 設計書の各節との対応: 目的の成功条件は Task 7、PdfSource は Task 3、PdfPage 要素と描画は Task 4、取り込みは Task 5 と 6、hayro の更新は Task 3、ビルドと flake は Task 1 と 8、テストは Task 2 から 5、退けた案と要検証は Task 7 で更新する
- 「描き直しの間も古い画像が残るか」は単体テストでは確かめられないので、Task 6 の Step 4 の手動確認で見る
