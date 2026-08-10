# markharness 改善指示プロンプト集

前回のレビュー（ツール評価・論文評価）で挙がった改善提案を、AIエージェント（Claude Code等）に実行させるためのプロンプト。各項目は独立して投入可能。本リポジトリの `CLAUDE.md` のルール（チェックリスト運用・TDD・破壊的コマンドの事前確認）に必ず従うこと。

項目7〜9は [review-data-model-improvement-proposals.md](./review-data-model-improvement-proposals.md)（外部の「データモデル分析レポート」の改善案5.1〜5.5を実装・論文と照合したレビュー）の結論から起こしたもの。同レビューで却下と判定した項目（5.1・5.2のchildrenフィールド版・5.4のpriority/status）は実行プロンプト化せず、項目9の後の「却下ログ」にのみ記録している。

**進捗メモ**：本ファイルの一部項目は、当時の指摘後に既に実装・論文反映が完了している。着手前に必ず現状を確認すること（各項目冒頭の状況注記を参照）。項目1・4・5は完了、項目2は部分完了、項目3・6は未確認。

---

## 共通の前提コンテキスト（全プロンプトの先頭に付ける）

```
このリポジトリ (markharness) は「テスト知識管理のGit-nativeモデル」という論文（docs/テスト知識管理のGit-nativeモデル_統合版.md）の実装です。
別リポジトリ mh-sample-test-case はその検証用サンプルデータです。
作業前に CLAUDE.md と PROJECT.md を必ず読み、チェックリスト運用・TDD（Red-Green-Refactor）・破壊的コマンドの事前確認ルールに従ってください。
既存のレビューで以下の課題が指摘されています。今回はその中の1項目に取り組みます。
```

---

## 1. 【優先度: 高】RQ1未実証問題への対応

**状況(2026-08時点)**：完了(方針B)。論文冒頭「論文種別」欄・第1章・第8章 Conclusion は既に「設計提案＋リファレンス実装のレポート」「RQ1の検証は未実施のFuture Work」と明記されており、正答率改善を既定路線として断定する記述はない。以下のプロンプトは経緯の記録として残す(再実行不要)。

```
論文 docs/テスト知識管理のGit-nativeモデル_統合版.md の RQ1（複数世代にわたる変更影響識別タスクでの正答率改善）は、
第5章に評価計画があるものの被験者実験が未実施で、第8章 Conclusion が空欄のままです。

以下のいずれかの方針で対応してください。方針の選択理由も含めて checklist-paper-gaps.md に追記すること。

方針A: パイロット評価を実施する
- 第5章の評価計画のうち、被験者を集めずに実施可能な「層別化タスクの設計」「co-changeノイズ除去ロジック」だけでも
  mh-sample-test-case を使って自己検証し、結果を docs/ 配下に新規ファイル（例: pilot-evaluation-results.md）としてまとめる。
- 数値が出せない場合は「なぜ出せないか」「本実施に何が必要か」を明記する。

方針B: 論文の位置づけを変更する
- 第1章・Abstractの記述を「実証的評価を伴う研究」から「設計提案＋リファレンス実装のレポート」に修正する。
- 第8章 Conclusion を、RQ1の検証は Future Work であると明記した上で執筆する。
- 誇大な主張（正答率改善を既定路線のように書いている箇所）を洗い出し、断定を仮説の提示に修正する。

作業前にどちらの方針を取るか私に確認してから進めてください。
```

---

## 2. 【優先度: 高】changes lineage の判定結果を主系譜に統合

**状況(2026-08時点)**：完了。`changes compute` はマイルストーン区間内に存在する**全て**の2親マージコミットを探索し、真の分岐と判定されたものを `ChangeEvent.true_divergences: Vec<TrueDivergence>`(`merge_commit` + `parent_tree_shas`)に古い順で記録するように実装済み (`src/changes.rs`)。当初は区間内で最初(≒最新、`git rev-list`のデフォルト順)に見つかった1件のみを見る暫定実装だったが、レビューで発覚したため一般化した(`checklist-changes-lineage-generalization.md`)。既存の `changes lineage --commit` との役割分担は維持しつつ、主系譜への統合は実現した。

