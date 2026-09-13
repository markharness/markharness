# 0027: 宣言的Knowledge ReconciliationによるAI authoring

## ステータス

Accepted（2026-09-13決定、実装未着手）。

## 背景

AIやスクリプトからKnowledgeを作成する現在の経路は、`KnowledgeDraft`を`knowledge validate`・`knowledge apply`へ渡した後、`identity migrate`を実行するという手続き的な操作を呼び出し側へ要求する。新規Requirementとそれを参照するFeatureを同時に作る場合は、Requirement UIDがまだ存在しないため、さらに事前の対話操作または一時的なdisplay ID参照が必要になる。

この経路では、呼び出し側がKnowledgeの保存順序、UID発行時期、display IDとUIDの使い分け、および`apply`と`migrate`の順序を理解しなければならない。`apply`成功後に`migrate`を忘れたり、その間に処理が中断したりすると、UIDなしのKnowledgeやdisplay IDを含む`requirement_uids`が正規保存領域へ露出し得る。これは[0013](0013-immutable-identity-model.md)の「UIDモードでは新規UIDなしKnowledgeを通常コマンドで導入しない」という不変条件と整合しない。

AI authoringで必要なのは保存手順の指定ではなく、「どのKnowledgeが存在してほしいか」という意図の宣言である。markharnessが、その宣言を現在のリポジトリ状態と照合し、UID発行・参照解決・検証・原子的保存を完結させる必要がある。

## 決定

### 1. `knowledge reconcile`をAI authoringの標準経路とする

次の非対話コマンドを導入する。

```text
markharness knowledge reconcile <intent-file> [--check] [--json] [--dir <path>]
```

入力を**Knowledge Intent**と呼ぶ。Knowledge Intentはauthoring専用の宣言であり、`.markharness/knowledge/`の保存形式ではない。新規要素のUID、保存パス、identity event、および移行手順を呼び出し側へ要求しない。

`knowledge reconcile`は、一つのKnowledge Intentについて次を一操作として行う。

1. 入力を解析し、対象リポジトリの現在のKnowledgeとAxisを読み取る。
2. 文書内参照、既存UID、およびdisplay IDを解決する。
3. ドメイン規則と現在状態に依存する規則を含め、変更全体を検証する。
4. 新規Requirement・Feature・Behavior・ScenarioのUIDを一括で予約する。
5. Featureのcontributes-to関連をRequirement UIDへ変換する。
6. 作成・更新・変更なしを含むmutation planを生成する。
7. 正規Knowledge、identity event、および必要な派生状態を単一のcrash-recoverableなトランザクションで保存する。
8. コミット済み状態がすべての不変条件を満たすことを確認し、構造化結果を返す。

検証完了前には正規保存領域を変更しない。論理commit前の失敗は旧状態へ収束し、論理commit後の失敗はcommit済みの新状態へ冪等にroll-forwardする。UIDなし、未解決参照、Knowledgeとidentity eventの片側だけが更新された状態を通常コマンドへ公開しない。

### 2. Knowledge Intentでは文書内keyにより新規要素を参照する

同一Intent内の新規要素は、保存されない文書ローカルな`key`で参照する。例えば、FeatureからRequirementへの関係は`contributes_to`へRequirementの`key`を記述する。既存RequirementはUIDで参照する。display IDだけによる既存Requirementの選択は行わない。

```yaml
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_todo
    id: todo
    source: native
    label: TODO管理
    axis: [functional]

features:
  - key: feature_todo
    id: todo-management
    contributes_to: [req_todo]
    label: TODO管理
    axis: [functional]
    behaviors:
      - key: behavior_add
        id: add-todo
        description: TODOを追加する
        scenarios:
          - id: empty-title
            description: 空のタイトルは追加できない
            phases:
              - steps:
                  - action: タイトルを空にして追加する
                results:
                  - TODOは追加されない
```

`key`と`contributes_to`はauthoring上の表現であり、正規Knowledgeへ保存しない。正規Featureでは、従来どおり`requirement_uids`だけをcontributes-to関連の正本とする。新しい永続的な関連型や汎用グラフは導入しない。

