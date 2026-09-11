# markharness v2 設計書

作成日：2026-09-11(初版)。2026-09-11、既存の設計・概念に引っ張られない再検討(grillingセッション)により全面書き直し。同日、既存実装(`src/`)との突合結果を反映して§5.2.1・§6.1・§9.1等を訂正。
状態：設計提案。以下の型、CLI、MVP仕様はv2への提案であり、実装済み仕様ではない。

## 1. 結論と製品の命題

開発者が機能・仕様(Requirement)・テストケースのいずれかを変更したとき、チームは次の4つの答えを手にする。

1. どの機能に関連するテストケース、どの要件に関連するかを確認する。
2. テストケースの修正漏れ、仕様の修正漏れを検知する。
3. 前回リリースにおいて、どのテストが検証スコープに入っていたかを確認し、判断の一助とする。
4. 今回のリリースにおいて、どの機能が影響するかの判断に使う。

これがv2のNorth Starであり、以下すべての設計判断はこの4点に照らして評価する(用語集は[markharness-v2-glossary.md](markharness-v2-glossary.md)、確定した用語の一次情報は[CONTEXT.md](../../../CONTEXT.md)を参照)。

成功の判定基準は次の3点。

- レビュー時に「テストが足りない」と気づく速度が上がる。
- 本番障害のうち「テストが古いまま見逃された」パターンが減る。
- Excel/Wiki等での手作業トレーサビリティ確認の工数が減る。

### 1.1 前版からの主な変更

本書は2026-09-11の同日中に、既存の設計・概念(v2旧版の`Artifact`/`Evidence`/`Execution Manifest`等の重量級モデル、旧実装の`identity`退役・復元機構)に引っ張られない前提で作り直したものである。旧版との主な差分は次の通り。

| 領域 | 旧版 | 本版 |
|---|---|---|
| 実行結果・証跡 | Evidence/EvidenceSelection/Execution Manifest/Implementation revision/Environment matrixの重量級モデル | `automated`/`manual`の1軸＋任意の参照文字列のみ([0020](../decisions/0020-execution-status-lightweight-model.md)) |
| 対応確認 | `AlignmentObligation`/`AlignmentDecision`という独立Domain型 | commit trailerによる軽量な記録([0019](../decisions/0019-alignment-check-commit-trailer.md)) |
| 同一性(退役・復元) | 退役・復元・ID予約解除の厳密な保証機構を維持 | 「退役＝削除」まで単純化([0021](../decisions/0021-identity-retire-simplification.md)) |
| 外部連携(StrictDoc/Playwright) | MVP必須(M0〜M4すべてに組込み) | MVP範囲外。データ構造だけ将来の連携を意識する |
| 構造プロファイル | 複数profileの切替機構(minimal-trace-v1/hierarchical-test-v1) | 単一構造(Feature→Behavior→Scenario→TestCase)のみ。profile切替は導入しない |
| GUI/dashboard | MVPから除外(Stage 3として後回し) | CLI/JSON出力のみで完結させ、ビューは別ツールに委ねる。ADR 0008 Stage 3で実装済みの`server.rs`・`ui/`は削除する([0022](../decisions/0022-remove-stage3-dashboard.md)) |
| Requirementの正本 | markharness nativeの実体のみ(`label`/`description`/`axis`) | native/externalの二モード。externalではStrictDocが正本で固定参照のみ保持([0023](../decisions/0023-requirement-native-and-external-source.md)、§5.2.1) |

## 2. 根拠

一次資料は2026-09-11の再設計grillingセッション(この会話)。根拠となる回答は次の通り。