```
src/changes.rs の `changes compute`（マイルストーン境界の線形比較で derived_from を導出する処理）と、
src/lineage.rs / cli.rs の `changes lineage --commit`（git merge-base による2親分岐判定、監査用の独立コマンド）が
現状連携しておらず、lineage の判定結果が changes/*.yaml の derived_from に自動反映されません。

TDDで以下を実装してください:
1. 現状の挙動を再現する失敗するテストを tests/changes_cli.rs に追加する
   （2親分岐があるケースで `changes compute` を実行しても derived_from に2親目が記録されないことを示すテスト）
2. `changes compute` の内部で lineage 判定ロジックを呼び出し、2親分岐が検出された場合は
   derived_from に両方の親を記録するように changes.rs を修正する
3. 既存の checklist-feature-tree-sha.md にある「lineage非連携」の Note を解消済みとして更新する
4. cargo test / cargo clippy -D warnings / cargo fmt --check を通す
5. 論文 §3.2 の記述と実装が一致するように、必要であれば論文側の実装状況表（§3.6）も更新する

破壊的な変更（既存の changes/*.yaml のスキーマ変更）を含む場合は、事前に私に確認してください。
```

---

## 3. 【優先度: 中】分岐・マージを含む検証シナリオをサンプルに追加

**状況(2026-08時点)**：未確認。`mh-sample-test-case`は別リポジトリのため本レビューのセッションでは検証していない。着手前に現状のブランチ構成を確認すること。

```
mh-sample-test-case は現状、単一ブランチ・単一担当者の逐次運用（test1→test2→test3）のみで、
論文が強調する「複数ブランチの分岐」「2親分岐判定」が一度も検証されていません。

以下の手順で新しいケーススタディブランチを作成してください:
1. mh-sample-test-case で feature ブランチを切り、todo-simple の一部Featureに変更を加える
2. main側でも別の変更を加えて、両方をマージする（マージコミットを作る）
3. マイルストーンタグを打ち、`markharness changes compute` と `markharness changes lineage --commit <merge-commit>` を実行する
4. 2親分岐が derived_from（または改修後の統合先）に正しく記録されることを確認する
5. 実行結果（コマンド出力、生成された changes/*.yaml, .markharness-cache/*.json）を
   docs/設計書との相違点_調査資料v2.md または新規ファイルに記録する
6. 想定通りに動かなかった場合は、その乖離を隠さずに記録する（このプロジェクトの既存の誠実な記録文化に倣う）

このシナリオは検証目的のため、mh-sample-test-case の既存データ（test1〜test3）は変更・削除しないでください。
```

---

## 4. 【優先度: 中】verify trace / verify pending を論文本体に反映

**状況(2026-08時点)**：完了。論文§3.7「変更検知に基づく再検証トラッキング」が新設済みで、§3.6実装状況表にも反映されている。`PROJECT.md`の主要機能一覧にも`verify trace`/`verify pending`の記載がある。以下のプロンプトは経緯の記録として残す(再実行不要)。

```
src/verify.rs に実装されている `verify trace` / `verify pending`（TestExecution と ChangeEvent の自動突合、
verified_feature_tree_shas による未検証テストの検出）は、論文 docs/テスト知識管理のGit-nativeモデル_統合版.md に
一切記載がなく、docs/change-event-verification-tracking-spec.md という別紙にのみ仕様があります。

以下を行ってください:
1. 論文に新しい節（例: §3.7「変更検知に基づく再検証トラッキング」）を追加し、
   verify trace / verify pending の目的・仕組み・具体例を、change-event-verification-tracking-spec.md の内容を要約して記載する
2. §3.6 の実装状況表に verify trace / verify pending の行を追加する
3. markharness/README.md と PROJECT.md の機能一覧にも同様に追記する
4. 追記内容は既存の文章スタイル（である調、章番号の体系）に合わせる

論文の他の主張と矛盾しないか確認してから追記してください。
```

---

## 5. 【優先度: 低】canonicalization_rule_version / id_index_schema_version の改訂シナリオ検証

**状況(2026-08時点)**：完了。`src/id_cache.rs`に`stale_canonicalization_rule_version_is_silently_recomputed_instead_of_trusted`・`stale_id_index_schema_version_is_silently_recomputed_instead_of_trusted`のユニットテストが存在し、バージョン不一致時に静かに再計算される挙動を検証済み。ただし「実際の値を"1"から改訂する運用」自体(論文§3.6・§7)はまだ発生していない。以下のプロンプトは経緯の記録として残す(再実行不要)。

