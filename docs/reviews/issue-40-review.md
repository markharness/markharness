# Issue #40 レビュー記録

## 対象

- リポジトリ: `markharness/markharness`
- Issue: [#40](https://github.com/markharness/markharness/issues/40)
- 比較範囲: `bc91b38..1ba9920`
- 修正コミット: `1ba9920 fix: 証跡選択・不変Case definition検査・Unresolved証跡の扱いに関するレビュー指摘を修正`
- 判定: `not mergeable`

## 修正で確認できた改善

- 同一のCase UID、revision、対象、環境に対して pass/fail が競合する場合、日時の新旧で自動判定せず `Unresolved` とする。
- `Unresolved` を計画サマリー、JSONスキーマ、表示、終了コードへ反映する。
- 不変 Case definition が存在しない場合、実行記録の作成を拒否する。
- Case definition の構文破損は計画生成時にエラーとして扱う。

## Standards

### [Must fix][High] 採用した実行証跡を明示的に参照していない

- Evidence: `src/plan.rs` の `PlanEvidence` に `execution_uid` がなく、複数の同一結果は最初の要素を暗黙に採用する。
- Violated contract: ADR 0017 §5「計画には採用する実行結果を明示的に関連付ける」。
- Reachability: 同じケース・版・対象・環境で複数回実行した場合。
- Impact: どの実行記録が計画の判定根拠なのか監査できない。
- Proposed remediation: `execution_uid` を証跡・計画出力へ伝播し、採用記録を明示する。未選択の複数件は `Unresolved` とする。
- Verification: 同結果複数、競合、不明・不適合UID、明示選択後の判定。

### [Must fix][High] Case definition の内容・キー一致を検査していない

- Evidence: `src/case_definition.rs` の `load_case_definition` はYAMLをdeserializeするだけで、パスのUID・revisionと内部値を照合しない。記録・計画側も存在確認にとどまる。
- Violated contract: ADR 0017 §5 の実行時実効定義との正確な関連付け、および破損証跡を合格にしない契約。
- Reachability: 手編集、merge conflictの誤解決、定義ファイルの取り違え。
- Impact: 別UID・別revision・別Phaseの定義をpassの根拠にできる。
- Proposed remediation: 内部キーとパスキーを照合し、記録時は生成TestCaseから構築した定義とPhaseを含めて完全一致させる。
- Verification: UID不一致、revision不一致、Phase改変、正常一致。

### [Must fix][Medium] 型付き識別子が本番経路で使われていない

- Evidence: `src/execution.rs` と `src/plan.rs` は `String` および `BTreeMap<String, String>` を使用している。`CaseUid`、`CaseRevision`、`ExecutionUid` などの型は定義・再exportにとどまる。
- Violated contract: ADR 0017 §3 のUID、表示ID、revisionを異なるドメイン型として扱う契約。
- Proposed remediation: 型を永続構造とAPIへ伝播し、入力境界で形式を検証する。
- Verification: 不正形式拒否、serde往復、型による取り違え防止。

## Spec

### [Must fix][High] FeatureがRequirementの表示IDを正本として保存している

- Evidence: `src/knowledge.rs` と `schema/feature.schema.json` は `requirement_ids` を使用している。
- Violated contract: Issue確定コメント §1 の `requirement_uids`。
- Impact: Requirementの表示ID変更や再利用で関連が切れる、または誤接続される。
- Proposed remediation: FeatureにはRequirement UIDを保存し、表示IDは境界で解決する。
- Verification: 複数UID、表示ID変更後の関連維持、不明UID拒否。

### [Must fix][High] 採用する実行結果の明示的な関連付けが未実装

`Unresolved` によって日時だけの上書きは解消されたが、Issue確定コメント §5 の「計画に採用する実行結果を明示的に関連付ける」は未達である。具体的な証拠と修正方針は Standards の1件目と同じ。

### [Must fix][High] Case definition の完全一致が保証されない

欠落と構文破損は検出できるようになったが、期待パスに別UID・別revision・別内容のparse可能な定義を置いた場合は通過する。具体的な証拠と修正方針は Standards の2件目と同じ。

### [Must fix][Medium] Scenario分割・統合の由来記録が未実装

- Evidence: `schema/scenario.schema.json` に由来情報がなく、Scenario向けの独立した由来記録もない。
- Violated contract: Issue確定コメント §8。
- Proposed remediation: 由来の保存形式を定め、切り出し・完全分割・統合で新UIDと由来を記録する。
- Verification: 各操作のUID発行、由来記録、過去証跡を継承しないこと。

## 検証

以下は成功した。

- plan、execution、presentation、case-definition の修正経路に対するfocused tests
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `git diff --check`

前回のフルテスト成功結果も確認済みであり、今回の再レビューではReview Policyに従って修正経路を中心に検証した。レビュー時点のworktreeはクリーンである。

## 結論

Standardsは3件、Specは4件のfindingが残る。最高 severity は両軸とも `Must fix / High` であり、Issue #40 の実装としてはマージ不可。

## Issue分割後の再レビュー（2026-09-06）

Issue #40から次の3項目が子Issueへ切り出されたことを確認した。

| 旧レビュー項目 | 追跡先 | #40での扱い |
|---|---:|---|
| 型付き識別子の本番経路伝播 | [#43](https://github.com/markharness/markharness/issues/43) | Follow-up |
| Featureの`requirement_uids`化 | [#44](https://github.com/markharness/markharness/issues/44) | Follow-up |
| Scenario分割・統合の由来記録 | [#45](https://github.com/markharness/markharness/issues/45) | Follow-up |

これらは設計契約自体から削除されたのではなく、Issue #40の実装範囲から分離されたものとして扱う。したがって、PR本文では完了範囲をStep 1〜4に限定し、必要に応じて`Refs #43`、`Refs #44`、`Refs #45`を記載する。

### #40に残るブロッカー

- **[Must fix][High] 採用証跡の明示参照が未実装**。`Unresolved`により日時だけの競合解決は防げるが、`PlanEvidence`に`execution_uid`がなく、同じ結果の複数実行ではどの記録を採用したか計画から特定できない。ADR 0017 §5の「計画には採用する実行結果を明示的に関連付ける」に違反する。
- **[Must fix][High] Case definitionの完全一致検証が未実装**。定義ファイルの存在とYAML構文は確認するが、パスのUID・revisionと内部値、生成された実効Phaseとの一致を検証していない。別内容のparse可能な定義でも証跡が合格に使われる可能性がある。

### 再レビュー判定

子Issueへ切り出した3項目を#40のブロッカーから除外した場合でも、上記2件が残るため判定は`not mergeable`。#43〜#45は親Issueの設計契約に対する後続実装として、各Issueで独立にレビューする。

## 切り出し方の妥当性

「同じ設計契約に属すること」と「同じPRで実装すること」は分けて判断する必要がある。Issue #40は、所属・同一性・版・実行証跡をまとめる親Issue／設計エピックとして扱うのが自然であり、子Issueを持つこと自体は適切である。

| 項目 | #40との関係 | 切り出し判断 |
|---|---|---|
| 型付き識別子（#43） | UIDと表示IDを区別する基盤契約。複数モジュールへ機械的に伝播する | 切り出しは妥当。ただし#40の完了条件から契約を削除したのではなく、依存する基盤Issueとして扱う |
| `requirement_uids`（#44） | 所属と意味上の関連を分離する中心契約 | 実装PRを分けるのは妥当。#40の設計完了には必要で、#44完了を#40の子Issueとして追跡する |
| Scenario由来（#45） | 分割・統合の履歴契約。具体形式が未決定で、他の実装と独立性が高い | 切り出しが自然。今回のCase UID・revision実装と同じPRへ含める必要はない |
| 採用`execution_uid`の明示参照 | 証跡をVerificationPlanへ適用する直接の契約 | 現在のPRに含めるべき。`Unresolved`だけでは契約を満たさない |
| Case definitionの完全一致 | 実行時に何を実行したかを保証する直接の不変条件 | 現在のPRに含めるべき。実行記録・計画照合と同じ垂直スライスに属する |

したがって、#43〜#45の切り出しは「別テーマにした」という意味では妥当だが、親Issueの設計契約から外れたことを意味しない。#40を完了させるなら、子Issueとの依存関係を明示し、子Issueが完了するまで親を未完了として追跡する必要がある。

現在のPRについては、PR本文の「Step 1〜4まで実装」「型付き参照を実装」という表現を、#43〜#45を後続にした段階的実装の表現へ修正するのが自然である。`Refs #40`に加えて、関連する子Issueを明記し、今回のPRの完了条件を採用証跡の明示参照とCase definitionの完全一致検証までに限定する。