### 3. 新規作成と既存要素の変更をUIDで明確に区別する

新規要素にはUIDを書かない。markharnessだけがUIDを発行する。

既存要素の内容変更またはdisplay ID変更には、その要素のUIDをKnowledge Intentへ明示する。内容の類似性や同じdisplay IDだけを根拠として既存UIDを推測・継承しない。この規則は[0017](0017-scenario-case-revision-and-execution-evidence.md)の改訂と新規作成の区別、および[0021](0021-identity-retire-simplification.md)の再登場時のidentity規則を維持する。

照合規則は次のとおりとする。

| Intentの指定 | 現在状態 | 結果 |
| --- | --- | --- |
| UIDなし、同じkind・scope・IDが存在しない | 新規 | UIDを発行して作成 |
| UIDなし、同じkind・scope・IDが存在し、正規化後の内容が完全一致する | 既存 | 変更なし |
| UIDなし、同じkind・scope・IDが存在するが内容が異なる | 既存 | `ambiguous_identity`で停止し、UIDの明示を要求 |
| UIDあり、UIDとkindが一致し、現在のscopeとIntent上のscopeも一致する | 既存 | 内容を比較し、更新または変更なし |
| UIDあり、display IDだけが異なる | 既存 | 明示的なrenameとして処理 |
| Scenario UIDあり、Intent上で別のFeatureまたはBehaviorへ配置される | 既存 | 明示的なreparentとしてUIDを維持して移動 |
| UIDが存在しない、kindが異なる、またはScenario以外でscopeが矛盾する | 不整合 | fail-closedで停止 |

Scenarioの内容変更およびUID付きの明示的なreparentではScenario UIDを維持し、effective contentに応じてCase revisionを再計算する。reparentは、既存Scenario UIDを現在とは異なるFeatureまたはBehavior配下へIntent上で配置した場合に限り成立する。UIDを省略した同名Scenarioから移動を推測しない。Scenarioの分割・統合・コピーで新しく生じるScenarioには新しいUIDを発行し、内容一致から分割・統合を推測しない。

### 4. 初期版は削除を行わない`merge`だけを提供する

初期版の`mode`は`merge`のみとする。Intentに記載した要素だけを作成または更新し、記載されていない既存要素は変更しない。省略を削除またはRetireの意思と解釈しない。

望ましい状態との差分から未記載要素をRetireする`exact`モードは、対象scope、確認・権限、派生するScenario/TestCaseへの影響を別途決定するまで実装しない。`--delete`や`--allow-retire`も本ADRの初期実装範囲外とする。

Axisは登録済みのものだけを参照できる。Knowledge IntentからのAxis作成や削除は、KnowledgeとAxisの所有範囲を混在させるため初期版では行わない。未知のAxisは`unknown_axis`で停止する。

### 5. UID付き既存要素にはpatch semanticsを適用する

UIDを指定した既存要素は、Intentに記載したフィールドだけを変更する。省略したscalar、value collectionおよび子Knowledge要素は現在値を維持する。明示したscalarは置換する。必須値を`null`または空値で消去する入力は、そのフィールドのドメイン規則に従って拒否する。

Collectionは次の二種類に分ける。

- **Knowledge要素collection**：Requirement・Feature・Behavior・Scenarioの集合。Intentに記載した要素をUIDまたは新規keyでpatch・作成し、未記載の既存要素を維持する。空collectionも既存要素を削除またはRetireしない。
- **Value collection**：`axis`・`contributes_to`・`procedures`・`phases`・`steps`・`results`。明示した場合はcollection全体を置換する。空collectionは空にする明示的な更新であり、省略とは異なる。置換後の値は各フィールドの必須・非空・参照規則を満たさなければならない。

Featureの`contributes_to`を省略した場合は現在の`requirement_uids`を維持し、指定した場合は文書ローカルkeyまたはRequirement UIDを解決した集合で全置換する。これにより、関連の追加と削除を別コマンドなしで表現する。

