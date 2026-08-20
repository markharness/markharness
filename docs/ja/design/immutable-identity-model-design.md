# 不変Identityモデル：実装設計仕様

**Status**: 設計確定(未実装)
**関連ドキュメント**: [decisions/0013-immutable-identity-model.md](../decisions/0013-immutable-identity-model.md)(以下「ADR 0013」)、[テスト知識管理のGit-nativeモデル_統合版.md](../テスト知識管理のGit-nativeモデル_統合版.md)
**対象読者**：`markharness`の実装者

**位置づけ**：本資料は、ADR 0013の「Acceptedへ変更する条件」に列挙された未決事項を、構造化されたヒアリング(design tree grilling)によって具体化した実装設計である。ADR 0013自体は方針(なぜ・何を)を定めるものであり、本資料は「どう作るか」を確定する。ADR本文の各条件と、本資料での決定箇所の対応は第2節の表を参照。

---

## 1. 決定の進め方

以下の順序で、後続の決定の前提になる項目から確定した。カッコ内はADR 0013「Acceptedへ変更する条件」の対応項目。

1. 実装範囲(今回は設計確定までとし、コードは書かない)
2. 新規ロジックの配置(`src/identity/`を新設し、`id_cache.rs`は下位ユーティリティとして残す)
3. `EntityKind`等の共通Interfaceのディスパッチ方式(条件8)
4. crash-recovery機構(条件3)
5. branch divergence時の解決方法(条件2の一部)
6. identity eventの配置(条件1の一部)
7. `case_uid`/`change_event_uid`のalgorithm(条件14)
8. crash-recovery中のロック機構(条件3の一部)
9. `release` eventの実行時摩擦(条件13)
10. schemaをRust domain typeから生成する経路(条件10)
11. `IdentityAuditor`のCLI面(条件15)
12. migration時のrecorded_at(条件6の一部)

## 2. ADR 0013「Acceptedへ変更する条件」との対応

| ADRの条件 | 本資料での決定箇所 |
|---|---|
| identity event/migration manifestのJSON Schema・配置、Registry cacheのformat/key | §4・§5 |
| root発行・`previous_identity_event_uid`・branch divergence・canonical replay規則 | §4・§7 |
| crash-recovery protocol(transaction intent・staging・commit point・lock等) | §6 |
| mutation planと論理commit境界 | §6.2 |
| process-kill注入テスト | §6.3(実装checklistへ反映) |
| legacy解決規則・golden fixture | §11(recorded_atのみ確定。fixture自体は実装checklistの一項目) |
| consumer移行順序・一時互換adapter | §9(vertical slice期間は非公開のため不要と判断) |
| `EntityKind`等Interface確定 | §3 |
| `EntityDescriptor`/contract test構造 | §3.3 |
| schema単一正準情報源の生成経路 | §8 |
| 実装checklistへの落とし込み | `checklist-immutable-identity-model.md` |
| 論文・CLI manual・schema・exampleへの影響一覧 | 別途(§1.4/§3.2/§3.3/§3.4/§1.3/§2.4/§8は前ADR改訂セッションで反映済み。§3.6/§7はAccepted移行時に対応) |
| `release` eventの実行条件・権限・監査要件 | §10 |
| `case_uid`/`change_event_uid`のalgorithm | §7 |
| 全履歴監査モジュールのInterface・分離境界・開示方法 | §11 |

## 3. モジュール構成

新規モジュール`src/identity/`を設ける。既存`src/id_cache.rs`(Git ref単位でのFeature id→tree SHA解決、482行)は責務が異なるため拡張せず、`identity`モジュールが内部で利用する下位ユーティリティとして残す(将来的な統合可否は別途判断)。

```
src/identity/
  mod.rs        # 公開Interface
  entity_kind.rs   # EntityKind enum、EntityDescriptor(宣言的な差分)
  event.rs         # IdentityEvent、IdentityMutation enum
  engine.rs        # IdentityEngine(検証・mutation plan生成)
  registry.rs       # Identity Registry(非commit cache)の読み書き・replay
  recovery.rs       # crash-recovery(staging・commit point・roll-forward)
  lock.rs           # アプリケーションレベルlock
  audit.rs          # IdentityAuditor(全履歴監査、`identity`コマンドのみが依存)
```

### 3.1 `EntityKind`とディスパッチ方式

`EntityKind`は次の5値の閉じたenumとする(将来利用者が追加できるものではない)。

```rust
pub enum EntityKind {
    Requirement,
    Feature,
    Behavior,
    Condition,
    ExpectedResult,
}
```

