# 0032: CLIリードモデルというseamを導入する

## ステータス

Accepted(2026-09-16決定)。

## 背景

`markharness` v2は、Change ImpactとRelease Coverageのビューやダッシュボードを別リポジトリ`markharness-view`(仮)に委ねる方針である。この方針を実現するため、`docs/ja/design/cli-read-model-design.md`(Draft)がCLIの外部向け読み取り契約(リードモデル)を、新設の`record_kind`/`schema_version`エンベロープと`CommandOutcome`への新variant追加を前提に提案した。

grillingセッションで実装(`src/`)を調べたところ、この前提は誤りだった。実際には次の2つの独立した出力系統が既に存在する。

1. **`CommandOutcome`/`Presenter`系統**(`src/presentation.rs`): `canonical_imported`・`generated`・`changes_computed`(`markharness changes compute`が`.markharness/changes/`へ書き込む処理結果)の3つの**書込み系**コマンドの結果を表す。`JsonPresenter`は`{"schema_version": 1, "outcome": "<tag>", ...フィールド}`というフラットな形式で、v0.6.1から出力している。
2. **各コマンド専用モジュールの系統**(`src/impact.rs`・`src/coverage.rs`): `impact`・`coverage`という**読み取り系**コマンドは、`CommandOutcome`/`Presenter`を一切経由しない。各モジュールが自前の構造体(`ChangeImpact`・`ReleaseCoverage`)を持ち、`schema_version`・`record_kind`(値はそれぞれ`"change_impact"`・`"release_coverage"`)フィールドを既に備え、`cli.rs`から`serde_json::to_string_pretty`で直接シリアライズしている。`--format`は現状`Json`しか値がなく、人間可読出力は未実装である。

つまり、ドラフトが提案しようとした「`record_kind`/`schema_version`を持つリードモデルを、Application層で生成してJSON Presenterへ渡す」という設計は、`impact`/`coverage`について**既に実装済み**である。また`record_kind`という名前自体も、[0025](0025-v2-forward-compatible-evolution.md)と`markharness-v2-design.md`§9.1が`.markharness/`配下の永続レコード向けに確立し、`impact`/`coverage`のCLI出力もそれを踏襲している既存の命名規約であり、ドラフトが暗黙に想定していた新規の入れ子構造やフィールド名でもない。

新規に必要なのは、この既存パターンに従う`traceability`コマンド(現状は未実装)を1つ追加することだけである。

さらに、`src/traceability.rs`というファイル自体は既に存在しており、`TraceabilityIndex`/`build_index`/`serialize_index`を実装している。これは`generate`が生成時に組み立て、`.markharness/generated/traceability-index.json`へ書き込む**生成アーティファクト**(`application.rs`)であり、`verify`が再生成の決定性を確認するために使う(`verify.rs`)。TestCase単位のフラットな一覧(`case_id`・`requirement_ids`・`feature`・`behavior`・`scenario`・`axis`)で、`record_kind`/`schema_version`を持たず、`markharness-view`が読む対象でもない。新設する`TraceabilityReadModel`(Requirement/Feature/Behavior/Scenario/TestCaseの各Node配列とRelationを持つ、`--at`指定時にオンデマンドで計算されCLI標準出力へ返す読み取り専用モデル)とは、生成物か問い合わせ結果かという点も含め責務が異なる。同じファイル名を異なる責務のために使い回すと、どちらのモジュールを指しているか読み手が混乱するため、本ADRで名前の衝突を解消する。

## 決定

### 1. `traceability`は`impact`/`coverage`と同じ出力パターンに従う

新設する`traceability`コマンドは、`CommandOutcome`/`Presenter`を経由しない。`impact.rs`・`coverage.rs`と同様、専用モジュールに`TraceabilityReadModel`構造体を定義し、`schema_version: u32`・`record_kind: &'static str`(値は`"traceability"`)を持たせ、`cli.rs`から`serde_json`で直接シリアライズする。

`CommandOutcome`への新variant追加(`ImpactComputed`・`CoverageComputed`・`TraceabilityComputed`)は行わない。`impact`・`coverage`は元から`CommandOutcome`を使っておらず、`traceability`もそれに合わせる。

