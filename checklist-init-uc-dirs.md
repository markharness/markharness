# Task: init コマンドの改善(存在しないディレクトリの自動作成 + UC1〜UC8想定の物理ディレクトリ構成)
Created: 2026-08-07

## 背景

- 現状の `run_init`(`src/init.rs`)は `SUBDIRS = ["knowledge", "generated", "changes"]` のみを対象とし、走査中に**最初に見つかった既存ディレクトリでエラー終了**する。そのため、例えば `knowledge/` は既にあるが `generated/`・`changes/` がまだ無い(部分初期化状態)というケースで、本来作成できるはずのディレクトリすら作られずに失敗する。これが「ディレクトリが存在しなければ作成する」という要求に反している。
- 物理ディレクトリ構成は論文(`docs/テスト知識管理のGit-nativeモデル_統合版V2.md` §3.5, 244-273行目)で `knowledge/ axes/ generated/ executions/ changes/ schema/ tools/` の7つが定義されているが、現在の実装は3つしか作らない。`docs/product-operation.md` のUC1〜UC8に対応させるには以下が必要:
  - `knowledge/` — UC1, UC1b
  - `axes/` — UC1(横断的観点レジストリ、§3.1)
  - `generated/` — UC2, UC3
  - `executions/` — UC4(マイルストーン・実行結果、ER図の TESTEXECUTION/MILESTONE)
  - `changes/` — UC5, UC6
  - `schema/` — UC7(id-index_schema_version 等、フォーマット・正規化ルール定義)
  - `tools/` — UC2/UC5/UC6/UC7 で使う生成・検証スクリプト置き場
  - UC8(既存ツールからのインポート)は専用ディレクトリを持たず `knowledge/` に書き込む想定のため対象外

## 決定事項(実装前に確定)

- `--force` オプションと「既存なら即エラー」という現行仕様は、部分初期化状態での自動作成を妨げる原因そのものなので廃止する。`run_init` は常に「無いディレクトリだけ作る・あるディレクトリには触らない」冪等な動作にする(中身の削除・上書きは元々していないため、この変更で破壊的操作が増えることはない)。
- それに伴い `run_init` のシグネチャから `force: bool` を削除し、CLI (`markharness init`) からも `--force` フラグを削除する。呼び出し側(`cli.rs` / テストコード)・ドキュメント(`docs/cli-manual.md` / `PROJECT.md`)も追従させる。

## Steps

- [x] Step 1: `src/init.rs` の `SUBDIRS` を7つ(`knowledge, axes, generated, executions, changes, schema, tools`)に拡張し、`run_init` から `force` 引数と AlreadyExists エラーを削除して「無ければ作る・あれば触らない」冪等な実装に変更(Red: 既存テストを更新 → Green: 実装修正)
- [x] Step 2: `src/init.rs` のテストを更新
  - 空ディレクトリから7ディレクトリ全て作成されることを確認するテスト
  - 部分初期化状態(例: `knowledge/` のみ既存)から実行しても残りが作成され、既存ファイルが保持されることを確認するテスト
  - 全て既に存在する状態で再実行してもエラーにならないことを確認するテスト
- [x] Step 3: `src/cli.rs` の `Init` サブコマンドから `force` フィールドを削除し、`init::run_init(&root)` の呼び出しに合わせる
- [x] Step 4: `src/generate.rs` / `src/interactive.rs` のテスト内の `run_init(dir.path(), false)` 呼び出しを `run_init(dir.path())` に更新(`cargo test` 23件全パス確認済み)
- [x] Step 5: `docs/cli-manual.md` §1.1(`markharness init`)を新仕様(7ディレクトリ作成・`--force`廃止・冪等動作)に更新
- [x] Step 6: `PROJECT.md` の「ディレクトリ構成」節に `axes/ executions/ schema/ tools/` を追記し、各ディレクトリとUCの対応を明記
- [x] Step 7: `cargo fmt` / `cargo test` / `cargo clippy --all-targets -- -D warnings` を実行し全てパスすることを確認(23テスト全パス、clippy警告0件)

## Notes
- `--force` 廃止は元のプロンプトには明記されていないが、「存在しなければ作成する」という要求を素直に満たすには、既存ディレクトリ時に即エラーで止まる現行ロジックとの整合が取れないため、冪等動作への変更として判断した。

## Summary
`run_init` を「無ければ作る・あれば触らない」冪等な実装に変更し、対象ディレクトリを論文§3.5に基づく7種(`knowledge/ axes/ generated/ executions/ changes/ schema/ tools/`)に拡張した。`--force` フラグと AlreadyExists エラーは、部分初期化状態での自動作成を妨げていたため廃止。CLI・テスト・ドキュメント(`docs/cli-manual.md`, `PROJECT.md`)を追従させ、`cargo fmt` / `cargo test` / `cargo clippy` は全てパス。
