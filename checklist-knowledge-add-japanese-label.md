# Task: knowledge add に日本語ラベル入力→ローマ字id自動変換を追加
Created: 2026-08-07

## 設計まとめ(grillingセッションで合意した内容)

- スコープ: 既存の Feature/Condition/ExpectedResult 3階層(`knowledge add` = `run_add`)の改善に限定。Requirement/Behavior階層・`testcase-generation-design.md`の生成アルゴリズムは対象外。
- 対象コマンド: 既存 `markharness knowledge add`(`src/interactive.rs` の `run_add`)を改修。新規コマンドは作らない。
- 日本語検出: 入力文字列がASCII以外を含むかどうかで判定(厳密な文字種判定はしない)。
- 変換: `kakasi` クレートで日本語ラベル→ローマ字変換 → 1案のみ提示 → ユーザーが自由編集(Enterでそのまま採用) → 自動正規化(小文字化・空白をハイフン化・非対応記号除去) → `is_valid_slug` で検証。
- 衝突時: 正規化後idが既存候補(`list_candidate_ids`)と一致したら警告し、再入力を促す(自動流用はしない)。既存の「番号選択による意図的な再利用」とは別扱い。
- スキーマ: `Feature` / `Condition` / `ExpectedResult` の3構造体すべてに `label: Option<String>` を新設(既存YAMLとの後方互換のため型としては任意、`#[serde(default)]`)。ただし対話フローで日本語ラベル入力を経由した新規作成分は必ず label をYAMLに保存する(捨てない)。既存データは無改修。
- 対象フィールド: id入力を対話で行うのは Feature id と Condition id の2箇所のみ(ExpectedResult id は既存通り自動連番のため対象外)。`ExpectedResult.label` はスキーマ上は追加するが、今回の対話フローからは埋めない(将来の拡張余地として保持するのみ)。
- 生成アルゴリズム(`generate.rs` / `testcase-generation-design.md`)は変更しない。`label` は knowledge 記述・保存のみに使う。
- 依存クレート: `kakasi`(pykakasi移植、漢字→ローマ字対応)を新規追加。

## 実行環境についての注記

このチェックリストを作成したセッションのサンドボックスにはRustツールチェインが無く、`cargo test` / `cargo clippy` / `cargo fmt` / `cargo audit` を実行できない。**実装(Red-Green-Refactorの実行)は cargo が使える環境で行うこと。** 各ステップは TDD(`tdd-workflow.instructions.md`)に従い、必ず Red(失敗確認)→ Green(最小実装)→ Refactor の順で進める。

## Steps

- [x] Step 1 (Red/Green): `Cargo.toml` に `kakasi` 依存を追加し、`cargo build` が通ることを確認(依存追加のみなのでテストは無し、ビルド確認のみ)
- [x] Step 2 (Red/Green): `src/knowledge.rs` に `contains_non_ascii(s: &str) -> bool` を追加。テスト: ASCII文字列で `false`、日本語混入で `true`
- [x] Step 3 (Red/Green): `src/knowledge.rs` に `romanize_label(japanese: &str) -> String`(`kakasi` を使いローマ字文字列を返す)を追加。テスト: 既知の日本語入力→期待するローマ字出力(kakasiの実出力に合わせてテスト値を確定させる。曖昧な読みがある単語は避け、確実な変換例を使う)
- [x] Step 4 (Red/Green): `src/knowledge.rs` に `normalize_slug_candidate(raw: &str) -> String`(小文字化・空白→ハイフン・連続ハイフン圧縮・先頭/末尾ハイフン除去・`is_valid_slug` が許可しない文字の除去)を追加。テスト: スペース区切り入力、大文字混在、記号混入の3ケース
- [x] Step 5 (Red/Green): `Feature` / `Condition` / `ExpectedResult` 構造体に `label: Option<String>`(`#[serde(default)]`)を追加。既存の `parses_feature_yaml` 等の既存テスト(labelフィールドなしYAML)が壊れないことを確認(回帰)
- [x] Step 6 (Red/Green): `serialize_feature` / `serialize_condition` / `serialize_expected_result` を、`label` が `Some` のときだけ `label: <値>` 行を出力するよう変更。テスト: label ありケースの出力文字列、label なしケース(既存出力のまま)の2パターン
- [x] Step 7 (Red/Green): `src/interactive.rs` に `prompt_id_or_label` (仮称)を新設。非ASCII入力を検知した場合に `romanize_label` → `normalize_slug_candidate` の候補を提示し、空Enterで採用・非空入力で編集を受け付ける挙動をテスト(reader/writerのCursorモックで)
- [x] Step 8 (Red/Green): `prompt_id_or_label` に衝突検知を追加。正規化後idが既存候補一覧(`list_candidate_ids`)に含まれる場合、警告メッセージを出し再入力ループに戻ることをテスト
- [x] Step 9 (Red/Green): `prompt_id_or_label` の返り値を `(id: String, label: Option<String>)` とし、ASCII直接入力(既存フロー)では `label = None` を返すことを回帰テストで確認
- [x] Step 10 (Red/Green): `run_add` の Feature id 取得部分を `prompt_slug` から `prompt_id_or_label` に置き換え、新規Feature作成時に `label` をYAMLへ保存することをテスト(日本語ラベル入力→id自動生成→保存されたfeature.yamlにlabel行がある)
- [x] Step 11 (Red/Green): `run_add` の Condition id 取得部分も同様に置き換え、新規Condition作成時に `label` を保存することをテスト
- [x] Step 12 (Red/Green): 既存Feature/Conditionを番号選択で再利用する場合(数値入力)は `label` を問わず `None` になり、既存ファイルが上書きされないことを回帰テスト
- [x] Step 13 (Refactor): `cargo clippy --all-targets -- -D warnings` / `cargo fmt` を実行し指摘を解消
- [x] Step 14: `cargo test` で全件通過(既存38件 + 新規分)を確認
- [x] Step 15: `cargo audit` を実行し、`kakasi` 追加による脆弱性が無いことを確認
- [x] Step 16: `docs/cli-manual.md` に日本語ラベル入力フロー・衝突時の挙動・バリデーション規則を追記
- [x] Step 17: `cargo run -- knowledge add --dir tmp/manual-check-ja` で実際に対話実行し、(1) 日本語ラベル入力→id候補提示→編集確認→保存、(2) 衝突時の警告表示、(3) 既存ASCII id直接入力フローに回帰が無いこと、を目視確認。確認後 `tmp/manual-check-ja` を削除

## Notes

- `label` フィールドの必須性について: 型としては `Option<String>` だが、これは既存YAML(labelフィールドを持たない)の後方互換パースのための制約。対話フローで日本語ラベルを経由した新規作成では label を必ず保存する(=実質必須)というのがgrillingセッションでの合意。ASCII直接入力(既存フロー踏襲分)では label を要求しない。
- `ExpectedResult.label` はスキーマとしては追加するが、ExpectedResult id自体は自動連番のため対話フローからは埋めない。将来Behavior/Requirement階層拡張時の再検討対象。
- `kakasi` クレートの実際の変換出力(大文字小文字・区切り文字の有無)を確認してから Step 3/4 のテスト期待値を確定させること(ドキュメントだけで断定せず、実際に動かして確認する)。
- 衝突検知(Step 8)は「意図的な番号選択による再利用」とは別ロジックであることに注意。番号選択はこれまで通り即座に既存id再利用として扱う。