| 論点 | 確定内容 |
|---|---|
| 再設計の対象範囲 | 既存の`knowledge/`コア(Feature/Behavior/Scenario/Axis/決定的生成/ChangeEvent)を含め全面見直し可としたが、結果としてこのコアは維持する判断に至った |
| 「機能」の実体 | 仕様上の概念(現行Feature相当)。実装コードの差分検知はmarkharnessの責務外 |
| 仕様の正本 | StrictDoc(`.sdoc`、Git管理)。JSON export取込・自前パーサはロードマップ扱い |
| テストケースの正本 | 現行`knowledge/`資産を維持 |
| 修正漏れ検知 | 仕様⇄テストケース双方向。自動判定＋commit trailerでの軽量な確認記録 |
| リリース単位 | PR base/head差分(日常)＋リリース全体一覧(判断補助)の両方 |
| 実行結果 | 簡易ステータス(automated/manual)のみ保持。証跡管理は別ツール(現状Excel)の責務 |
| 環境(ブラウザ/OS等) | テストケースの外。仕様書/実行ツール側の責務 |
| identity | 単純化。退役後の厳密な復元・ID予約解除は保証しない |
| 後方互換 | 不要(実利用者はごく少数のプロトタイプ段階、[0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md)背景と同じ前提) |

## 3. 設計原則

| ID | 原則 | 設計への帰結 |
|---|---|---|
| P1 | 外部の正本を尊重する | StrictDocの要件本文を複製・編集せず、固定参照(id・版)のみ保持する |
| P2 | 同一性と版を分ける | UIDを名前・パス・内容ハッシュから無条件に作らない(現行`knowledge/`の方針を維持) |
| P3 | 判定は再現可能にする | 同一Git snapshotから同一Change Impact/Release Coverageを作る |
| P4 | 合否と検証手段を分ける | pass/fail等の詳細判定は別ツールに委ね、markharnessは「自動/手動どちらの手段で検証するか」とその参照先だけを扱う(§5.2) |
| P5 | Coreは外部形式を知らない | StrictDoc固有フィールドと変換規則は将来のAdapterに置き、Coreへ持ち込まない |
| P6 | 拡張は命題から評価する | 汎用plugin基盤・独自業務管理を先行実装しない([CLAUDE.md](../../../CLAUDE.md)のYAGNI原則) |
| P7 | 判定は一箇所に集約する | CLI・CIは同じApplication結果を使う(現行[0008](../decisions/0008-verification-plan-product-roadmap.md)のモジュラーモノリス方針を継承) |

## 4. 責務境界

| 領域 | 正本／担当 | markharnessが保持するもの |
|---|---|---|
| 仕様(Requirement)の本文・構造 | external: StrictDoc / native: markharness | externalは固定参照(id・版)のみ、nativeは`label`/`description`([0023](../decisions/0023-requirement-native-and-external-source.md)) |
| テストケースの意図・手順・期待結果 | markharness native(`knowledge/`) | Feature/Behavior/Scenario/TestCaseそのもの(既存資産を維持) |
| 実行、証跡本体、日時、実行者 | 別ツール(現状Excel、将来Playwright等) | `automated`/`manual`の1軸＋任意の参照文字列のみ |
| 実行環境(ブラウザ/OS等) | 仕様書・実行ツール側 | 保持しない |
| 対応確認の記録 | Gitのcommit履歴(trailer) | 確認要否の自動判定のみ。記録本体はGit履歴に委ねる |
| Change Impact・Release Coverageの算出 | markharness Core | 決定的な差分・一覧算出 |

## 5. ドメインモデル

### 5.1 中核概念(既存維持)

Feature・Behavior・Scenario・TestCase・Axis・Case revision・ChangeEventは現行`knowledge/`実装をそのまま維持する。決定的生成、Git tree SHAによる版比較、Axisによる横断検索は今回の再設計の対象外であり、変更しない。

なお現行実装はRequirementも`knowledge/requirements/<id>/requirement.yml`としてnativeな実体(独自UID・`label`・`description`・`axis`)で保持し、`feature.requirement_uids`で多対多に関連付けている([0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md)§1・§3、`src/knowledge.rs`・`src/identity/entity_kind.rs`・`src/traceability.rs`)。Requirementはcase identityには含まれない(`case_id = tc-{feature}-{behavior}-{scenario}`、`src/generate.rs`)。したがって§5.2の`Requirement`は新規概念の追加ではなく、**既存のnative Requirementに「正本を外部に置くモード」を足す変更**である([0023](../decisions/0023-requirement-native-and-external-source.md)、§5.2.1)。StrictDocを導入していない運用でもmarkharnessは単独で成立する必要があるため、正本を常に外部へ固定する設計は採らない。