既存要素の子Knowledge要素をIntentへ記載したことは、その子だけを作成またはpatchする意味であり、同じ親の未記載の子を削除しない。`procedures`と`phases`は独立したUIDを持つKnowledge要素ではなく、所有元のBehaviorまたはScenarioの値であるため、この規則ではなくValue collectionの全置換規則に従う。

External Requirementの固定参照を現在の`.sdoc` blobへ進める場合は、UID付きRequirementへ`source_revision: current`を明示する。Reconciliation Moduleは`source_locator`が指す現在のblob OIDを解決して正規Knowledgeへ保存し、native Requirement、存在しないlocator、または解決不能なGit状態ではfail-closedで停止する。`current`はIntentだけの命令値であり、正規Knowledgeへ保存しない。

### 6. `--check`と通常実行は同じmutation planを使う

`--check`は解析・照合・検証・mutation plan生成までを通常実行と同じ実装で行い、書込みだけを行わない。差分がなければ成功し、変更が必要なら機械判定可能な専用終了コードと計画を返す。

通常実行はコミット直前に現在状態を再確認し、`--check`後または計画中に入力状態が変化していればstale planとして停止する。`--check`の結果を、そのまま後続書込みの許可証として扱わない。

同じリポジトリ状態と同じIntentからは、UIDの具体値を除いて同じmutation planを生成する。新規UIDは計画上の一時トークンで表し、コミットする実行内で割り当てる。同じUIDなしIntentを成功後に再実行した場合、kind・scope・display IDおよび正規化後の内容が完全一致する要素は`unchanged`となる。内容差分を伴う更新やrenameには、最初の成功結果または最新の機械可読snapshotから得たUIDをIntentへ明示する。

### 7. 結果と診断を安定した機械可読形式で返す

`--json`の成功結果は、少なくとも`created`・`updated`・`unchanged`、各要素のkind・UID・display ID、および変更パスを返す。失敗結果は安定した`code`、Intent内の位置、message、可能ならremediationを返す。Rustの型名や内部エラー文字列を外部契約にしない。

初期版で必要な診断には、少なくとも次を含める。

- `invalid_format`
- `duplicate_key`
- `duplicate_uid`
- `unknown_local_reference`
- `unknown_axis`
- `ambiguous_identity`
- `unknown_uid`
- `conflicting_scope`
- `conflicting_existing_value`
- `invalid_procedure_reference`
- `invalid_source_revision`
- `stale_plan`
- `invariant_violation`

### 8. `identity migrate`をauthoring手順にしない

`knowledge reconcile`成功時点で、作成したすべての正規KnowledgeはUIDを持ち、すべてのUID参照が解決済みでなければならない。後続の`identity migrate`を成功条件に含めない。

`identity migrate`は既存・手動導入データの移行または明示的な修復操作として残す。AI向け文書では`knowledge reconcile`を標準経路とし、複数コマンドの手続き的フローを案内しない。

`knowledge reconcile`と責務が重複する既存のauthoringコマンドは、[0028](0028-consolidate-knowledge-authoring-commands.md)に従い、本ADRのReconciliation Moduleと移行先Interfaceが完成した後に廃止する。旧経路との後方互換は提供しない。

## 不変条件

- UIDなしでよいのは未保存のKnowledge Intent内の新規要素だけである。
- コミット済みのRequirement・Feature・Behavior・ScenarioはすべてUIDを持つ。
- `requirement_uids`にはRequirement UIDだけを保存し、display IDやIntentの`key`を保存しない。
- Knowledgeとidentity eventは一つの論理コミットとして更新する。
- 1 Scenario = 1 TestCaseを維持し、Case UIDはScenario UIDから決定的に導出する。
- Requirementへのcontributes-to関連の正本はFeature側だけに置く。
- 内容の類似性からidentityを推測しない。
- Intentのscope外や未記載の要素を暗黙に変更・Retireしない。
- `source: external`のRequirement本文をmarkharnessの正本として取り込まない。

## 影響範囲