`IdentityEngine`/`EntityDescriptor`はtrait object(`Box<dyn EntityDescriptor>`)による動的ディスパッチにしない。`EntityKind`をmatchするclosed enum-dispatchのみを用いる。ADR 0013が「1種類しか実装がない振る舞いのために抽象的なSeamを増やさない」と定めているためで、5種類が今後増減しない前提とも整合する。

### 3.2 `EntityDescriptor`

種類ごとの差分(親kind、marker file名、schema名、ID policy)は`EntityDescriptor`という宣言的なデータ(構造体、trait objectではない)に閉じ込める。

```rust
struct EntityDescriptor {
    kind: EntityKind,
    parent_kind: Option<EntityKind>,
    file_name: &'static str,       // "feature.yml" 等
    schema_name: &'static str,     // "feature.schema.json" 等
}

const DESCRIPTORS: [EntityDescriptor; 5] = [ /* Requirement, Feature, Behavior, Condition, ExpectedResult */ ];
```

種類固有の読み書きが本当に異なる箇所(例:`ExpectedResult`だけ親が`Condition`である、ファイルが`expected/*.yml`のように複数形である等)だけに薄い関数を用意し、lifecycle規則(発行・rename・retire等)は複製しない。

### 3.3 契約テスト

全`EntityKind`へ同一のcontract test suiteを適用する。closed enum-dispatchの上に構築するため、`for kind in EntityKind::ALL { ... }`の形で1つのテスト関数を5種類に対して実行する構造にする。最低限、以下を種類ごとに検証する:

- UID必須(UIDなしKnowledgeはUID mode下でvalidation error)
- 重複UID・重複IDの拒否
- rename eventの生成とRegistryへの反映
- event replay結果とKnowledge YAMLの一致
- Registry cacheあり/なしで結果が等価であること
- migrationの冪等性
- crash recovery(処理途中でのprocess kill後、収束すること)

`EntityKind`に新しい値を追加した場合、`DESCRIPTORS`配列・schemaファイル・fixtureのいずれかが欠けていることを検出する網羅性テストを1本用意する(`EntityKind::ALL`の要素数と各テーブルのkeyの集合を突き合わせる)。

## 4. Identity Event

### 4.1 配置

`.markharness/identity-events/`配下を、kind・entityごとにグルーピングする。

```
.markharness/identity-events/
  features/
    01ARZ3NDEKTSV4RRFFQ69G5FAV/
      01ARZ3NDEKTSV4RRFFQ69G5FE0.yml   # issued
      01ARZ3NDEKTSV4RRFFQ69G5FE1.yml   # renamed
  requirements/
    .../
```

理由:replayの主要アクセスパターンは「あるentityの全eventを取得する」であり、フラット配置だと全event fileを走査してentity_uidでフィルタする必要が生じスケールしない。Identity Registry cache(`.markharness-cache/identities/features/<uid>.yml`)と対になる配置にすることで、両者の対応関係も見通しやすくなる。

### 4.2 イベント種別

`IdentityMutation`(= event種別)は次の7種とする。

| type | 意味 | 主なフィールド |
|---|---|---|
| `issued` | 新規UID発行 | (root。先行eventなし) |
| `renamed` | `id`変更 | `from_id`, `to_id` |
| `retired` | 削除に伴うUID退役 | - |
| `restored` | 削除済みUIDの復元 | - |
| `released` | 退役idの再利用予約解除 | `released_id` |
| `reissued` | copy/import時の新規UID発行 | `source_uid`(任意) |
| `resolved` | branch divergenceの明示的解決 | `previous_identity_event_uids`(複数)、`winning_event_uid` |

`issued`以外は先行eventを持つ。通常eventは`previous_identity_event_uid`(単数)で直前の自entity内headを参照する。`resolved`だけは`previous_identity_event_uids`(複数)で解決対象の全divergent headをjoinする。

```yaml
identity_event_uid: 01ARZ3NDEKTSV4RRFFQ69G5FE1
previous_identity_event_uid: 01ARZ3NDEKTSV4RRFFQ69G5FE0
type: renamed
entity_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
from_id: todo-management
to_id: task-management
recorded_at: 2026-08-20T12:34:56Z
```

### 4.3 replay順序

filename・`recorded_at`・filesystem列挙順・ULIDの時刻順のいずれにも依存しない。`previous_identity_event_uid`(または`resolved`の`previous_identity_event_uids`)による先行参照だけがreplay順序を決定する。独立したentityのevent graphは任意順でreplay可能で、byte-for-byteで同じ結果を生む。

同じheadを伸ばす2つのeventが両方存在するsnapshotはbranch divergenceであり、`resolved` eventなしでは曖昧性エラーとする(第7節参照)。

## 5. Identity Registry(非commitキャッシュ)

