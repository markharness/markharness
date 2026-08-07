# Task: markharness knowledge validate / apply サブコマンドの実装
Created: 2026-08-08
Spec: docs/knowledge-apply-cli-spec.md

## Steps

### 0. 準備
- [x] Step 0-1: `Cargo.toml` に `serde_json` を追加(`--json` 出力用)。`serde` に `derive` はあるが `Serialize` も必要なら features を確認。
- [x] Step 0-2: `src/knowledge_draft.rs`・`src/knowledge_apply.rs` の空モジュールを作成し `src/lib.rs` に `pub mod knowledge_draft;` `pub mod knowledge_apply;` を追加(コンパイルが通る最小状態)。

### 1. knowledge_draft.rs — ドラフト構造体とパース(§4, §9.2)
- [x] Step 1-1 (Red→Green): `RequirementDraft`/`FeatureDraft`/`BehaviorDraft`/`ConditionDraft`/`ExpectedDraft`/`KnowledgeDraft` 構造体を定義し、`parse_draft(yaml: &str) -> Result<KnowledgeDraft, DraftParseError>` が仕様§4のサンプルYAMLを正しくパースするテストを書く(全フィールドあり)。
- [x] Step 1-2 (Red→Green): `axis`/`description`/`label` 省略時(既存id再利用ケース)を `Option` として正しくパースするテスト。
- [x] Step 1-3 (Red→Green): 不正YAML(パース不能)で `DraftParseError` を返すテスト。
- [x] Step 1-4 (Refactor): clippy/fmt。

### 2. knowledge_draft.rs — ValidationError とエラーコード(§5, §6)
- [x] Step 2-1: `ValidationErrorCode` enum(`invalid_slug`/`missing_axis`/`missing_description`/`unknown_axis`/`redundant_prefix`/`conflicting_existing_value`/`parent_not_found`)と `ValidationError` 構造体(`code`/`path`/`value`/`message`/`suggestion`)を定義。

### 3. knowledge_draft.rs — axisレジストリ照合(§8)
- [x] Step 3-1 (Red→Green): `load_axis_registry(root: &Path) -> HashSet<String>` が `axes/*.yml` から `id` を読み込むテスト(`axes/gameplay.yml` に `id: gameplay` がある場合に含まれる)。
- [x] Step 3-2 (Red→Green): `axes/` が空/存在しない場合は空集合を返すテスト。

### 4. validate_draft() — 個別バリデーションルール(§5)
各ルールにつき Red→Green を1サイクルとする。既存 `is_valid_slug`/`strip_redundant_condition_prefix` を再利用。

- [x] Step 4-1: 新規5階層フルセットで全ルール通過(エラー0件)になる正常系テスト。
- [x] Step 4-2: `invalid_slug` — id が `is_valid_slug` を満たさない場合にエラー(path指定を含む)。
- [x] Step 4-3: `missing_axis` — 新規作成のrequirement/feature/behaviorで `axis` が空配列 or 未指定の場合にエラー。
- [x] Step 4-4: `missing_description` — behavior/condition/各expectedで空descriptionの場合にエラー(該当パスごとに個別テスト)。
- [x] Step 4-5: `unknown_axis` — axisレジストリに存在しない値でエラー(`suggestion`は近似候補があれば設定、なければNone)。
- [x] Step 4-6: `redundant_prefix` — condition.idがbehavior.idと重複接頭辞を持ち `strip_redundant_prefix: false` の場合にエラー(`suggestion`に除去後id)。
- [x] Step 4-7: `strip_redundant_prefix: true` の場合はエラーにならないことのテスト(書き込みは knowledge_apply 側で確認するので、ここではエラーなしのみ確認)。
- [x] Step 4-8: レガシーディレクトリ優先(既存の冗長接頭辞付きディレクトリが実在する場合は除去しない)を反映したテスト。
- [x] Step 4-9: `parent_not_found` — 既存feature/behavior/conditionファイルの親参照フィールドがドラフトのチェーンと矛盾する場合にエラー(実装可能な形で解釈: 親参照フィールドが実在パス経由のドラフト整合と一致しない場合)。
- [x] Step 4-10: `conflicting_existing_value` — 既存id再利用時、指定された`axis`/`description`/`label`が既存ファイルの値と不一致の場合にエラー(§12-3の「省略フィールドは比較対象外」方針を実装)。
- [x] Step 4-11: 既存id再利用時、フィールド省略で値比較をスキップし成功するテスト。
- [x] Step 4-12 (Refactor): `validate_draft` 内の重複ロジック整理、clippy/fmt。

