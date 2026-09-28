# khimoo/rnote（Rnote の個人用 fork）

flxzt/rnote を個人で使うために改造する fork。
変更の置き場所は khimoo/rnote の `main` だけ。

## 本家とのやり取り

flxzt/rnote には PR、issue、コメント、レビューを一切出さない。
本家の issue や PR、コードを読むのは自由。
`upstream` remote は push できないように設定してあり、`gh` の既定のリポジトリは khimoo/rnote にしてある。

## 本家への追従

`git fetch upstream && git rebase upstream/main` のあと、`git push --force-with-lease origin main` で反映する。

rebase の衝突を小さく保つため、機能は新しいファイルに置き、本家のファイルへの変更は分岐の追加と呼び出しの差し替えにとどめる。
衝突が出やすいのは、`crates/*/src/meson.build` のファイル一覧と、hayro を本家より新しい版に上げたことに伴う変更（`Cargo.toml`、`Cargo.lock`、`strokes/bitmapimage.rs`、`strokes/vectorimage.rs` の hayro 呼び出し）。

## flake

`flake.nix` は `~/sagyo/flake_public` から `github:khimoo/rnote` として取り込まれる。
`packages.<system>.default` の名前や中身を変えるときは、flake_public 側の評価も確かめる。

開発環境は `nix develop` で入る。
flake は git の管理下のファイルしか見ないので、新しく作った Nix のファイルやソースは `nix develop` や `nix build` の前に `git add` しておく。
ビルドとテストはデスクトップ（`ssh desktop-ts`）の `/tmp/rnote-work` に作業ツリーを複製して行う（手順は [実装計画](docs/superpowers/plans/2026-09-28-pdf-page-strokes.md) の「作業ツリーの同期とコマンド」）。

## 設計

PDF のページを参照で取り込む機能の設計は [docs/superpowers/specs/2026-09-28-pdf-page-strokes-design.md](docs/superpowers/specs/2026-09-28-pdf-page-strokes-design.md) にある。