`.markharness-cache/identities/<kind>/<uid>.yml`に、対象refのidentity eventをreplayして得られる結果をmaterialized viewとして書く。既存の`.markharness-cache/<ref>.json`(id解決キャッシュ、`id_cache.rs`)と同じ設計原則に従う:

- 欠落は正常であり、読み込み時に再構築を起動する。
- 存在するキャッシュが古い・不整合な場合は静かに破棄して再構築する(content-addressed cache keyの不一致で判定)。
- 正準情報源はあくまでGit管理された`.markharness/identity-events/`であり、Registryキャッシュは削除しても同じrefのeventだけから再構築できなければならない。

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

## 6. crash-recovery機構

### 6.1 staging・commit point

新規のWAL(write-ahead log)相当機構は発明しないが、実装に着手したところ、当初案(`fs_safety::replace_dir_from_staging`による単一ディレクトリの一括差し替え)には無理があると判明した。1回のidentity operationが書き込む先(`knowledge/`配下のKnowledge YAMLと`.markharness/identity-events/`配下のevent file)は物理的に別ディレクトリであり、1回のrenameで両方を同時に確定させることはできない。

代わりに、identity eventのモデルそのものが持つ「Knowledge YAMLはevent replayの投影である」という性質(第2節)を利用し、真に単一原子操作が必要な箇所を1点だけに絞る。

1. `.markharness/.identity-staging/<operation-id>/`に`intent.yml`(対象entity・目的のmutation種別を記録するtransaction intent)を、`fs_safety::create_new_no_follow`で最初に書く。これは「operationを開始した」ことの耐久な証拠である。
2. **単一の論理commit point**:新規identity eventファイルを、その最終配置(`.markharness/identity-events/<kind>/<uid>/<event_uid>.yml`)へ`fs_safety::replace_file`(既存の単一ファイル原子書き込み)で直接書く。このファイルが存在するかどうかだけが「operationが成立したか」を決める。
3. Knowledge YAML(例:`feature.yml`の`id:`)を、replay結果から決定的に導出される内容で`replace_file`により更新する。これは冪等なroll-forwardであり、同じ内容を再度書いても副作用はない。
4. Registryキャッシュを無効化(削除)する。
5. `.identity-staging/<operation-id>/`を削除し、operation完了を示す。

起動時recoveryスキャンは、残っている`.identity-staging/<operation-id>/`ごとに次を行う:

- `intent.yml`が指すidentity eventファイルが最終配置に**存在しない**場合:手順2に到達していない(commit pointより前)。旧状態のままであり、`.identity-staging/<operation-id>/`を削除するだけでよい。
- 存在する**場合**:commit pointは完了している。手順3・4(Knowledge YAMLのroll-forward、Registryキャッシュ無効化)を冪等に再実行してから、`.identity-staging/<operation-id>/`を削除する。

この設計により、「1回のrenameで全てを確定する」という当初案より単純に、「identity eventファイルの存在だけがoperationの成立を決め、他は全てそこから決定的に再導出できる」という不変条件で crash-recovery を実現できる。

### 6.2 mutation planと論理commit境界

`issued`/`renamed`/`retired`/`restored`/`released`/`reissued`/`resolved`のいずれも、上記手順(intent書き込み→commit-point eventのatomic write→projectionのroll-forward)に従う同一の枠組みで扱う。operation種別ごとの差はreplayから得るprojectionであり、commit-point機構自体は共通化する。project-wide migrationは§12のbatch形式を用いる。永続intentに予定する全issued eventを含め、最初のeventを単一の論理commit pointとして、recoveryが残りを完了する。

### 6.3 process-kill注入テスト

各operation種別について、「commit pointの直前」「rename実行中(OS側の原子性に守られる区間)」「commit pointの直後」の3ポイントでプロセスを強制終了させ、次回起動時のrecoveryが必ず「旧状態」または「commit済み新状態」のいずれかに収束することを検証するテストを、実装checklistの一項目として用意する。

## 7. branch divergenceの解決

2つのブランチが同一entityに対して独立にidentity eventを生成し、マージ時にdivergent head(同じ先行eventを共有する2つの後続event)が両方存在する状態になった場合、custom merge driverによる自動解決は行わない。

- 通常の`git merge`はそのまま行わせる。
- `markharness validate`(および`changes compute`等の中核パス)は、divergent headを検出すると曖昧性エラーで処理を止める。
- 人間が`markharness identity resolve <entity-uid>`を実行する。このコマンドは、divergent headのうちどちらを正とするか(または新たな`id`を指定するか)を引数で受け取り、`resolved` event(`previous_identity_event_uids`に両headのevent UIDを列挙)を新規発行して解決する。