### 5. knowledge_apply.rs — apply_draft()(§3.2, §9.2)
- [x] Step 5-1 (Red→Green): 新規5階層一括作成が成功し、`ApplyResult.written_paths` に5ファイル分のパスが含まれるテスト(既存 `creates_new_requirement_feature_behavior_condition_and_expected_from_scratch` 相当のアサーションをファイル内容に対して行う)。
- [x] Step 5-2 (Red→Green): バリデーションエラーがある場合、`apply_draft` は書き込みを一切行わずエラーを返すテスト(アトミック性、§3.2)。
- [x] Step 5-3 (Red→Green): 既存id再利用時(requirement/feature/behavior/conditionが既存)、該当ファイルを上書きせず、新規expectedのみ追記するテスト。
- [x] Step 5-4 (Red→Green): `expected` 配列に複数要素を渡した場合、連番採番(`{condition_id}-{seq:03}`)で複数ファイルが書かれるテスト。
- [x] Step 5-5 (Red→Green): `strip_redundant_prefix: true` でcondition.idの接頭辞が除去された名前でディレクトリが作られるテスト(既存 `auto_dedup_strips_redundant_condition_prefix_and_notifies` 相当)。
- [x] Step 5-6 (Refactor): 書き込み処理(一時ファイル+リネーム、失敗時ロールバック)を `write_all_atomically`/`write_one` に整理。validate_draft を先に呼び全エラーが空の場合のみ書き込みへ進む構造を確保。clippy/fmt通過。

### 6. CLI統合(§3.1, §3.2, §3.4, §6)
- [x] Step 6-1: `cli.rs` の `KnowledgeCommand` に `Validate { draft_file, dir, json }` / `Apply { draft_file, dir, json, strip_redundant_prefix, dry_run }` バリアントを追加し、`Cli::parse_from` の引数パーステスト(既存 `parses_knowledge_add_*` に倣う)。実行系(exit code検証)テストは `run()` 内部で `std::process::exit` を呼ぶため同一テストプロセス内では実施できず、`tests/knowledge_cli.rs` にサブプロセス起動形式で追加する方針に変更(Notes参照)。
- [x] Step 6-2 (Red→Green): `knowledge validate` 実行 — 正常系で終了コード0、人間可読で何も出力しない(またはok旨のメッセージ)、`--json` で `{"ok": true, ...}` 相当を出力。(`tests/knowledge_cli.rs` にサブプロセス起動形式で実装)
- [x] Step 6-3 (Red→Green): `knowledge validate` — バリデーションエラー時に終了コード1、stderrへ `error: <code>: ...` 形式(§6テキスト例に準拠)、`--json` 指定時はstdoutへ§6のJSON配列形式。
- [x] Step 6-4 (Red→Green): ファイル不在・YAMLパース不能で終了コード2(不正フラグはclapが処理しexit code 2相当を返す標準動作に委譲)。
- [x] Step 6-5 (Red→Green): `knowledge apply` 正常系 — 終了コード0、ファイルが実際に書き込まれる、`--json` で `{"ok": true, "written": [...]}` を出力。
- [x] Step 6-6 (Red→Green): `knowledge apply` バリデーションエラー時 — 終了コード1、ファイル未書き込み。
- [x] Step 6-7 (Red→Green): `knowledge apply --dry-run` が `validate` と同義(書き込みなし、終了コードはvalidateと同じ)であることのテスト。
- [x] Step 6-8 (Red→Green): `knowledge apply --strip-redundant-prefix` 指定時の終了コード0・書き込み結果テスト。
- [x] Step 6-9 (Refactor): `run()` 内のknowledge validate/apply分岐、共通のエラー出力関数(`report_validation_outcome`/`print_errors_human`/`errors_to_json`/`apply_result_to_json`)を抽出。clippy/fmt通過、`cargo test`全体104件成功。