```
src/id_cache.rs 等で使われている canonicalization_rule_version / id_index_schema_version が
"1" 固定のまま一度も改訂されておらず、バージョン変更時にキャッシュが正しく破棄・再計算される挙動が未検証です。

TDDで以下を実装してください:
1. バージョン値を意図的に変更した場合、既存キャッシュが無効化され再計算されることを検証するテストを追加する
   （src/id_cache.rs のユニットテスト、または tests/ 配下の統合テスト）
2. バージョン不一致時の挙動（再計算 or エラー）が設計意図と一致しているか確認し、
   一致していなければ実装を修正する
3. この検証結果を checklist-paper-gaps.md に記録する
```

---

## 6. 【優先度: 低】論文V2の実質的な更新

**状況(2026-08時点)**：未確認。`mh-sample-test-case/docs/...V2.md`側は本セッションでは参照していない。項目1・2・4・5の反映状況(上記)を踏まえてV1・V2の差分を再確認すること。

```
docs/テスト知識管理のGit-nativeモデル_統合版.md のV1とV2（mh-sample-test-case/docs/...V2.md）を比較すると、
参照ファイル名の書き換え以外に実質的な差分がありません。

以下を行ってください:
1. 上記1〜5の改善が実施され次第、その内容を反映してV2を更新する
   （§3.6実装状況表、§8 Conclusion、新設した§3.7など）
2. 更新のたびにバージョン番号だけでなく変更履歴（Changelog）セクションを論文末尾に追加し、
   「何が変わったか」を明記する運用に切り替える
3. 今後は内容に実質的な変更がない場合、バージョン番号を上げないルールを CLAUDE.md か PROJECT.md に明記する
```

---

## 7. 【優先度: 中】ChangeEventに related_events(任意・人手記入)を追加

```
docs/review-data-model-improvement-proposals.md の5.3節を踏まえた対応です。

元の分析レポートが提案した「複合ChangeEvent」（複数Featureの変更を1つのイベントに統合し、
Feature単位の自動計算という原子性を崩す案）は却下しました。代わりに、ChangeEventの
Feature単位・自動計算という性質(src/changes.rs::compute_changes)は変更せず、
「これらのChangeEventは実は同じ論理変更の一部だった」という関連付けだけを
人間が事後的に追記できる、加算的なフィールドを追加します。

TDDで以下を実装してください:
1. 失敗するテストを追加する: ChangeEvent が related_events: Vec<String> を持ち、
   デフォルト空配列で(de)シリアライズできること(src/changes.rs のテストに追加)。
2. src/changes.rs の ChangeEvent 構造体に #[serde(default)] related_events: Vec<String> を追加する。
3. markharness changes annotate に --related <event_id>(複数指定可)オプションを追加し、
   既存の annotate_change_type と同様に changes/*.yaml を横断検索して related_events に
   追記できるようにする(src/changes.rs / src/cli.rs)。
4. 存在しない event_id を --related に指定した場合はエラーにする(annotate_change_type の
   NotFound エラーと同様の扱い)。
5. 論文 docs/テスト知識管理のGit-nativeモデル_統合版.md §3.5 に、related_events の目的
   (人間が事後的に関連ChangeEventを相互参照できる、Feature単位の自動計算原則は崩さない)
   を1〜2文で追記する。既存の文体(実装状況の注記スタイル)に合わせること。
6. cargo test / cargo clippy --all-targets -- -D warnings / cargo fmt --check を通す。

フィールド名・CLIオプション名(--related)は破壊的変更ではありませんが設計判断のため、
実装前に私に確認してください。
```

---

## 8. 【優先度: 低】Requirementに source / related_issues(任意)を追加