- authoring専用のKnowledge Intent schemaを新設し、正規Knowledge schemaと明確に分離する。
- 現在状態の読取り、参照解決、検証、UID予約、mutation plan、crash recoveryを一つのReconciliation Moduleの内部実装へ集約する。
- CLIはReconciliation Moduleを呼ぶ薄いAdapterとし、将来GUI等を追加しても同じInterfaceを利用する。
- テストはModuleのInterfaceを主なtest surfaceとし、新規一式、既存更新、rename、曖昧identity、未知Axis、再実行、書込み失敗、pre-commit/post-commit recoveryを検証する。
- `docs/knowledge-from-code.ai.md`は実装完了時に`knowledge reconcile`を標準手順として更新する。
- [0013](0013-immutable-identity-model.md)のUIDなしKnowledge禁止は緩和しない。実装が同ADRの既存crash-recovery protocolを再利用・拡張する場合も、通常コマンドへ中間状態を公開しない条件を維持する。

## ADR 0028開始ゲート

[0028](0028-consolidate-knowledge-authoring-commands.md)の削除へ進む前に、次の機能完成ゲートをすべて満たす。本節はADR 0027全体の受け入れ条件ではなく、ADR 0028を開始するための中間ゲートである。

- 新規Knowledge一式の作成、既存要素の変更なし判定、UID付きpatch・rename・Scenario reparent、および複数要素の一括処理が動作する。
- `contributes_to`の全置換でRequirement関連の追加・削除を表現できる。
- External Requirementの`source_revision: current`が、既存`requirement repin`と同じblob OID検証を行う。
- `--check`、機械可読結果、Intent雛形、およびcrash-recoverableな原子的保存が動作する。
- 現在のKnowledgeDraftで表現できるKnowledgeを情報を失わずIntentで表現できる。
- 旧コマンドの置換動作を確認するテストが、旧コマンドをまだ削除していない状態で通る。

## 受け入れ条件

本ADRは、次をすべて満たしたときに実装完了とする。

- `knowledge reconcile`が、新規Knowledge一式の作成、既存要素の変更なし判定、UIDを指定した更新・rename、複数要素の一括処理、`--check`、機械可読結果、Intent雛形、およびcrash-recoverableな原子的保存を提供する。
- 現在のKnowledgeDraftで表現できるRequirement・Feature・Behavior procedure・Scenario phaseを、情報を失わずKnowledge Intentで表現できる。
- UIDなしKnowledge、未解決参照、Knowledgeとidentity eventの片側だけが更新された状態を通常コマンドへ公開しないことを、pre-commitおよびpost-commit/pre-roll-forwardの回復テストで確認する。
- [0028](0028-consolidate-knowledge-authoring-commands.md)を本ADRの機能実装後に実行し、重複する旧authoringコマンドと実装を削除する。
- AI向け文書、日英CLIマニュアル、README、schemaおよび例を、`knowledge reconcile`だけをauthoringの標準経路として示す状態へ更新する。

## 検討したが採用しない選択肢

- **`knowledge apply`後に`identity migrate`を1回実行する**：現行実装との差分は小さいが、display IDを`requirement_uids`へ一時保存し、UIDなしの正規Knowledgeとコマンド順序を呼び出し側へ露出する。
- **`knowledge apply`内部だけを拡張する**：一操作で完結できるが、現在の1チェーン・create-or-reuse中心のDraft契約と、複数要素の望ましい状態を宣言する契約が同じInterfaceに混在する。既存コマンドの意味を暗黙に変えるより、authoring intentのseamを明示し、移行完了後は重複する旧Interfaceを削除する。
- **AIが正規Knowledgeファイルを直接作成してから正規化する**：保存レイアウトと内部参照形式がAI向けInterfaceとなり、不正な正規データを先に作業木へ露出する。
- **内容ハッシュからUIDを決定的に生成する**：採番操作は不要になるが、内容変更やrenameを同一identityとして追跡する要件を破る。
- **対話形式を拡張する**：人には利用できても、AI・CI・スクリプトには標準入力、再開、エラー修正の状態管理が不安定なまま残る。
- **最初から`exact`同期と暗黙Retireを提供する**：宣言的同期としては完全だが、scopeの誤指定やAIの省略が破壊的変更になる。具体的な削除要求がない初期版には不要である。