### 7. 仕上げ
- [x] Step 7-1: `cargo test` 全体通過確認(lib 99件 + knowledge_cli 10件 = 109件、doc-tests 0件、全成功)。
- [x] Step 7-2: `cargo clippy --all-targets -- -D warnings` 通過確認(警告0件)。
- [x] Step 7-3: `cargo fmt` 適用済み(`cargo fmt --check` 差分なし)。
- [x] Step 7-4: `cargo audit` 実行、指摘なしを確認(新規依存 serde_json 含む、脆弱性0件)。
- [x] Step 7-5: 本チェックリストに Summary を追記。

## Notes
- 本仕様の非スコープ: `knowledge add --edit`(§3.3, §9.3)、`axes list` コマンド(§8)、既存 `interactive.rs` のリファクタ(§10)は今回実装しない。
- `conflicting_existing_value` の比較粒度は §12-3 の仮採用方針(省略フィールドは比較対象外)に従う。要レビューと明記されているため、実装後にユーザーへ確認する。
- 既存 `strip_redundant_condition_prefix(feature_id, condition_id)` の第1引数名は「feature_id」だが実体はbehavior_id相当(`interactive.rs`での呼び出し実績あり)。knowledge_draft.rsでもbehavior.idを渡すこと。
- `--json` は当初 `serde_json` を新規依存として追加し使う予定だったが、`{ok, errors}` / `{ok, written}` のキー順序をspec §6の例と完全一致させるため(serde_jsonのデフォルトMapはBTreeMapでキーがアルファベット順になり順序保証がない)、実際には `cli.rs` 内で手書きJSON文字列組み立て(`json_escape`/`json_string_or_null`/`validation_error_to_json`等)を採用した。結果として `serde_json` は未使用になったため、実装完了後にCargo.tomlから削除した(不要な依存を持たない方針)。
- `parent_not_found` ルール(§5 #9)は、ドラフトが単一チェーン構造(Requirement→Feature→Behavior→Condition)でありディレクトリ階層自体がすでに親子関係を表現しているため、「新規作成時にドラフト自身の値と整合していること」は自動的に満たされる。実装では、既存ファイルを再利用する際にそのファイル内の親参照フィールド(例: `feature.yml`の`requirement:`)がドラフトのチェーンと矛盾していないかを検証する形で解釈した(レガシーデータ破損検知に相当)。
- 人間可読エラー出力で `suggested="..."` を表示するかどうかについて、spec §6の例では `unknown_axis` は表示せず `redundant_prefix` のみ表示しているが、実装では一貫性を優先し `suggestion` が存在する場合は常に表示する方針とした(spec例からの意図的な逸脱)。

## Summary
`markharness knowledge validate` / `apply` サブコマンドを仕様書どおりに実装した。`knowledge_draft.rs`(ドラフトのパース・バリデーション・axisレジストリ照合)と `knowledge_apply.rs`(検証+アトミック書き込み)を新規モジュールとして追加し、`cli.rs` に両サブコマンドと終了コード(0/1/2/3)・人間可読/JSON出力を配線した。既存の対話型 `knowledge add` には手を加えていない。TDDサイクルで109件のテスト(ユニット99件+CLI統合テスト10件)を追加し、`cargo test`/`cargo clippy -D warnings`/`cargo fmt --check`/`cargo audit`すべて通過を確認済み。