### 2. 既存の`src/traceability.rs`は`src/traceability_index.rs`へリネームし、`src/traceability.rs`を新設CLIコマンド用にする

`TraceabilityIndex`/`build_index`/`serialize_index`(`generate`の生成アーティファクト用)は、名前を`src/traceability_index.rs`へ変更する。呼び出し元(`application.rs`・`verify.rs`)の`use`と参照パスを更新する。型名・関数名・`.markharness/generated/traceability-index.json`というファイル名自体は変更しない(生成アーティファクトのファイル名はモジュール名と独立した既存の外部仕様であり、変更する理由がない)。

空いた`src/traceability.rs`を、新設CLIコマンド`traceability`(`TraceabilityReadModel`)専用のモジュールとする。これにより、`impact.rs`↔`impact`コマンド、`coverage.rs`↔`coverage`コマンドと同じ「モジュール名がコマンド名と一致する」命名パターンを`traceability`でも維持できる。

このリネームは、`markharness::traceability::TraceabilityIndex`という公開Rust APIパス(`src/lib.rs`は`pub mod traceability;`を含め全モジュールを`pub`にしている)を`markharness::traceability_index::TraceabilityIndex`へ破壊的に変更する。本クレートの`pub mod`群は`tests/`配下の統合テストからクレート内部へアクセスするためのものであり、外部のライブラリ利用者への安定APIとして提供・保証しているものではない(`Cargo.toml`の`description`は"Git-native test knowledge management CLI"であり、ライブラリとしての利用は想定外)。したがって、再エクスポート(`pub use`)による旧パスの互換維持は行わない。CLAUDE.mdの「後方互換のための設計を持ち込まない」方針をRust公開APIにも適用する。

### 3. `impact`・`coverage`の出力契約は変更しない

`ChangeImpact`・`ReleaseCoverage`は、`record_kind`/`schema_version`を含め、既に外部契約として妥当な形になっている。ドラフトが提案していた`ChangeImpactReadModel`・`ReleaseCoverageReadModel`は、既存の`ChangeImpact`・`ReleaseCoverage`を指すものとして読み替える。既存フィールドの破壊的変更は、本ADRの範囲では行わない。

### 4. `CommandOutcome`/`Presenter`系統(`outcome`フィールド)はリードモデルの対象外とする

`canonical_imported`・`generated`・`changes_computed`は書込み系コマンドの実行結果であり、`markharness-view`が読み取り対象とする`traceability`/`impact`/`coverage`とは無関係である。これらの`outcome`フィールドを`record_kind`へ統一する変更は、具体的な必要性がないため本ADRのスコープに含めない(YAGNI)。将来、書込み系コマンドの出力を外部契約として整理する具体的な必要が生じた場合は、別のADRで扱う。

### 5. 本ADRのスコープは「`traceability`を既存の読み取り系パターンに合わせて追加する」という決定のみとする

`TraceabilityReadModel`の詳細なフィールド構成(Node種別ごとの配列、Relationの表現等)は、本ADRでは決定事項として扱わず、`docs/ja/design/cli-read-model-design.md`(設計doc)の責務として残す。

### 6. 本ADRは設計決定の記録であり、実装は別途行う

`src/traceability.rs`のリネームと新規実装、`cli.rs`への`traceability`コマンド追加は、本ADRの範囲に含めない。別タスク(実装フェーズ)として、TDD(Red-Green-Refactor)で行う。

## 影響範囲