```
docs/review-data-model-improvement-proposals.md の5.4節を踏まえた対応です。

元の分析レポートが提案した priority / status は、それを読む検証・生成ロジックが
存在せず死んだメタデータ化するため見送りました。source(要件の出所)と
related_issues(外部チケットへの参照配列)のみ、Bidirectionality(逆参照)の
欠如を補う目的で追加します。

TDDで以下を実装してください:
1. 失敗するテストを追加する: schema/requirement.schema.json に source(string, optional)・
   related_issues(array of string, optional)を追加した場合の markharness validate の挙動
   (両方省略可、指定時は型チェックされる)。
2. schema/requirement.schema.json を更新する。additionalProperties: false は維持し、
   source と related_issues を properties に追加、required には含めない。
3. src/knowledge.rs の Requirement 構造体に pub source: Option<String> と
   #[serde(default)] pub related_issues: Vec<String> を追加する。
4. markharness knowledge add の対話フロー(src/interactive.rs)に両フィールドの入力ステップを
   追加するかどうかは任意判断でよい。追加する場合は必ずスキップ可能にする(必須項目にしない)。
5. 論文 §3.1 または §3.5 の requirement.yml 例に反映するかどうかは任意。反映する場合は
   「製品化提案、論文本文には明記なし」の注記を付ける(testcase-generation-design.md の
   既存の書き方に合わせる)。
6. cargo test / cargo clippy --all-targets -- -D warnings / cargo fmt --check を通す。
```

---

## 9. 【優先度: 低】ExpectedResultに生成出所メタデータ(縮小版)を追加

```
docs/review-data-model-improvement-proposals.md の5.5節を踏まえた対応です。

元の分析レポートが提案した generated_by(model/prompt_version/confidence_score等)は、
「knowledge/配下は検証済みの確定知識である」という本スキーマ群(additionalProperties: false)
の前提と衝突するため、そのままでは採用しません。離散的な事実情報(生成手段の種別)と
真偽値のレビューゲートのみに絞った縮小版を実装します。

TDDで以下を実装してください:
1. 失敗するテストを追加する: schema/expected_result.schema.json に generated_by
   (enum: "manual" | "llm" | "auto_combination", optional)と
   verified_by(object: { human_review: boolean }, optional)を追加した場合の
   markharness validate の挙動。
2. schema/expected_result.schema.json を更新する。additionalProperties: false は維持し、
   両フィールドを properties に追加、required には含めない
   (省略時の扱いは「不明」であって「manual」を意味しない、という解釈を schema の
   description か本ドキュメントに明記する)。
3. src/knowledge.rs の ExpectedResult 構造体に対応するフィールドを追加する
   (Option<String> または enum、serde(default) で省略可に)。
4. prompt_version・model名・confidence_score は追加しないこと(レビュー結論通り、
   揮発性が高く「検証済み知識」というスキーマの前提と相性が悪いため)。
5. 将来的なCIゲート(「llm生成かつhuman_review未確認のExpectedResultがあればverifyで
   警告する」等)は本チケットのスコープ外。実装しないが、論文 §7 Future Work に
   1行追記するかどうかは任意。
6. cargo test / cargo clippy --all-targets -- -D warnings / cargo fmt --check を通す。

generated_by を省略した場合の意味(「不明」なのか「manual扱い」なのか)は設計判断のため、
実装前に私に確認してください。
```

---

## 却下ログ(実行プロンプト化しない項目)

以下は`docs/review-data-model-improvement-proposals.md`で却下と判定した項目。再提案・再検討する場合は、まず同資料の該当節を読み、却下理由が解消されているかを確認すること。

- **5.1 test_variations(Conditionの多重化)**：`docs/testcase-generation-design.md`§3.2が「ExpectedResultごとのTestCase分割」を意図的に撤回した経緯と正面衝突するため却下。再提案する場合はConditionの多重化ではなく、既存のCondition分割ワークアラウンドを優先すること。
- **5.2 Axis階層化(`children`フィールド版)**：親子を二重に記録する冗長設計であり、かつ横断ビュー機能という消費者自体が現状存在しないため却下(保留)。命名規約(`id: ui.input`のようなドット区切り)のみであれば代替可能だが、それも消費者機能が実装されるまでは着手しない。
- **5.4 priority / status**：それを読む検証・生成ロジックが存在せず、外部issueトラッカー側の状態と二重管理になるリスクが高いため却下。`source` / `related_issues`のみ項目8で対応する。

---

## 使い方の推奨順序

項目1〜6のうち、実質的に残っているのは項目2(マイルストーン区間内の任意の位置でのマージへの一般化)・項目3・項目6のみ（1・4・5は完了）。3は2の実装後に検証すると効率的（lineage統合後にサンプルで動作確認できる）。項目7〜9はいずれもスキーマへの加算的な変更で相互依存はないため、優先度順(7→8→9)に着手するか、1〜6と並行で進めても支障ない。ただし各項目末尾に「実装前に私に確認してください」とある設計判断(フィールドの意味論・省略時の扱い)は、着手前に必ず確認を取ること。
