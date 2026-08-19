# 0013: 可変IDと不変Identityを分離するKnowledge同一性モデル

## ステータス

Proposed

## 背景

[0004](./0004-feature-id-change-migration.md)は、Feature `id:`変更後も履歴を追跡したいという具体的な要望が出た場合に移行方法を再検討するとしていた。[Issue #17](https://github.com/markharness/markharness/issues/17)により、ドメイン用語変更、命名規則統一、他システムからの移行、repository統合、組織・製品再編という具体的な必要性が示され、この条件が満たされた。

当初は`feature.yml`へ旧IDの`aliases`を追加する案、次にFeatureだけへ不変`uid`を追加する案を検討した。しかし、いずれも次の問題を局所的にしか解決しない。

- 現在のKnowledge treeだけでは、削除済みFeatureの旧ID・UID再利用を検証できない。
- Feature IDはChangeEvent、lineage、verification、execution、canonical import、derived index、server表示など複数のconsumerで直接使われている。
- TestCase IDにもFeature IDが含まれ、renameによりexecutionとの対応が切れる。
- Git差分だけでは、意図したrenameと通常の手編集を区別できない。
- Feature以外のRequirement、Behavior、Condition、ExpectedResultにも同じrename問題が将来発生しうる。

根本原因は、人間向けの可変な名前と機械的な同一性を同じ文字列IDに担わせていることである。実装コストを判断材料にせず、この二つをKnowledgeモデル全体で分離する。Git上の任意の2 ref snapshotから結果を決定的に再計算できるという既存の原則(論文§3.2〜3.4)は維持するが、入力の役割を分離する。内容変更の`ChangeEvent`は従来どおり2つのKnowledge snapshotから導出し、稀なidentity lifecycle宣言だけがそのsnapshot間で論理的同一性を解決する。Identity eventは通常編集の操作ログではない。入力範囲はFeature tree SHAだけからcommit済み`.markharness` snapshotへ一般化する。

この設計は、外部データベースプロセスや専用サーバーを一切必要としないという意味では、論文§1.4「専用DB不要」の字義を満たす。しかし実態としては、Git管理されたappend-only identity eventログとそのreplayによる導出・crash-recovery protocolという、軽量なevent-sourcing型ストレージエンジンをGit管理ファイルの上に自作することになる。これは「専用DBが持つ複雑さそのものを避ける」という同フレーズの含意からは踏み出す選択である。この判断はIssue #17の要求(削除後も残る同一性追跡)から見て有用性の観点で正当化される。論文§1.4はそのため、Gitを唯一の永続化境界とし、軽量identity event storeをリポジトリ内に持ち、Git外の正準永続化サービスを持たないという正確な境界を記述する。

## 決定内容

### 1. 全ての永続ドメイン要素へ不変UIDを導入する

Requirement、Feature、Behavior、Condition、ExpectedResultに不変`uid`を持たせる。UIDはCLIが要素の発行時に生成する26文字のULIDとする。

```yaml
uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
id: task-management
label: Task Management
```

役割を次のように分離する。

| 値 | 用途 | 変更 |
|---|---|---|
| `uid` | 内部同一性、関連付け、外部連携 | 不可 |
| `id` | CLI入力、人間可読な外部識別子 | 明示操作で可能 |
| `label` | 自由な表示名 | 可能 |
| path | Git上の配置 | 可能 |

ChangeEvent、lineage、verification、execution、canonical import、derived index、server履歴表示はUIDを同一性キーに使う。IDは対象ref内でUIDへ解決する。同一snapshot内のUIDとIDはそれぞれ一意でなければならない。

#### 5要素種別に共通する実装構造

5要素種別へ同じidentity lifecycleを個別実装しない。UID発行、rename、retire、restore、release、reissue、event replay、Registry導出、migration、共通validationは、一つの深いIdentity ModuleのImplementationへ集約する。その小さなInterfaceは少なくとも次の共通ドメイン型を受け渡す。

- `EntityKind` (`Requirement`、`Feature`、`Behavior`、`Condition`、`ExpectedResult`)
- 型付きの`EntityUid`と`EntityId`
- `IdentityHeader` (`uid`、`id`、kind)
- `IdentityMutation` (`Issue`、`Rename`、`Retire`、`Restore`、`ReleaseId`、`Reissue`)
- `IdentityEvent`と、mutation planを検証・生成する`IdentityEngine` Interface

要素種別ごとの差は、親kind、marker file、schema名、ID policyなどの宣言的な`EntityDescriptor`へ置く。種類固有の読み書きが本当に異なる箇所だけに薄いAdapterを置き、lifecycle規則をAdapterへ複製しない。1種類しか実装がない振る舞いのために抽象的なSeamを増やさない。

Knowledgeの親子参照も可変IDではなくUIDを正準にする。`requirement_uid`、`feature_uid`、`behavior_uid`、`condition_uid`を保存し、IDは表示・CLI解決用のprojectionとする。これにより親のrenameが子孫ファイルの関係書換えを発生させない。

Rustのdomain type、配布JSON Schema、`markharness init`が生成するschemaを別々の正準として手作業で同期しない。共通`IdentityHeader`を含むRustのdomain typeをschemaの単一の正準情報源とし、配布schemaとinit schemaを決定的に生成する。生成物をcommitする場合、CIは再生成差分を拒否する。

全`EntityKind`へ同じcontract test suiteを適用する。少なくともUID必須、重複拒否、rename event、event replayとKnowledgeの一致、cache有無の等価性、migration冪等性、crash recovery、descriptor/schema/fixtureの網羅性を種類ごとに検証する。種類追加時は`EntityKind`、descriptor、schema、fixtureの不足を一つの網羅性testで検出する。

### 2. identity宣言を正準情報源とし、Registryは派生物に保つ

現在のKnowledge treeから消えた同一性も追跡するため、`.markharness/identity-events/`のappend-only eventをidentity lifecycleの唯一の正準情報源とする。eventは`knowledge/`と同じく通常のGit管理ファイルであり、外部データベースやツール内部の状態ではない。記録するのは、最終content snapshotだけから意図を復元できない発行、rename、retire、restore、release、reissueに限る。通常のKnowledge編集はidentity eventにせず、内容`ChangeEvent`は2 snapshot差分から事後導出する。

`.markharness-cache/identities/`のIdentity Registryは、対象refに含まれるidentity eventを決定的にreplayして得られる非commitのmaterialized viewとし、既存id解決cacheと同じ設計原則に従う。削除後も同じrefのeventだけから再構築できなければならない。Knowledge YAMLは現在のKnowledge内容の投影であり、validatorはevent replayとKnowledge YAMLを直接比較する。Registry cacheがある場合はcontent-addressed cache keyとreplay結果の一致後だけ使う。CLIはrename・作成・retire・restore・release・reissueのevent追加とKnowledge YAML変更を一つのcrash-recoverableなidentity operationとして行い、cacheは後から再生成または無効化する。

```yaml
# .markharness-cache/identities/features/01ARZ3NDEKTSV4RRFFQ69G5FAV.yml
uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
kind: feature
status: active
current_id: task-management
id_history:
  - id: todo-management
    from_identity_event_uid: 01ARZ3NDEKTSV4RRFFQ69G5FE0
  - id: task-management
    from_identity_event_uid: 01ARZ3NDEKTSV4RRFFQ69G5FE1
```

RegistryはUIDの発行、lifecycle、ID履歴を表す派生snapshotである。Registryの欠落は正常であり再構築を起動する。存在するcacheが古い、または不整合な場合は破棄する。要素を削除してもreplay結果には`retired`として残る。

各要素のlifecycle eventは因果graphを構成する。発行eventは先行eventを持たず、通常の後続eventは`previous_identity_event_uid`でそのsnapshot内の現在headを参照する。競合解決eventは`previous_identity_event_uids`で解決対象の全divergent headをjoinする。replay順序はこれらの先行参照で定め、filename、`recorded_at`、filesystemの列挙順、ULIDの時刻順に依存しない。独立した要素graphは任意順でreplayでき、byte-for-byteで同じ結果を生まなければならない。同じheadを伸ばす2つのeventはbranch divergenceである。各branch snapshotは個別に正常だが、両方を含むsnapshotは明示的な解決eventを必要とし、なければ曖昧性エラーにする。

この設計により、`changes compute`が読む入力範囲は、従来の「`knowledge/`配下のFeatureツリーSHA」から「ある時点でコミットされた`.markharness`スナップショット全体(`knowledge/`・`identity-events/`・migration manifest・必要な生成物を含む)」へ一般化される。任意の2つのrefの比較は、各snapshotにeventをreplayするだけで完結し、Git commit historyを辿らない。同じentity UIDが両snapshotにある場合、root発行event UIDとcanonical payloadが一致し、両snapshotに共通する全event UIDのcanonical contentがbyte単位で同一でなければならない。rootが異なる、または共通eventが書き換えられている場合は継続性ではなくidentity conflictとする。branch固有のsuffixは個別にreplayし、branch統合時にその和集合をvalidationする。

`ChangeAnalyzer`・`verify`等の中核Moduleの結果は、比較対象2refのコミット済み`.markharness` snapshot、明示されたoptions、identity canonicalization version、tool versionだけで決定される。working tree、現在のHEAD、外部DB・外部service、wall-clock time、乱数、非コミットcache、第三のrefには依存しない。cacheを利用しても、cacheを削除した場合とbyte-for-byteで同じ結果を返さなければならない。

Git commit historyを走査する`IdentityAuditor`は、両方の選択snapshotから消えたevent、共通event集合外の過去改変、repository履歴上のUID再利用を検出する。2-ref比較とは別Moduleである。`ChangeAnalyzer`は比較対象2 snapshotの整合性に加え、root発行eventと共通eventの同一性を保証するが、repository全体のappend-only完全性やcross-branch履歴の網羅性までは主張しない。`changes compute`・`verify`等の中核パスは`IdentityAuditor`に依存しないが、出力でこの狭い監査境界を明示する。

snapshot内のevent replayにより、UID重複・変更・再利用、旧ID再利用、削除済み要素の再出現、repository統合時の衝突を検証できる。一度あるUIDへ発行されたIDは、明示的な`release` event(下記)で解除されない限り、同じUIDのrestoreを除き別UIDへ再割り当てできない。過去commitに対するeventの削除・改変は`IdentityAuditor`が検出する。

### 3. renameとlifecycle変更を第一級イベントとして保存する

renameは通常のYAML編集ではなく、CLIによる明示的なドメイン操作として行い、`.markharness/identity-events/`へappend-only eventを追加する。

```yaml
identity_event_uid: 01ARZ3NDEKTSV4RRFFQ69G5FE1
previous_identity_event_uid: 01ARZ3NDEKTSV4RRFFQ69G5FE0
type: feature_renamed
entity_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
from_id: todo-management
to_id: task-management
recorded_at: 2026-08-20T12:34:56Z
```

`markharness feature rename-id <old> <new>`は、UID維持、ID重複検証、Knowledge YAML更新、event追加、Registry cache無効化、全体validationを一つのidentity operationとして実行する。手編集によるID変更は、対応するevent遷移がないためvalidation errorになる。

作成、retire、restore、release、import時のreissueも同じeventモデルで記録する。`release`は、`retired`状態のUIDに紐づく旧idの再利用予約を明示的に解除するeventであり、以後そのidを別のUIDへ新規発行できるようにする。対象snapshot内のevent順序矛盾はreplay時に検出し、過去commitに対するevent fileの変更・削除は`IdentityAuditor`が検出する。

#### crash recovery方針

複数ファイルをOSレベルで同時に書き換える「真のmulti-file atomic write」や、通常エラー時に全ファイルを必ず旧状態へ戻すrollbackを必須方式とはしない。必要な保証は、処理途中の状態を正常状態として公開せず、通常エラー・プロセスkill・system crash後の次回起動時に、旧状態またはcommit済みの新状態のいずれかへ収束することである。

identity operationは、少なくともtransaction intent、staging、単一の論理commit point、recovery情報を持つ。commit pointより前の未完了operationは破棄または旧状態へ復旧し、commit pointより後は正準identity eventからKnowledge projection・生成物を冪等にroll-forwardし、派生cacheを無効化または再生成する。対応するcommitted operationが存在する不一致はrecovery対象とし、operation記録のない不一致は不正な手編集としてvalidation errorにする。

通常コマンドは開始時に未完了operationを検出し、recoveryを完了するまで通常処理を行わない。同時実行はlockで制御する。recovery自身が途中停止しても再実行可能でなければならない。既存の`knowledge_apply::apply_batch`のbest-effortな個別ファイル削除は、この保証を満たすtransactionプリミティブとはみなさない。

### 4. TestCaseとExecutionも不変Identityで追跡する

生成TestCaseには不変`case_uid`を割り当てる。`case_id`は現在の人間可読な投影として維持するが、照合には使わない。

```yaml
case_uid: 01ARZ3NDEKTSV4RRFFQ69G5FT1
case_id: tc-task-management-create-task-empty-title
generated_from:
  requirement_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAA
  feature_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
  behavior_uid: 01ARZ3NDEKTSV4RRFFQ69G5FB1
  condition_uid: 01ARZ3NDEKTSV4RRFFQ69G5FC1
  expected_result_uids:
    - 01ARZ3NDEKTSV4RRFFQ69G5FD1
```

`case_uid`は、`requirement_uid`・`feature_uid`・`behavior_uid`・`condition_uid`・`expected_result_uid`の集合(canonical順に整列)から決定的に導出する(例:これらを連結した値への決定的ハッシュ)。これは新たな永続ストア(TestCase Identity Registryのようなもの)を必要としない純粋関数であり、`generate`が既に持つ決定性・純粋性(同じKnowledgeスナップショットからは常に同じ出力が得られる性質)をそのまま維持する。同じprovenance集合であれば再生成のたびに同じ`case_uid`が得られる。ID・label・path変更やprovenance構成要素自体の内容変更ではTestCase identityを維持し、Conditionの分割などprovenance構成要素のUID集合そのものが変わる場合は新しいTestCaseとする。

execution recordは`execution_uid`、`case_uid`、`feature_uid`、実行時点の`case_id`、検証したFeature tree SHAを保存する。verificationは`case_uid`で再実行を照合する。

### 5. ChangeEvent自体にも不変UIDを導入する

`change_event_uid`を内部参照キーとし、現在の`event_id`は人間可読な表示値へ位置づけを変える。`change_event_uid`はULIDを新規発行せず、2-ref再計算時に決定的に導出する。

```yaml
change_event_uid: 8f8a3c5d-2df5-5ca7-95ef-11e405455a07
event_id: task-management--v2--v3
feature_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
feature_id_at_from: todo-management
feature_id_at_to: task-management
from_milestone: v2
to_milestone: v3
```

導出にはdomain separator、identity canonicalization/algorithm version、from/to snapshot identity、対象`feature_uid`、canonical change payload、結果に影響する明示optionsを、型tagと長さを含むcanonical encodingで入力したUUIDv5または同等のversioned hashを使う。一般的なtool versionはUIDへ直接含めず、UIDの意味が変わる場合だけalgorithm versionを上げる。同じ入力からは常に同じUIDを得て、入力境界の衝突を許さない。annotation、related event、verificationとの関連は`change_event_uid`で保存する。過去・現在の表示IDは監査情報として保持する。

### 6. copy、import、repository統合の規則

- 同じ要素を継続するcopy/importだけ既存UIDを保持する。
- 別要素として取り込む場合は新UIDを発行し、reissue eventを記録する。
- 異なる要素が同じUIDを持つrepositoryを統合する場合、一方を明示的にreissueしてから統合する。
- 外部システムとの対応表はUIDを保存し、IDは表示用snapshotとしてのみ保持する。

## 移行

実装は、(1)共通Identity Moduleとcrash-recovery機構、(2)Featureを使ったend-to-endのvertical slice、(3)残る4種類のdescriptor/Adapter、(4)全要素migration、(5)schema version 2の公開cutover、の順で進める。Feature vertical sliceは共通設計とInterfaceを早期検証する内部段階であり、FeatureだけがUID modeになる中間形式を公開・永続サポートしない。公開cutoverまでは既存schema versionを維持し、cutoverは5種類を一括して切り替える。

project-level markerを`.markharness/config.toml`へ追加する。

```toml
[identity]
schema_version = 2
mode = "uid"
```

`markharness identity migrate`は次を一つのcrash-recoverableなidentity operationとして行う。

1. 全Knowledge要素へUIDを発行する。
2. 初期発行eventを作成し、非commitのIdentity Registry cacheを導出する。
3. 既存TestCaseへ`case_uid`を割り当てる。
4. 既存ChangeEvent・executionとのlegacy mappingを作成する。
5. schema、cache/index canonicalization version、project markerを更新する。
6. 生成物を再生成して全体validationを実行する。

dry-runでは予定するUID、競合、変更ファイルを表示する。論理commit pointより前の失敗ではUID modeを有効にせず、commit後の失敗では次回起動時に冪等なroll-forwardで移行を完了する。partial migrationは正規状態として通常コマンドへ公開しない。

過去refと既存成果物を読み取るため、migration manifestへlegacy snapshot identity(tree SHA)、entity kind、旧ID、旧path/content locator、旧case IDと新UIDの対応を保存する。比較方向にかかわらず、2つのsnapshotに含まれるmanifestを対称に収集し、snapshot-qualified keyから一意に解決する。mappingがない、または複数候補が残る場合は決定的なエラーにする。UID modeへの移行後にUIDなし要素が追加された場合は通常コマンドを拒否し、明示的なrepair/import操作を要求する。移行済みかどうかはFeature数やUIDの有無ではなくproject markerで判定する。

## 検証規則

- UIDの形式、一意性、不変性、retired UID再利用禁止、および`release`されていない過去IDの別UIDへの再割り当て禁止。
- identity event replayとKnowledge YAMLの整合性。Registry cacheがある場合はcache keyとreplay結果が一致し、不一致なら破棄して再構築すること。
- ID変更には対応するrename eventがちょうど一つ存在すること。
- 対象snapshot内のidentity eventが曖昧でない因果chainを構成し、矛盾なくreplayできること。filenameや時刻を順序決定に使わない。
- 両比較snapshotに同じentity UIDがある場合、root発行eventと全共通eventが同一であること。
- 生成`case_uid`が、対応するprovenance UID集合からの決定的導出と一致すること。
- UID modeではUIDなしKnowledge・生成物・executionを新規作成しないこと。
- migration manifestなしで移行境界をまたぐ比較をしないこと。
- `changes compute`・`verify`等の中核パスが、比較対象2refの`.markharness` snapshot、明示options、canonicalization/tool version以外へ依存しないこと。
- `IdentityAuditor`だけがGit commit historyを走査し、repository全体のevent append-only性と、選択2 snapshotの外側にある削除・過去改変を検証すること。

## 0004およびIssue #17の要件への対応

- ID変更は`rename-id`と永続rename eventにより明示的・監査可能になる。
- `changes compute`はUIDで旧IDと新IDを同一要素として解決する。
- TestCase、execution、ChangeEvent、外部対応表はUIDで履歴を継続する。
- alias、循環、alias再利用規則は不要になる。
- 保持されたidentity eventのreplayにより、削除後もUID・旧IDの再利用を検出できる。Registryは再構築可能なcacheにすぎない。
- migration markerとmanifestにより既存projectと過去成果物を読み取れる。
- identity宣言と派生される内容変更を分離し、入力範囲を`.markharness` snapshotへ一般化することで、決定的な2-ref再計算を維持しながら全要素の同一性を保証する。通常の内容変更は編集操作ログを必要とせず、snapshotが推論できないidentity意図だけを明示宣言する(論文§3.2〜3.4)。
- Featureの分割・統合は別のlifecycle/derivation eventを必要とするため、本ADRの対象外とする。

## Acceptedへ変更する条件

- identity eventとmigration manifestのJSON Schema・配置、および派生Registryのcache format・keyを確定する。
- root発行、`previous_identity_event_uid`、解決eventの`previous_identity_event_uids`、branch divergenceと競合解決、時刻やfilesystem順に依存しないcanonical replay規則を確定する。
- 複数ファイルidentity operationのためのcrash-recovery protocolを独立した設計ゲートとして確定する。transaction intent、staging、単一commit point、lock、flush/durability、未commit操作の破棄、commit後の冪等なroll-forward、recovery中の通常処理拒否、Windows/Unixの保証差を含める。
- UID発行・rename・retire・restore・release・reissue・migrationが上記protocolへ渡すmutation planと、各operationの論理commit境界を詳細設計する。
- 全書き込み段階とrecovery段階でのprocess killを注入するtestを用意し、再起動後に旧状態またはcommit済み新状態へ収束して中間状態が通常処理から観測されないことを検証する。
- legacy ChangeEvent、TestCase、executionをUIDへ解決する規則をgolden fixtureで検証する。
- 全consumerをUIDベースへ移行する実装順序と、一時的な互換adapterの削除条件を決める。
- `EntityKind`、`EntityUid`、`EntityId`、`IdentityHeader`、`IdentityMutation`、`IdentityEvent`、`IdentityEngine` Interfaceを確定し、発行・rename・lifecycle・replay・migrationの規則が要素種別ごとに重複しないことを設計レビューで確認する。
- 要素種別間の差を`EntityDescriptor`または薄いAdapterへ限定し、全種類へ同じidentity contract test suiteを適用する具体的なtest構造を確定する。
- Rust domain typeをschemaの単一の正準情報源とする生成経路、配布schema・init schemaとの同期検証、CIで片側だけの変更を拒否する方法を確定する。
- 共通基盤、Feature vertical slice、残る4種類、全要素migration、schema version 2公開cutoverという実装順序と、Feature-only形式を公開しないためのgateを実装checklistへ落とす。
- Acceptedへ変更する前に、日本語・英語の論文が最終設計と同期していることを確認する。Gitを唯一の永続化境界とし、Knowledgeとidentity eventをリポジトリ内の正準データ、Registryを破棄可能なcacheとすること、実装状況表が現行実装と本Proposed設計を分離すること、identity lifecycle因果graphと将来の永続`derived_from` Version DAGを別概念とすることを含む。CLI manual・schema・exampleへの影響一覧は別途確定・反映する。
- `release` eventの実行条件・権限・監査要件を確定する。
- `case_uid`と`change_event_uid`のdomain separator、canonical encoding、algorithm/versionを確定する。
- 全履歴監査モジュールのインターフェース、`changes compute`等の中核パスからの分離境界、およびcommandが狭い2-snapshot監査境界をどう表示するかを確定する。

## 将来の再検討条件

- Featureの分割・統合、Knowledge要素間の1対多・多対1の同一性継承が必要になった場合、identity eventへderivation relationを追加する。