### 5.2 新規概念

```text
Requirement {
  id,                    // 表示ID(現行`requirement.yml`の`id`)。externalではStrictDoc側UIDと一致させる
  uid,                   // 現行のRequirement UID(ADR 0013)を維持する
  source: native | external,   // 既定はnative
  axis,                  // 両モードで保持する。markharness自身の分類であり外部正本の複製ではない

  // source = native のとき必須、externalでは書けない
  label,
  description,           // optional

  // source = external のとき必須、nativeでは書けない
  source_locator,        // 同一Gitリポジトリ内の`.sdoc`パス
  source_revision,       // 取込時に固定したGit blob OID
}

ExecutionStatus {
  case_uid,              // 表示IDではなくCase UIDで参照する(ADR 0013、rename耐性)
  mode: automated | manual,
  reference: string,     // optional。テストコードへのパスやURL
}
```

`source: external`の`Requirement`はStrictDoc側の内容を複製しない。markharnessが保持するのは固定参照だけであり、本文・受け入れ条件等はStrictDoc側を都度参照する(P1)。`source: native`では従来どおりmarkharnessが`label`/`description`の正本を持つ。両方のフィールドを併せ持つ、あるいはどちらも欠く`requirement.yml`は`validate`で拒否する([0023](../decisions/0023-requirement-native-and-external-source.md))。

FeatureからRequirementへの多対多関連は、新しい`ContributesTo`型・格納先を作らず、現行の`feature.requirement_uids`をそのまま用いる(正本はFeature側、逆方向一覧は派生。[0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md)§1・§3)。「実現に寄与する」ことを示すのみで、検証済みの証明ではない。既存フィールドで足りるため新規型を作らないのはP6(YAGNI)に従う判断である。

`ExecutionStatus`は§1.1・[0020](../decisions/0020-execution-status-lightweight-model.md)の通りTestCase単位の最小限の記録である。これは「実行された事実」ではなく**検証手段(自動/手動)とその参照先**を表す。合否・日時・実行回数を持たないため、値の存在を「最新版で実行済み」と読んではならない。

### 5.2.1 現行native Requirementとの関係

| 項目 | 現行 | 本版 |
|---|---|---|
| `source` | 無し | 追加。省略時は`native`とみなす |
| `requirement.yml`の`label`/`description` | markharnessが本文相当を保持 | nativeでは維持。externalでは書けない(P1。表示名が必要になった時点でM3のStrictDoc Adapterが都度取得する) |
| `source_locator`/`source_revision` | 無し | externalで必須、nativeでは書けない。欠落・混在は`validate`で拒否する |
| `axis` | 保持 | 両モードで保持(markharness自身の分類であり外部正本の複製ではない) |
| `uid`・`feature.requirement_uids` | ADR 0013のUID・多対多関連 | そのまま維持 |
| 対話作成フロー(`src/interactive.rs`・`knowledge_draft.rs`)のRequirement入力 | `label`/`axis`を入力 | nativeはそのまま。externalを選んだ場合のみ`source_locator`入力へ切り替える(AC02と整合させるため) |
| `traceability.rs`のRequirement索引・`GeneratedFrom.requirement_ids`/`requirement_uids` | 実装済み | 維持 |

既存`requirement.yml`は`source`省略=nativeとしてそのまま有効であり、変換は不要である。externalへ移す場合は人が書き直す(§2の後方互換不要方針により自動変換は作らない)。

### 5.3 対応確認(Alignment check)

Requirementの意味変更、またはTestCaseの実効内容変更を検知した際、関連する他方(TestCaseまたはRequirement)が同じGit差分区間内で更新されたか、あるいは`Spec-Reviewed: no-change-required`のようなcommit trailerで「変更不要」と明示的に確認されたかを判定する([0019](../decisions/0019-alignment-check-commit-trailer.md))。いずれでもない場合は「未確認」として一覧に含める。独立した承認ワークフローは作らない。