merge driverを使わない理由:各開発者のローカル環境ごとの個別登録が必要でテストしにくく、「cloneすればそのまま動く」というGitの前提を壊すため。本プロジェクトは`rename-id`など同一性に関わる操作を常に明示的なCLIコマンドとして扱っており、競合解決もこの方針に揃える。

## 8. `case_uid`/`change_event_uid`のalgorithm

いずれも標準のUUIDv5(RFC 4122、SHA-1ベース)を用いる。`uuid`クレート(既存依存があれば流用、なければ追加時にライセイン確認)のv5生成をそのまま利用し、独自のhash方式は設計しない。

- `case_uid`:namespace UUID + `requirement_uid`・`feature_uid`・`behavior_uid`・`condition_uid`・`expected_result_uid`の集合(canonical順に整列・連結)をnameとして導出。
- `change_event_uid`:namespace UUID + domain separator・identity canonicalization/algorithm version・from/to snapshot identity・`feature_uid`・canonical change payload・明示optionsをcanonical encodingで連結したものをnameとして導出。

## 9. release eventの実行摩擦

`markharness identity release <uid> <old-id>`は、`rename-id`と同様に確認フラグなしでそのまま実行できるコマンドとする。本プロジェクトは同一性に関わる操作の監査証跡をコマンド実行自体とGit差分・identity eventに委ねる方針で一貫しており、`release`だけを特別扱いしない。取り消し可能な操作(再度別のUIDへ発行し直せば実質的に取り消せる)である点も踏まえた。

## 10. schemaの単一正準情報源

`schemars`等のコード生成クレートは追加しない。既存通り`schema/*.schema.json`(および`.markharness/schema/`ミラー)を手書きで維持し、Rust構造体のフィールド集合とJSON Schemaの`properties`が一致することを検証するテストを新設する。新規クレード追加によるライセイン確認・保守負担を避け、既存の`schema::validate_yaml`の枠組みとも自然に統合できるため。

`IdentityHeader`(`uid`・`id`・`kind`)を含む5要素の構造体・schemaファイルは、この一致検証テストの対象に含める。

## 11. `IdentityAuditor`のCLI面

全履歴監査は、既存`changes`系コマンド群とは独立した新規トップレベルコマンド`markharness identity audit`とする。`IdentityAuditor`はGit commit history全体を走査する重い処理であり、`changes compute`等の「2ref間の軽量比較」とは実行コストの性質が異なるため。

`changes compute`・`verify`等、2-snapshot比較にとどまる中核パスのJSON出力には`audit_scope: "two_snapshot"`のような機械可読フィールドを持たせ、狭い監査境界をCIゲート等から検知できるようにする。ドキュメント記載のみでは自動検知できないため。

## 12. migrationにおける`recorded_at`

`markharness identity migrate`が既存要素へ初期発行eventを付与する際、migration operationのUTC開始時刻を一度だけ取得し、全eventの`recorded_at`へ同じ値を使う。CLIがworking treeの変更を準備する時点では、後にそれを記録するGit commitはまだ存在せず、そのcommit時刻を正直な入力として利用できない。`git log --follow`等による実際の初回commit時刻の遡及推定も行わない。UID導入以前は`id`変更に追従できない(ADR 0013の出発点そのもの)ため、rename前の履歴を`--follow`で正しく遡れる保証がなく、不正確な値を正確であるかのように見せるリスクがある。共通のoperation開始時刻により「本当の作成時刻は不明であり、追跡を始めた時点を記録している」ことを正直に表す。

予定するUID/event割当の全体を、最初のeventがfinal pathへ到達する前に一つの永続batch intentへ記録する。最初のeventをbatch全体の論理commit pointとする。その後にcrashした場合、startup recoveryは通常commandを再開する前に、残る全eventを書き、全Knowledge projectionをroll-forwardする。partial migrationを正規状態として公開しない。`identity migrate --dry-run`はlock、staging、event、Knowledge fileを書かず、予定するUID割当を表示する。

## 13. 実装順序

ADR 0013の「移行」節が定める順序をそのまま踏襲する。

1. 共通Identity Module(本資料§3〜§6)とcrash-recovery機構
2. Featureを使ったend-to-endのvertical slice(公開・永続サポートしない内部段階)
3. 残る4種類(Requirement/Behavior/Condition/ExpectedResult)のdescriptor/adapter
4. 全要素のmigration
5. schema version 2の公開cutover(5種類を一括切替)

vertical slice段階(2)では何も公開しないため、一時的な互換adapterは不要と判断した(ADR条件「全consumerをUIDベースへ移行する実装順序と、一時的な互換adapterの削除条件」に対応)。

具体的なタスク分解は`checklist-immutable-identity-model.md`を参照。
