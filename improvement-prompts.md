# markharness 改善指示プロンプト集

前回のレビュー（ツール評価・論文評価）で挙がった改善提案を、AIエージェント（Claude Code等）に実行させるためのプロンプト。各項目は独立して投入可能。本リポジトリの `CLAUDE.md` のルール（チェックリスト運用・TDD・破壊的コマンドの事前確認）に必ず従うこと。

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

## 使い方の推奨順序

1〜2は論文・実装の信頼性に直結するため先に着手。3は2の実装後に検証すると効率的（lineage統合後にサンプルで動作確認できる）。4〜6は並行して進めても支障ない。