trailerは対象要素を識別できる形にする(例: `Spec-Reviewed: no-change-required (req-login-01)`)。1つのコミットが複数のRequirement/TestCaseに触れる場合、対象を持たないtrailerではどの対応確認が済んだのか判定できない(具体的な書式は[0019](../decisions/0019-alignment-check-commit-trailer.md)の通り実装設計で確定する)。またsquash mergeされたPRではtrailer行がmerge commit本文の途中に埋め込まれ得るため、判定は`git log base..head`の各コミット本文を走査する実装とし、末尾行だけを見る実装にしない。

## 6. Change ImpactとRelease Coverage

### 6.1 Change Impact(PR単位)

base/head間のFeature版比較(現行`changes.rs`の`ChangeEvent`計算を流用)に加え、次を行う。

1. 変更されたFeatureに`contributes_to`するRequirementを特定する。
2. 仕様側が変更されたかを、Requirementのモードに応じて判定する。
   - `source: native`：`requirement.yml`自体のbase/head差分で判定する。粒度はRequirement単位で、外部ツールを必要としない。
   - `source: external`：固定参照`source_revision`とhead時点の`source_locator`のblob OIDを比較する。`.sdoc`が**markharnessと同一のGitリポジトリで管理されている**ことを前提とし、`.sdoc`の構文解析を必要としない。粒度はファイル単位であり、同一ファイル内の別Requirementの変更でも「変更あり」と判定される(偽陽性を許容する。Requirement単位の粒度が必要になった時点でM3の`.sdoc`解析へ引き上げる)。
3. 変更されたTestCase・Requirementそれぞれについて、Alignment checkの状態(確認済み/未確認)を算出する。
4. 影響を受けるTestCase一覧、関連Requirement一覧、未確認のAlignment checkを出力する。

この方式により、Change Impact(M1)は`.sdoc`パーサ(M3)にも、StrictDocの導入有無にも依存しない。externalモードでは、確認後に固定参照を新しいblob OIDへ更新する操作(§7の`requirement repin`)が必要であり、これを行わない限り同じRequirementが以降の差分でも「変更あり」と報告され続ける。

### 6.2 Release Coverage(リリース単位)

指定したRequirement/Feature集合全体について、次を一覧化する。

- 各TestCaseに`ExecutionStatus`が存在するか、`mode`は何か。
- 各Requirementに`contributes_to`するFeatureが存在するか(coverage gap)。

Change Impactが「今回の差分で何が変わったか」を示すのに対し、Release Coverageは「リリース対象全体を取りこぼしなく見渡せるか」を示す補助情報であり、リリース判断時にChange Impactと併用する。

Release Coverageは指定したGit ref(既定はHEAD)の内容で評価する。§1の問い3(前回リリースでどのテストが検証スコープに入っていたか)は、リリースtagを`--at`に渡して当時のKnowledgeと`ExecutionStatus`を評価することで答える。`ExecutionStatus`自体は日時・リリース番号を持たないため、時点の指定はGit refに委ねる(P3)。

## 7. CLI案

```text
markharness requirement link --feature <feature-id> --requirement <requirement-id>
markharness requirement unlink --feature <feature-id> --requirement <requirement-id>
markharness requirement repin --requirement <requirement-id>   # externalのみ。source_revisionをhead時点のblob OIDへ更新
markharness execution set --case-uid <case-uid> --mode automated --reference src/tests/login.spec.ts
markharness execution set --case-uid <case-uid> --mode manual
markharness impact --base <ref> --head <ref> --format json
markharness coverage --requirements <requirement-ids-or-all> --at <ref> --format json
```

`requirement link`/`unlink`は`feature.yml`の`requirement_uids`を編集するコマンドであり、新しい格納先は作らない(§5.2)。出力はCLI/JSONのみとし、ローカルサーバーやダッシュボードはMVPに含めない(§8)。終了コード・JSON schemaのversioning方針は実装時に確定する。廃止するCLIは§9.1で扱う。

## 8. 非目標

