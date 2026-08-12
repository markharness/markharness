# markharness

Git そのものをバックエンドにした、テスト知識(Feature / Condition / ExpectedResult)の Git-native 管理 CLI(Rust実装)です。`knowledge/` に YAML で手動記述したテスト知識から `TestCase` を決定的に生成し、マイルストーンタグ間の Git tree SHA 比較によって `ChangeEvent`(Featureごとの版履歴の差分ログ)をブランチ運用非依存で自動計算します。

設計の背景は [docs/テスト知識管理のGit-nativeモデル_統合版.md](./docs/テスト知識管理のGit-nativeモデル_統合版.md)、プロダクトとしての詳細は [PROJECT.md](./PROJECT.md) を参照してください。

> このリポジトリはもともと汎用AI開発テンプレートから作られています。テンプレート自体の説明(`/customize` 等のスラッシュコマンド)は [docs/template-readme.md](./docs/template-readme.md) に退避してあります。

## 最小チュートリアル

このセクションのコマンドは全て `cargo build --release` 後の `target/release/markharness`(Windowsは `.exe`)を指します。以下、`markharness` と表記します。

サンプルの知識データ一式は [examples/todo-minimal/](./examples/todo-minimal/) にあります。外部リポジトリへの依存はなく、このリポジトリの中だけで完結します。

```bash
# 1. 新しいプロジェクト用の空リポジトリを用意する
mkdir my-todo-project && cd my-todo-project
git init

# 2. markharness init — knowledge/ / axes/ / generated/ / executions/ / changes/ / schema/ を作成
markharness init

# 3. 知識登録 — examples/todo-minimal/ の axis レジストリとドラフトYAMLを使う
cp -r <markharness のクローン先>/examples/todo-minimal/axes .
markharness knowledge apply <markharness のクローン先>/examples/todo-minimal/draft-v1.yml

# 4. 生成 — knowledge/ から TestCase を決定的に生成する
markharness generate

# 5. マイルストーン(git tag) — 最初のリリース地点にタグを打つ
git add -A && git commit -m "add todo-management/add-todo knowledge"
git tag v1
markharness milestone init v1

# --- ここで仕様が変わったとする(examples/todo-minimal/draft-v2.yml は
#     同じ Feature に新しい Condition を1件追加するドラフト) ---
markharness knowledge apply <markharness のクローン先>/examples/todo-minimal/draft-v2.yml
markharness generate
git add -A && git commit -m "add max-length condition"
git tag v2
markharness milestone init v2

# 6. changes compute — v1..v2 間の ChangeEvent を自動計算する
markharness changes compute v1 v2
cat changes/v2.yaml

# 7. 実行結果を記録してから、未再検証のTestCaseを確認する
markharness execution record tc-empty-title-001 --milestone v2 --result pass --executor <your-name>
markharness execution record tc-max-length-001 --milestone v2 --result pass --executor <your-name>
markharness verify pending --from v1 --to v2
```

最後の `verify pending` は、`v1..v2` で影響を受けた2件のTestCase(`tc-empty-title-001` / `tc-max-length-001`)がいずれも `v2` 時点で実行記録済みであることを検出し、`pending`(未再実行)を0件と報告します。両方のステップを省略して直接 `verify pending` を実行すると、逆にこの2件が pending として出力されます(実際に上のコマンド列で手元確認済み)。

各コマンドの詳細なオプション・出力形式は [docs/cli-manual.md](./docs/cli-manual.md) を参照してください。

## 運用上の制約

- **Gitタグがマイルストーンの前提**：`changes compute` / `backfill run` は `git tag` された地点しかマイルストーンとして扱えません。タグを打たない限りリリース境界を認識できません(UC4のタグ付け自体は人間の判断ポイントであり、`markharness` は代行しません)。
- **`git notes` は push/fetch で自動同期されません**：バックフィルの進捗記録([第4.3節](./docs/テスト知識管理のGit-nativeモデル_統合版.md))は `refs/notes/markharness-backfill` に保存されますが、これは通常の `git push`/`git fetch` の対象外です。共有リポジトリでチーム運用する場合は、`git push origin refs/notes/*` と対応する fetch 設定(`git config --add remote.origin.fetch '+refs/notes/*:refs/notes/*'` 等)を各メンバー・CI環境で追加してください。
- **既存TMS(TestRail / Xray 等)からの移行は未実装**：UC8(既存ツールからのインポート)は未実装です。移行は手作業で `knowledge/` 配下のYAMLを作成する(または `markharness knowledge apply`/`add` を使う)ことになります。詳細は [PROJECT.md](./PROJECT.md) の主要機能一覧を参照してください。

## 未対応事項

[docs/テスト知識管理のGit-nativeモデル_統合版.md §3.6 実装状況まとめ](./docs/テスト知識管理のGit-nativeモデル_統合版.md#36-実装状況まとめ)を参照してください。要点:

- 既存TMS(TestRail/Xray等)からのインポータ(UC8) — 未実装。
- id解決キャッシュの `canonicalization_rule_version` / `id_index_schema_version` — 現状固定値で、実際の改訂運用は未検証。
- id⇔pathの汎用的な独立インデックス層(パスを変えないid変更の追跡等) — 未実装。
- `verify trace` / `verify pending` — 導入前の既存実行記録には遡及適用されない。`executions/*/results.yml` 用のJSON Schemaも未整備。
- `markharness backfill run` — 常駐デーモンではなく、呼び出しごとに未処理ペアを1パス処理して終了する設計(CI等からの反復呼び出しを前提とする)。

## 開発

Rust(edition 2024)実装です。ビルド・テスト・Lintの標準コマンドは [PROJECT.md](./PROJECT.md#技術スタック) を参照してください。

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## ドキュメント一覧

- [PROJECT.md](./PROJECT.md) — プロダクト概要・技術スタック・ディレクトリ構成
- [docs/README.md](./docs/README.md) — 設計ドキュメント群の索引(論文・製品運用イメージ・CLIマニュアル・個別コマンド仕様)
- [docs/cli-manual.md](./docs/cli-manual.md) — 実装済み/未実装のCLIコマンド一覧
- [docs/テスト知識管理のGit-nativeモデル_統合版.md](./docs/テスト知識管理のGit-nativeモデル_統合版.md) — 設計の元になった研究(論文ドラフト)
