# 0028: Knowledge authoringを`knowledge reconcile`へ統合する

## ステータス

Accepted（2026-09-13決定、実装未着手）。[0027](0027-declarative-knowledge-reconciliation.md)の「ADR 0028開始ゲート」を満たした後に実行する。

## 背景

[0027](0027-declarative-knowledge-reconciliation.md)は、Knowledge Intentの解析、状態依存検証、UID発行、参照解決、更新・rename、および原子的保存を`knowledge reconcile`の一つのInterfaceへ集約する。

現在の`knowledge add`・`knowledge scaffold`・`knowledge validate`・`knowledge apply`は、同じKnowledge authoringを対話入力、旧KnowledgeDraftの雛形、検証、保存という手続き的な段階へ分割している。`feature rename-id`も、UIDで既存Featureを選択してdisplay IDを変更するため、UID付きIntentによるrenameと同じ状態遷移になる。

新旧Interfaceを並存させると、二つの入力schema、二つの検証規則、二つの原子的書込み経路、および複数の推奨手順を維持することになる。[0026](0026-module-inventory-and-plan-removal.md)の後方互換を考えない方針に従い、移行先が完成した後は重複を残さない。

## 決定

### 1. 重複するauthoringコマンドをすべて廃止する

[0027](0027-declarative-knowledge-reconciliation.md)の「ADR 0028開始ゲート」を満たした後、次を同一変更で削除する。

- `markharness knowledge add`（`--edit`を含む）
- `markharness knowledge scaffold`
- `markharness knowledge validate`
- `markharness knowledge apply`（`--batch`・`--dry-run`を含む）
- `markharness feature rename-id`
- `markharness requirement link`
- `markharness requirement unlink`
- `markharness requirement repin`

旧コマンドのalias、非推奨期間、互換wrapper、旧引数を受け取る隠し経路は設けない。削除後のKnowledge authoring Interfaceは次だけとする。

```text
markharness knowledge reconcile <intent-file> [--check] [--json] [--dir <path>]
markharness knowledge reconcile --print-template
```

複数Knowledge要素は一つのIntentへ記述するため、旧`--batch`相当の別モードを設けない。`--check`は旧`knowledge validate`および`knowledge apply --dry-run`の役割を包含する。`--print-template`は旧`knowledge scaffold`を置き換える。

### 2. 旧KnowledgeDraft実装を削除する

旧コマンドからだけ利用されるKnowledgeDraftの型、parser、validator、apply処理、editor loop、template、参考schema、テスト、および専用ドキュメントを削除する。現行ファイル名では少なくとも次が対象候補になるが、削除時には参照検索で実際の到達可能性を確認する。

- `src/knowledge_draft.rs`
- `src/knowledge_apply.rs`
- `src/knowledge_edit.rs`
- `docs/knowledge_draft.schema.json`
- 旧KnowledgeDraftだけを対象とするunit test・CLI integration test・example draft

Reconciliation Moduleと共有すべきdomain validation、Knowledge parser/serializer、filesystem safety、identity replayおよびcrash recoveryは削除せず、移行先Moduleから利用する。旧Moduleを残して新Moduleから呼ぶ構造にはせず、必要な規則を現在の責務に対応する場所へ移してから旧Moduleを削除する。

### 3. 人間向け操作も同じInterfaceへ統一する

対話promptと`$VISUAL`/`$EDITOR`起動をKnowledge authoringの組込み機能として残さない。人もIntent templateを取得・編集し、`knowledge reconcile`を実行する。エディタ起動はshellやエディタ自身の責務とする。

Featureのrenameも、対象UIDと新しいdisplay IDを含むKnowledge Intentで行う。`feature rename-id`専用のmutation pathは残さない。Reconciliation Moduleは既存のidentity eventとcrash-recovery不変条件を維持した同じrename結果を生成する。

FeatureとRequirementの関連追加・削除は、UID付きFeatureの`contributes_to` collectionを全置換するpatchで行う。External Requirementの固定参照更新は、UID付きRequirementの`source_revision: current`で行う。`requirement link`・`unlink`・`repin`専用のmutation pathは残さない。

### 4. identity保守・監査とAxis管理は残す

次はKnowledge authoringと責務が異なるため廃止しない。

- `identity migrate`：既存または手動導入データの移行・修復
- `identity audit`：Git履歴全体のidentity event監査
- `identity resolve`：branch divergenceの明示的解決
- `identity sync`：identity eventからKnowledgeファイルを再導出する障害復旧
- `axes list`・`axes add`・`axes prune`：初期版Knowledge Intentの外で管理するAxis registry操作

`markharness validate`は正規Knowledge・Axis・関連する保存状態全体を検証するコマンドであり、未保存Intentを検証する旧`knowledge validate`とは異なるため残す。

### 5. 実行順序を固定する

実装順序は次のとおりとする。

1. [0027](0027-declarative-knowledge-reconciliation.md)のReconciliation Module、Knowledge Intent schema、CLIおよび回復テストを実装する。
2. ADR 0027の「ADR 0028開始ゲート」を満たし、更新・rename・Scenario reparent・Requirement関連の追加と削除・external Requirementのrepin・複数要素・`--check`・雛形出力が機能することを確認する。
3. README、AI向け文書、日英CLIマニュアル、例を`knowledge reconcile`へ切り替える。
4. 本ADRの§1・§2に従い、旧コマンドと旧KnowledgeDraft実装を同一変更で削除する。
5. 全テスト、lint、format、license、生成物自己検証、およびCLIから旧コマンドが到達不能であることを確認する。

旧コマンドを先に削除して一時的にauthoring不能な状態を作らない。一方、移行先完成後に新旧経路を複数リリース並存させない。[0027](0027-declarative-knowledge-reconciliation.md)は本ADRの削除完了を受け入れ条件とする。

## 帰結

- Knowledge authoringで利用者が学ぶ書込みInterfaceは一つになる。
- UID発行、参照解決、検証、rename、原子的保存の規則がReconciliation Moduleへ集約される。
- TTY対話、editor loop、1チェーンDraft、batchのファイル順依存、およびapply後のmigrate手順がなくなる。
- 旧KnowledgeDraftを使用する外部スクリプトは動作しなくなる。これは意図した破壊的変更であり、互換経路は提供しない。
- `identity migrate`等の保守コマンドを削除対象に含めないため、既存リポジトリの修復・監査能力は維持される。

## 検討したが採用しない選択肢

- **旧コマンドを非推奨として残す**：移行期間は利用者に親切だが、二つのschemaとmutation pathを維持し、最善のInterfaceへ統一するという目的を損なう。
- **旧コマンドをReconciliation Moduleのwrapperとして残す**：実装重複は減るが、旧KnowledgeDraftの制約と複数のCLI表面は残る。
- **`knowledge add --edit`だけを人向けに残す**：人とAIで異なるauthoring経路になり、検証・エラー・再実行の挙動が分岐する。template編集で十分代替できる。
- **`feature rename-id`を便利な短縮形として残す**：操作自体は簡潔だが、renameだけに別Interfaceとmutation pathを残す。UID付きIntentへ統一するほうが小さい。
- **`requirement link`・`unlink`・`repin`を便利な短縮形として残す**：関連collectionの置換とexternal Requirementの固定参照更新はKnowledgeの更新そのものであり、専用mutation pathを残すと単一Interfaceの決定に反する。
- **identity保守コマンドも`reconcile`へ統合する**：migration、履歴監査、branch divergence解決、障害復旧はdesired-state authoringとは異なる権限・入力・失敗モードを持つため、一つのInterfaceへ混ぜない。