- StrictDoc要件編集UI、独自要件承認workflow。
- テストケースCRUDの新規UI(現行`knowledge/`編集フローを維持するのみ)。
- 実行結果の詳細管理(pass/fail・証跡本体・実行環境matrix)。別ツールの責務とする。
- Playwrightコード生成、実行エンジン、CI連携の自動化。要望が出た時点で改めて設計する。
- StrictDoc `.sdoc`の自前パーサ・JSON export取込の自動化(ロードマップ扱い、§2)。
- 退役後の厳密な同一性保証(復元・ID予約解除)([0021](../decisions/0021-identity-retire-simplification.md))。
- ダッシュボード、SaaS、RBAC/SSO、共有DB(既存のStage 3 dashboardも削除する。[0022](../decisions/0022-remove-stage3-dashboard.md))。

## 9. 既存実装からの再利用・置換・廃止

| 現行資産 | 判断 | 理由 |
|---|---|---|
| `knowledge/`一式(Feature/Behavior/Scenario/Axis、決定的生成) | 維持 | North Starの前提資産。§5.1 |
| `changes.rs`(ChangeEvent計算) | 維持・拡張 | Change Impactの基盤としてそのまま使う。§6.1 |
| `case_definition.rs`(Case revision固定保存) | 維持 | 既に[0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md)§1〜4相当を実装済み |
| `execution.rs`(target_revision/environment等) | 縮小 | [0020](../decisions/0020-execution-status-lightweight-model.md)の`ExecutionStatus`へ置き換え |
| `plan.rs`(Evidence適用可能性判定) | 縮小・置換 | 厳密な突合ロジックは不要。ExecutionStatusの有無参照へ簡略化 |
| `src/identity/`(retire/restore/release/reissue部分) | 縮小 | [0021](../decisions/0021-identity-retire-simplification.md) |
| `src/identity/`(UID発行/rename部分) | 維持 | [0021](../decisions/0021-identity-retire-simplification.md)の対象外 |
| `src/git.rs`・`fs_safety.rs` | 維持 | 不変ref読出し・原子的操作は今回の変更と独立 |
| `src/canonical.rs`(ImportSourceArg等) | 見直し | StrictDoc取込は将来別Adapterとして再設計。現行のNative/Junit importerとの関係は着手時に整理 |
| `knowledge/requirements/`(native Requirement実体) | 縮小・意味変更 | 本文相当(`label`/`description`)を廃止し固定参照へ。§5.2.1 |
| `src/traceability.rs`(Requirement索引) | 維持 | Requirement⇄TestCaseの逆引きは既存実装をそのまま使う |
| `src/server.rs`・`ui/`・`markharness serve`(ADR 0008 Stage 3のdashboard) | 廃止 | 現行UIは`plan`/evidence出力に依存し、plan縮小と同時に壊れる([0022](../decisions/0022-remove-stage3-dashboard.md))。§9.1 |
| `src/milestone.rs`・`src/backfill.rs` | 要判断 | base/head指定のChange Impactへ統合できるかを着手時に判定する。§9.1 |
| `src/verify.rs`・`audit_scope.rs`・`derived_index.rs`・`lineage.rs` | 棚卸し対象 | 本書では未分類。M0着手時に維持/縮小/廃止を確定する |
| `identity` CLIの`retire`/`restore`/`release`/`reissue` | 廃止 | [0021](../decisions/0021-identity-retire-simplification.md)。既存イベントログの扱いは§9.1 |

### 9.1 既存CLI・既存データ・既存UIの扱い

- **廃止するCLI**：`identity retire`/`restore`/`release`/`reissue`([0021](../decisions/0021-identity-retire-simplification.md))と、`plan`・`execution record`のEvidence系オプション([0020](../decisions/0020-execution-status-lightweight-model.md))。削除範囲は実装時のチェックリストで確定する。
- **既存データ**：`.markharness/executions/`配下の既存実行記録と、`retire`/`release` eventを含む`.markharness/identity-events/`は自動変換しない(§2)。replay時は廃止したevent種別を警告付きで無視する方針とし、旧eventの存在だけで実行を失敗させない。
- **既存dashboard**：`src/server.rs`・`ui/`・`markharness serve`・frontendのbinary同梱を削除する([0022](../decisions/0022-remove-stage3-dashboard.md))。削除は`plan`縮小と同じタイミングで行い、`tests/server.rs`等の関連テストも同時に削除する。リポジトリ外のviewerが`plan`出力を参照している場合は、Change Impact/Release Coverage出力への切替が必要になる。

