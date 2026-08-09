# Task: 論文「テスト知識管理のGit-nativeモデル」の未実装部分の実装
Created: 2026-08-10

対象: docs/テスト知識管理のGit-nativeモデル_統合版V2.md §3.6/§7 に記載された5項目
(TMSインポータ・UC8は対象外、Future Workのまま)

## Steps
- [x] Step 1: id_cache.rs を「id=ディレクトリ名」から「id=feature.ymlのid:フィールド」に統一する(§3.3のid path非依存化の最小実装)。TDD。
- [x] Step 2: id重複検出(同じidを持つ複数のFeatureディレクトリが存在する場合はエラー)を追加する。TDD。
- [x] Step 3: id_cache.rs のキャッシュキーを内容アドレス方式(tree_sha + canonicalization_rule_version + id_index_schema_version + tool_version)に変更し、読み込み時に不一致なら自動再計算する(§3.3)。TDD。
- [x] Step 4: ChangeEvent に change_type: Option<ChangeType> フィールドを追加する(ChangeType = SpecChange | BugFix | Refactor | Other、snake_caseシリアライズ)。compute_changesはNoneのまま生成。TDD。
- [x] Step 5: `markharness changes annotate <event_id> --type <value>` コマンドを追加し、changes/*.yaml を横断検索してイベントのchange_typeを書き換える。TDD + CLI統合テスト。
- [x] Step 6: jsonschema クレートを追加し、schema/*.json (requirement/feature/behavior/condition/expected/axes) を定義する。
- [x] Step 7: `markharness validate` コマンドを追加し、knowledge/ 配下を全走査してJSON Schemaで構造検証する。
- [x] Step 8: validate に axis タグ整合性(axes/*.ymlに存在しない値を拒否)・forked_from参照先の存在チェックをRust側クロスリファレンスとして追加する。
- [x] Step 9: git.rs に merge_base / parents (2親取得) のgitラッパー関数を追加する。TDD。
- [x] Step 10: `markharness changes lineage --commit <sha>` コマンドを新設し、§3.2の場合分け(線形/真の分岐/1親)を全id分判定して出力する。監査用途、changes/*.yamlには書き込まない。TDD + CLI統合テスト。
- [x] Step 11: 全体を通して `cargo test` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt` を実行し、docs/テスト知識管理のGit-nativeモデル_統合版V2.md の §3.6 実装状況まとめ表・各節の実装注記を更新する。docs/cli-manual.md にも新規3コマンドの節を追加した。

## Notes
- ユーザーとのgrilling結果: 5項目を今回スコープに含める。TMSインポータ・UC8は対象外。
- merge-base監査コマンドは changes compute とは独立した副次機能。永続データ(changes/*.yaml)には影響しない。実装後もこの非連携はThreats to Validity・Future Workに明記のまま。
- change_typeは事後アノテーション方式。compute時には計算しない(既存コメントの設計意図を維持)。
- schema検証は新規 `markharness validate` コマンド(既存の `knowledge validate <draft_file>` とは別物)。jsonschemaクレートはno-default-featuresで追加(reqwest/wasm系の巨大な依存を回避)。
- id正準化: id_cache.rs の `feature_dir_from_feature_yml_path` はディレクトリ名を返す実装だったが、feature.ymlをパースしてid:フィールドを読むよう変更した。既存のgenerate.rsは既にid:フィールドを使っており、両者の食い違いという潜在バグを解消した。
- 全ステップTDD(Red-Green-Refactor)で実施。ユニットテスト(id_cache/git/changes/schema/validate/lineage)+ CLI統合テスト(tests/changes_cli.rs, tests/validate_cli.rs, tests/lineage_cli.rs)を追加。最終的に `cargo test` は9スイート・225+ユニットテスト含め全緑、`cargo clippy --all-targets -- -D warnings` 警告ゼロ、`cargo fmt --check` 差分ゼロ。
- `cargo audit` は環境側のadvisory DBに重複ID(RUSTSEC-2026-0244)がありエラーで実行不可(このリポジトリ側の問題ではない)。

## Summary
論文§3.6/§7で「未実装」「設計から簡略化」と整理されていた6項目のうち、TMSインポータ(UC8)を除く5項目(merge-base祖先探索監査コマンド・change_typeフィールドと事後アノテーション・schema/JSON Schema検証・idキャッシュの内容アドレス方式キー化・idのパス非依存化)をTDDで実装し、論文本文とdocs/cli-manual.mdの実装状況注記を更新した。
