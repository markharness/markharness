# PROJECT.md — プロジェクト定義

> **このファイルがテンプレートの唯一のカスタマイズポイントです。**
> `.github/` 配下の instructions / prompts / skills はすべてこのファイルを参照します。
> 新しいプロダクトを作るときは `/customize` プロンプトで書き換えてください。
> `<!-- CUSTOMIZE -->` が付いたセクションがカスタマイズ対象です。

## プロダクト概要 <!-- CUSTOMIZE -->

| 項目 | 値 |
|------|----|
| プロダクト名 | markharness |
| 概要 | Git そのものをバックエンドにした、テスト知識(Feature / Condition / ExpectedResult)の Git-native 管理 CLI。`knowledge/` に YAML で手動記述されたテスト知識から `TestCase` を決定的に生成し、マイルストーンタグ間の Git blob SHA 比較によって版履歴(`derived_from` / `ChangeEvent`)をブランチ運用非依存で自動計算する。設計の元になった研究は `docs/テスト知識管理のGit-nativeモデル_統合版V2.md`、製品化した運用イメージは `docs/product-operation.md`、TestCase 生成アルゴリズムの詳細設計は `docs/testcase-generation-design.md` を参照。 |
| 主要機能 | 右の一覧を参照 |

- `knowledge/**/{feature,condition,expected/*}.yaml` へのテスト知識の手動記述(UC1)
- Feature + Behavior + Condition からの `TestCase` 決定的生成(`markharness generate`)と CI 差分検証(`markharness verify`、`generated/testcases/*.yml`、UC2/UC3)
- マイルストーンタグ間の `ChangeEvent`(`derived_from`)自動計算 — blob SHA 比較 + `git merge-base`(UC5、核心的貢献)
- 大規模既存リポジトリ向けの非同期・優先度付きバックフィル(`git notes` で進捗管理、UC6)
- id 解決キャッシュの破棄・再構築(UC7)、既存 TMS(TestRail / Xray 等)からのインポート(UC8)

## 技術スタック <!-- CUSTOMIZE -->

テンプレートのデフォルト(TypeScript + Vitest + ESLint)から Rust に変更しています(`Cargo.toml` / `src/main.rs` 参照)。

| 項目 | 値 |
|------|----|
| 言語 | Rust(edition 2024) |
| テスト | `cargo test`(標準テストハーネス。conformance テストは `spec/` 配下に追加予定) |
| Lint / Format | `cargo clippy` / `cargo fmt` |
| ビルド | `cargo build` |

### 標準コマンド

| 用途 | コマンド |
|------|---------|
| ビルド | `cargo build` |
| テスト(全件) | `cargo test` |
| テスト(単体) | `cargo test <test-name>` |
| Lint | `cargo clippy --all-targets -- -D warnings` |
| フォーマット | `cargo fmt` |
| フォーマットチェック | `cargo fmt --check` |
| 脆弱性スキャン | `cargo audit` |

> **Windows での既知の注意点**: MSVC Build Tools がない環境では既定の `msvc` ツールチェインでリンクできない場合がある。その場合は `rustup override set stable-x86_64-pc-windows-gnu` と WinLibs(mingw64)の `bin` を `PATH` に追加する。

## 認証情報・シークレット <!-- CUSTOMIZE -->

| 項目 | 値 |
|------|----|
| 認証情報ディレクトリ | 該当なし |
| 格納ファイル | 該当なし |

markharness は Git リポジトリ自身(ワーキングツリー・blob SHA・`git notes`・タグ)のみを入出力とするローカル CLI であり、外部サービスへの認証は不要。将来 UC8(TestRail / Xray 等の既存 TMS からのインポート)で API 連携が必要になった場合は、このセクションと `.github/instructions/security.instructions.md` の方針(ワークスペース外保存)に従って追記する。

## 外部 API 連携 <!-- CUSTOMIZE -->

| API | 用途 | 認証方式 |
|-----|------|---------|
| (なし — Git 自身がバックエンド。UC8 のインポータ実装時に対象 TMS の API を追記) | | |

## ディレクトリ構成

`markharness init` は以下7ディレクトリ(論文 §3.5、UC1〜UC8の前提)を、存在しないものだけ作成する(既存ディレクトリ・ファイルはそのまま)。UC8(既存ツールからのインポート)は専用ディレクトリを持たず `knowledge/` に書き込む。

```text
src/
└── main.rs           # CLI エントリポイント

knowledge/             # テスト知識(Test Designer が手動記述、UC1 / UC1b)
└── <requirement>/
    ├── requirement.yml
    └── <feature>/
        ├── feature.yml       # requirement: <requirement> で親を参照
        └── <behavior>/
            ├── behavior.yml
            └── <condition>/
                ├── condition.yml
                └── expected/
                    └── 001.yml, 002.yml, ...

axes/                  # 横断的観点(Axis)のレジストリ(UC1、§3.1)
generated/
└── testcases/
    └── <condition>.yml  # knowledge/ から CI が決定的に再生成(1 Condition = 1 ファイル、UC2 / UC3)

executions/             # マイルストーンごとの実行結果(UC4)

changes/
└── <milestone>.yaml   # マイルストーン間の derived_from(ChangeEvent、UC5 / UC6)

schema/                 # フォーマット・正規化ルール定義(UC7)
tools/                  # 生成・検証スクリプト(UC2 / UC5 / UC6 / UC7)

docs/                  # 設計ドキュメント(論文・運用イメージ・生成アルゴリズム設計)
```

> **既存プロジェクトへの注意**: `knowledge/<requirement>/<feature>/...` への階層追加は破壊的変更です。自動移行コマンドは提供していないため、以前の `knowledge/<feature>/...` 構造で作成済みのプロジェクト(例: todo-test, game-test)は、`<feature>/` ディレクトリ群をまるごと任意の `<requirement>/` ディレクトリ配下に手動で移動し、各 `feature.yml` に `requirement: <requirement-id>` を追記してください。

## Pre-PR チェックリスト

PR を作成する前に以下をすべて満たすこと:

- [ ] 全テストがパス(`cargo test`)
- [ ] Lint エラーゼロ(`cargo clippy --all-targets -- -D warnings`)
- [ ] フォーマット済み(`cargo fmt --check`)
- [ ] `cargo audit` で脆弱性なし
- [ ] `generated/testcases/*.yml` が `knowledge/` の内容と一致(`markharness verify` で再検証)
- [ ] コード・ログ・チャット出力にシークレットが含まれない