## 10. ロードマップ

| 段階 | 作るもの | 完了条件 |
|---|---|---|
| M0 | `Requirement`の新schema(native/externalの二モード)・`ExecutionStatus`のschema、`feature.requirement_uids`による関連付け、CLI(§7)、Alignment check(§5.3)の自動判定、対話作成フローの更新(§5.2.1) | native運用(StrictDocなし)とexternal運用の双方でFeature⇄Requirementの対応とTestCaseのExecutionStatus記録がGit/CLI経路で完結し、モードの混在した`requirement.yml`が拒否される |
| M1 | Change Impact(§6.1) | PR base/head間で影響Feature・Requirement・未確認Alignment checkを一覧できる(`.sdoc`解析=M3に依存しない) |
| M2 | Release Coverage(§6.2) | 指定Requirement集合全体のcoverage gapを一覧できる |
| M3(将来) | StrictDoc `.sdoc`取込(Git管理された要件の実体反映) | 需要確認後に着手。自前パーサの要否を含め別途設計する |
| M4(将来) | Playwright連携(自動実行結果の取込) | 要望が出た時点で着手。§9の`ExecutionStatus`データ構造を前提に設計する |

MVPはM0〜M2とする。M3・M4は本書の時点では着手を約束しない。

## 11. 受け入れ条件

| ID | シナリオ | 期待結果 |
|---|---|---|
| AC01 | FeatureをRequirementへ`contributes_to`で関連付ける | 関連はFeature側が正本を持ち、逆引き一覧は派生する |
| AC02 | Requirementの内容をmarkharnessから編集しようとする | 拒否する。markharnessはRequirementの固定参照のみ保持する |
| AC03 | Requirementが変更されたのに関連TestCaseが更新されていない | Change Impactの出力で「未確認」として明示する |
| AC04 | TestCase変更コミットに`Spec-Reviewed: no-change-required`が付与されている | Alignment checkは「確認済み」と判定する |
| AC05 | TestCaseに`ExecutionStatus(mode=manual)`を記録し、日時や実行者は渡さない | 記録が成立する。日時・実行者フィールドは存在しない |
| AC06 | 同一入力から複数回Change Impact/Release Coverageを計算する | 同じ出力を再現する(P3) |
| AC07 | 退役(削除)したTestCaseと同内容のTestCaseを再度追加する | 新規の別TestCaseとして扱われ、旧UIDは引き継がれない([0021](../decisions/0021-identity-retire-simplification.md)) |
| AC08 | Requirementに`contributes_to`するFeatureが一つもない | Release Coverageでcoverage gapとして一覧される |
| AC09 | `source: external`なのに`source_locator`/`source_revision`を持たない`requirement.yml`を置く | `validate`が拒否する(§5.2.1) |
| AC09b | `source`を省略した既存の`requirement.yml`(`label`あり)をそのまま置く | nativeとして有効。StrictDocなしでChange Impact/Release Coverageが動作する([0023](../decisions/0023-requirement-native-and-external-source.md)) |
| AC09c | `label`と`source_locator`を両方持つ`requirement.yml`を置く | `validate`が拒否する(モード混在) |
| AC10 | `source: external`のRequirementで、`.sdoc`のblobがhead時点で固定参照と異なる | Change Impactが仕様側変更として検出する。`.sdoc`の構文解析は行わない(§6.1) |
| AC10b | `source: native`のRequirementの`label`/`description`をbase/head間で変更する | Change Impactが仕様側変更として検出する(§6.1) |
| AC11 | 過去のリリースtagを`--at`に指定してRelease Coverageを算出する | 当時のKnowledge・ExecutionStatusに基づく一覧を再現する(§6.2) |
| AC12 | 1コミットで複数のRequirementに触れ、対象を書かないtrailerを付与する | どの対応確認が済んだか判定できないため「未確認」のまま残る(§5.3) |
| AC13 | Scenarioの表示idをrenameする | `ExecutionStatus`はCase UID参照のため維持される(§5.2) |
