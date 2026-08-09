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
- **2026-08-10 追記(解消)**：上記の「lineage非連携」は、`to_milestone`タグが直接2親のマージコミットを指す場合に限り解消した(`checklist-lineage-integration.md`参照)。`ChangeEvent`に`from_tree_shas: Vec<String>`を加算的フィールドとして追加し、`changes compute`内部で`lineage::classify`を呼び出して`TrueDivergence`のFeatureのみ両親tree SHAを記録する。ただしマイルストーン区間内の任意の位置でのマージ(タグがマージコミットを直接指さない一般ケース)への統合は依然として未対応であり、Threats to Validity・Future Workの記述は「非連携」から「部分統合・範囲限定」に更新した(論文§3.2・§3.6・第6章・第7章)。
- change_typeは事後アノテーション方式。compute時には計算しない(既存コメントの設計意図を維持)。
- schema検証は新規 `markharness validate` コマンド(既存の `knowledge validate <draft_file>` とは別物)。jsonschemaクレートはno-default-featuresで追加(reqwest/wasm系の巨大な依存を回避)。
- id正準化: id_cache.rs の `feature_dir_from_feature_yml_path` はディレクトリ名を返す実装だったが、feature.ymlをパースしてid:フィールドを読むよう変更した。既存のgenerate.rsは既にid:フィールドを使っており、両者の食い違いという潜在バグを解消した。
- 全ステップTDD(Red-Green-Refactor)で実施。ユニットテスト(id_cache/git/changes/schema/validate/lineage)+ CLI統合テスト(tests/changes_cli.rs, tests/validate_cli.rs, tests/lineage_cli.rs)を追加。最終的に `cargo test` は9スイート・225+ユニットテスト含め全緑、`cargo clippy --all-targets -- -D warnings` 警告ゼロ、`cargo fmt --check` 差分ゼロ。
- `cargo audit` は環境側のadvisory DBに重複ID(RUSTSEC-2026-0244)がありエラーで実行不可(このリポジトリ側の問題ではない)。

## Steps (2026-08-10 追記: RQ1未実証問題への対応)
- [x] Step 12: RQ1(複数世代にわたる変更影響識別タスクでの正答率改善)が第5章の評価計画のみで被験者実験が未実施、第8章Conclusionが空欄という指摘(improvement-prompts.md 項目1)に対応する。方針はユーザーとの確認の結果、**方針B(論文の位置づけ変更)**を採用した。理由：被験者実験(群あたり15〜30名、複数専門家による正解データ確認)は本セッションの作業範囲でパイロット的にも代替不能な規模の実証コストを要し、"自己検証で数値を出す"(方針A)は評価設計自体が要求する対照群(組織の実運用)・被験者割当を満たせず、むしろ「弱い自己検証で結果を出した」という誤ったシグナルを生むリスクがあると判断した。
- [x] Step 13: 冒頭の「論文種別」「想定投稿先」を「設計提案＋リファレンス実装のレポート」に修正し、実証的評価が未実施である旨を明記した。
- [x] Step 14: §1.2 RQ1の直後に「RQ1の現状の位置づけ」節を追加し、本文中の「改善する」等の記述が設計上の期待であり被験者実験による実証結果ではないことを明記した。
- [x] Step 15: §5の章題を「Empirical Evaluation Plan(未実施)」に変更し、冒頭に本章が計画であり結果ではない旨の断り書きを追加した。§5.1の「検証する」を「検証することを目的として、以下の評価計画を設計した」に修正した。
- [x] Step 16: §8 Conclusionを執筆した。RQ1への肯定的結論は主張せず、設計・実装(第3〜4章)の到達点を要約した上で、被験者実験による実証をFuture Workとして明記する内容にした。

## Notes(2026-08-10追記)
- 方針Bの適用範囲は「本ドラフトが主張する結論のスコープ」の修正であり、第5章の評価計画自体(タスク層別化・co-changeノイズ除去・対照群設計)は将来実験を実施する際にそのまま使える設計として維持した(改変・削除していない)。
- 他に「誇大な主張」の候補として§1.1のモデル構造上の説明(「原理的に答えられない問い」等)を洗い出したが、これらは版履歴DAGという設計そのものの性質を述べているのであり、被験者実験の結果を先取りした断定ではないと判断し、変更しなかった。過剰断定の是正は「実証結果として書かれている箇所」に絞った。

## Steps (2026-08-10 追記: canonicalization_rule_version/id_index_schema_versionの改訂シナリオ検証)
- [x] Step 17: `src/id_cache.rs`に`canonicalization_rule_version`・`id_index_schema_version`それぞれの不一致時にキャッシュが再計算されることを検証するテストを追加した(詳細: `checklist-cache-version-revalidation.md`)。
- [x] Step 18: 検証の結果、実装は既にキャッシュキー全体の等値比較で4フィールド全てを同一ロジックで扱っており、設計意図(§3.3: 不一致なら静かに再計算)と一致していたため、コード修正は不要だった。

## Summary
論文§3.6/§7で「未実装」「設計から簡略化」と整理されていた6項目のうち、TMSインポータ(UC8)を除く5項目(merge-base祖先探索監査コマンド・change_typeフィールドと事後アノテーション・schema/JSON Schema検証・idキャッシュの内容アドレス方式キー化・idのパス非依存化)をTDDで実装し、論文本文とdocs/cli-manual.mdの実装状況注記を更新した。