- 既存の`src/traceability.rs`(`TraceabilityIndex`/`build_index`/`serialize_index`)を`src/traceability_index.rs`へリネーム。型名・関数名・出力ファイル名`.markharness/generated/traceability-index.json`は変更しない
- `src/application.rs`・`src/verify.rs`: 上記リネームに伴う`use crate::traceability::...`の参照パス更新
- `src/lib.rs`: `pub mod traceability;`を`pub mod traceability_index;`へ変更し、新設の`traceability`コマンド用モジュールを`pub mod traceability;`として追加する。公開Rustモジュールパスの破壊的変更であり、再エクスポートによる互換維持は行わない(決定2参照)
- リネームで空いた`src/traceability.rs`(新規): 新設CLIコマンド`traceability`用の`TraceabilityReadModel`構造体、生成ロジック
- `src/cli.rs`: `traceability`コマンドの追加(`impact`/`coverage`と同じ、直接`serde_json`でシリアライズする配線)
- `docs/ja/design/cli-read-model-design.md`・`docs/en/design/cli-read-model-design.md`: 既存の`impact`/`coverage`の実装状況、`CommandOutcome`とは無関係であること、`src/traceability.rs`のリネームの明記。**この2ファイルのみ、本ADRの一部として既に更新済み**(いずれもまだgit未追跡の新規ファイル)。

**未着手(実装フェーズで行う)**: 以下の既存の追跡済みドキュメントは、本ADRの時点では`src/traceability.rs`への言及を意図的にそのままにしてある。理由は、決定2のリネームがまだコードに反映されておらず(`src/traceability.rs`は現時点でも`TraceabilityIndex`のままである)、今ここで`src/traceability_index.rs`と書き換えると、まだ存在しないファイルを指す誤った記述になるためである。実際のRustリネーム(決定6)が実装された時点で、同じコミット内でこれらも更新する。

- `docs/ja/design/markharness-v2-design.md`・`docs/en/design/markharness-v2-design.md`(§5.1・実装状況表)
- `docs/ja/design/testcase-generation-design.md`・`docs/en/design/testcase-generation-design.md`(冒頭のStatus行、§3.4)
- `docs/ja/cli-manual.md`・`docs/en/cli-manual.md`(§3 動作確認・テスト、`cargo test`対象モジュール一覧。新設`traceability`コマンドの説明追加も同時に行う)
- **変更不要**: `src/presentation.rs`(`CommandOutcome`・`Presenter`)、`src/impact.rs`・`src/coverage.rs`の出力構造体、`.markharness/generated/traceability-index.json`のファイル形式・内容
- `schema/`配下(将来、`schema/traceability-read-model.schema.json`等を追加する際は、`impact`/`coverage`が既に使っている`record_kind`命名に従う)

## 検討したが採用しない選択肢

- **`CommandOutcome`に`ImpactComputed`/`CoverageComputed`/`TraceabilityComputed`を追加する**: 当初のドラフトの前提。実装を確認した結果、`impact`/`coverage`はそもそも`CommandOutcome`を使っておらず、この前提自体が誤りだった。既に動いている独立した出力系統をわざわざ`CommandOutcome`へ統合する具体的な理由もない。
- **`canonical_imported`/`generated`/`changes_computed`の`outcome`フィールドも`record_kind`へリネームする**: 全コマンドの命名を統一する案として一度検討した。しかしこれらは`markharness-view`が読む対象ではなく、リネームの具体的な動機がない(YAGNI)。書込み系コマンドの出力契約を将来整理する必要が生じた場合、別ADRで扱う。
- **`record_kind`/`schema_version`を新しい入れ子構造にする**: `impact`/`coverage`が既に採用しているフラットな構造と異なる形式を新設する理由がない。
- **`TraceabilityReadModel`を既存の`src/traceability.rs`(`TraceabilityIndex`)と同じファイルに共存させる**: ファイル数は増えないが、「`generate`が書き込む生成アーティファクト」と「`markharness-view`向けにオンデマンドで計算する読み取り専用モデル」という異なる責務・異なるライフサイクルが1ファイルに混在し、どちらを指しているか読み手が混乱する。
- **新設のCLI読み取りモデル側を別名(例: `traceability_read_model.rs`)にし、既存の`TraceabilityIndex`側の名前`src/traceability.rs`を維持する**: `impact.rs`↔`impact`コマンド、`coverage.rs`↔`coverage`コマンドという既存の命名対称性が`traceability`だけ崩れる。既存の`TraceabilityIndex`は`generate`内部の一部品であり、`traceability`というコマンド名を代表するものではないため、代表権は新設コマンド側に譲るのが自然。
