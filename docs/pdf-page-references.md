# PDF をページ参照として取り込む

教科書の全ページを格子状に並べ、拡大して手書きで書き込むための個人用 Rnote fork。
実装は `khimoo/rnote` の `main` にある。

## ビルド済みのアプリを試す

2026-09-29 時点では、デスクトップのターミナルで次を実行できる。

```bash
/tmp/rnote-work/result/bin/rnote
```

これは一時的なビルド成果物で、`/tmp` の掃除や再起動後も残るとは限らない。
再ビルドはデスクトップで行う。

```bash
nix build github:khimoo/rnote --out-link /tmp/rnote-result
/tmp/rnote-result/bin/rnote
```

PDF は外部ファイルへの参照として保存する。取り込み後も元の PDF を同じ場所に残す。
2 台で文書を開く場合は、PDF を両方の端末の同じ絶対パスに置く。
PDF の移動・差し替えは、アプリを閉じてから行い、再起動後の参照切れを確認する。
ページ参照を含む `.rnote` は、この fork で開く。
Xournal++ への書き出しでは PDF のページ参照は省かれる。

## 手動確認

文書のレイアウトを「無限」にし、メニューの
「Import PDF as Page References…」から PDF を選ぶ。
ページ範囲、列数、間隔を指定して取り込む。

まず次の小さな PDF を使う。デスクトップの作業ツリーにも同じパスがある。

- `crates/rnote-engine/tests/fixtures/pdf/vector.pdf`
- `crates/rnote-engine/tests/fixtures/pdf/scan.pdf`
- `crates/rnote-engine/tests/fixtures/pdf/mixed.pdf`

確認項目:

- [ ] 格子状に配置され、取り消し1回で取り込み全体を戻せる。
- [ ] 縮小して全ページを見渡せる。拡大すると電子版の文字が鮮明になる。
- [ ] 拡大縮小や移動の最中に、描き直し待ちのページが不自然に消えない。
- [ ] ページを移動・回転・拡大縮小しても描画位置が合う。
- [ ] ペンで書き込み、保存してアプリを閉じ、再び開いてページと書き込みが残る。
- [ ] 実際の日本語の教科書と高解像度のスキャンで、文字表示と速度が実用になる。
- [ ] 2 台の端末と液タブで書き心地を確認する。

## flake_public への組み込み

この作業では flake_public は変更していない。
`flake.nix` の `inputs` に追加する。

```nix
rnote.url = "github:khimoo/rnote";
```

Rnote が固定している nixpkgs を使うため、`follows` は付けない。
`modules/home-manager/gui/apps.nix` の引数に `inputs` を追加する。
既存の `lib/configurations.nix` は `extraSpecialArgs` に `inputs` を渡している。

```nix
{ config, pkgs, kiro, lib, inputs, ... }:
```

同ファイルの `guiApps` リストに追加する。

```nix
{ pkg = inputs.rnote.packages.${pkgs.stdenv.hostPlatform.system}.default; }
```

flake_public 側の lock 更新・構成評価・適用は別作業。
ビルドはデスクトップで行い、このノートPCでは行わない。

## 引き継ぎ時の検証（2026-09-29）

実装コミット `98ffb352` をデスクトップで検証した。

- `cargo test -p rnote-engine`: 単体24件、統合2件が成功。doc test 1件は既存の ignored。
- `meson compile cli-cargo-check ui-cargo-check -C _mesonbuild`: 成功。
- Nix パッケージ: 前セッションでビルド成功した成果物を確認し、今回起動した。
  パッケージを作った `af7b4b1c` と `98ffb352` の差分は回転描画のテスト追加だけ。
- 独立した D-Bus と Xvfb の仮想画面で GUI を起動。
  `import-pdf-pages` アクションが有効であることを D-Bus から確認した。
- GitHub へ push し、`nix eval --refresh --raw github:khimoo/rnote#packages.x86_64-linux.default.name`
  が `rnote-0.15.0` を返すことを確認した。

仮想画面での確認は、上の手動操作や液タブの確認の代わりにはならない。
性能計測は Rust のアロケータだけを対象にし、GTK のテクスチャは含まない。
詳細は [設計書](superpowers/specs/2026-09-28-pdf-page-strokes-design.md) を参照。
