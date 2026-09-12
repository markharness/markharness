# markharness v2 設計書

作成日：2026-09-11(初版)。2026-09-11、既存の設計・概念に引っ張られない再検討(grillingセッション)により全面書き直し。同日、既存実装(`src/`)との突合結果を反映して§5.2.1・§6.1・§9.1等を訂正。さらに、StrictDoc→markharness→Playwrightの実運用後に完全モデルへ進めるための契約を§9.2と[ADR 0025](../decisions/0025-v2-forward-compatible-evolution.md)へ追加した。
状態：設計提案。以下の型、CLI、MVP仕様はv2への提案であり、実装済み仕様ではない。

## 1. 結論と製品の命題

開発者が機能・仕様(Requirement)・テストケースのいずれかを変更したとき、チームは次の4つの答えを手にする。

1. どの機能に関連するテストケース、どの要件に関連するかを確認する。
2. テストケースの修正漏れ、仕様の修正漏れを検知する。
3. 前回リリースにおいて、どのテストが検証スコープに入っていたかを確認し、判断の一助とする。
4. 今回のリリースにおいて、どの機能が影響するかの判断に使う。

問い3についてmarkharnessが答えるのは、そのリリースの`ReleaseScope`(選定リスト)が記録されていれば「何を検証対象に選んだか」まで、無ければ「その時点で登録されていたTestCaseと検証手段」までである。いずれの場合も実行された事実は扱わない(§6.2)。これがv2のNorth Starであり、以下すべての設計判断はこの4点に照らして評価する(用語集は[markharness-v2-glossary.md](markharness-v2-glossary.md)、確定した用語の一次情報は[CONTEXT.md](../../../CONTEXT.md)を参照)。

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

ExecutionBinding {
  case_uid,              // 表示IDではなくCase UIDで参照する(ADR 0013、rename耐性)
  mode: automated | manual,
  reference: string,     // optional。テストコードへのパスやURL
}

ReleaseScope {
  release_id,            // リリースの表示名(Git tag名を推奨)。安全な単一パス構成要素に限る(下記)
  case_uids: [case_uid], // そのリリースで検証対象に選んだTestCase
}
```

`source: external`の`Requirement`はStrictDoc側の内容を複製しない。markharnessが保持するのは固定参照だけであり、本文・受け入れ条件等はStrictDoc側を都度参照する(P1)。`source: native`では従来どおりmarkharnessが`label`/`description`の正本を持つ。両方のフィールドを併せ持つ、あるいはどちらも欠く`requirement.yml`は`validate`で拒否する([0023](../decisions/0023-requirement-native-and-external-source.md))。

FeatureからRequirementへの多対多関連は、新しい`ContributesTo`型・格納先を作らず、現行の`feature.requirement_uids`をそのまま用いる(正本はFeature側、逆方向一覧は派生。[0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md)§1・§3)。「実現に寄与する」ことを示すのみで、検証済みの証明ではない。既存フィールドで足りるため新規型を作らないのはP6(YAGNI)に従う判断である。

`ReleaseScope`は「そのリリースで何を検証対象に選んだか」だけを記録する([0024](../decisions/0024-release-scope-selection-list.md))。選定日時・担当者・承認状態・合否は持たず、内容は人がCLIで記録する。`.markharness/releases/<release_id>.yml`としてGit管理下に置くため、`--at <ref>`で過去時点の選定も再現できる。`release_id`はこのパスの**単一の構成要素**になるため、現行`generate.rs`の`require_valid_slug`が`id:`に課しているのと同じ理由で文字集合を制限する：ASCII小文字英数字・ハイフン・ドットのみを許し、空文字、`.`と`..`そのもの、先頭がドットの値、パス区切り(`/`・`\`)やドライブ指定を含む値は書き込み前に拒否する(`v1.2.0`のようなtag名は通り、`../../etc/passwd`は通らない)。書き込み自体も`fs_safety`の原子的置換経路を通す。選定リストが無いリリースについては、Release Coverageは従来どおり登録状態の一覧だけを返す(§6.2)。

`ExecutionBinding`は§1.1・[0020](../decisions/0020-execution-status-lightweight-model.md)・[0025](../decisions/0025-v2-forward-compatible-evolution.md)の通りTestCase単位の最小限の記録である。これは「実行された事実」ではなく**検証手段(自動/手動)とその参照先の対応宣言**を表す。合否・日時・実行回数を持たないため、値の存在を「最新版で実行済み」と読んではならない。将来のExecution Factへ変換・読み替えず、別の型として追加する。

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

Requirementの意味変更、またはTestCaseの実効内容変更を検知した際、関連する他方(TestCaseまたはRequirement)の状態を次の**三値**で出力する([0019](../decisions/0019-alignment-check-commit-trailer.md))。独立した承認ワークフローは作らない。

| 状態 | 条件 |
|---|---|
| 追随変更あり | 同じbase/head区間内で関連する他方の実効内容も変更されている。意味の整合を人が確認した証拠ではない |
| 確認済み | 有効な`Spec-Reviewed`トレーラーが対象を特定して存在する |
| 未確認 | 上記いずれでもない |

「追随変更あり」と「確認済み」を同じ状態にまとめない。両方のファイルがたまたま同じPRで変わっただけでは意味の整合を確認したことにならない、という[0019](../decisions/0019-alignment-check-commit-trailer.md)の前提をそのまま出力に反映する。

トレーラーの扱いは次の規則による。

1. **対象を特定できないトレーラーは採用しない。** `Spec-Reviewed: no-change-required (req-login-01)`のように対象を書く。対象のないトレーラーで複数のRequirement/TestCaseをまとめて確認済みにはしない(未確認のまま残す)。片側指定から相手を一意に解決できない場合は、`Spec-Reviewed: no-change-required (req-login-01, <case-uid>)`のように両側を明示する(規則2)。
2. **有効範囲はコミット時点の要件・ケースの組に限る。** 確認は、トレーラーを含むコミット時点のRequirement UIDとCase UIDの組に結び付ける。対象IDは当該コミットで解決し、片側だけの指定から変更内容・関連を用いて相手を一意に特定できない場合は採用しない。その場合は両側を明示する。版はGitから解決し、手入力の版文字列や独立した保存型は要求しない。同一区間内の後続コミットで、組のどちらかの実効内容(TestCaseはCase revision、Requirementは`requirement.yml`または`.sdoc` blob)が変更された場合、その組の確認を無効化する。別の組への確認の流用や、後から追加されたケースへの拡張はしない。無効な記録は「確認済み」の根拠にせず、有効な別記録がなければ§5.3の規則に従い「追随変更あり」または「未確認」を出力する。
3. **記法を限定する。** コミット本文中の、行頭から始まる`Spec-Reviewed: <value>`形式の行のみをトレーラーとして解釈する。引用行・インデントされた行・コードブロック内の同名文字列は対象外とし、本文中の言及を宣言と誤認しない。
4. **squash mergeに対応する。** 判定は`git log base..head`の各コミット本文を走査する実装とし、末尾行だけを見る実装にしない。有効範囲(規則2)を解決できない記録は採用しない。
5. **履歴を入力として明示する。** Change Impactの入力にはKnowledge/`.sdoc`のtreeに加えて`base..head`のコミット履歴が含まれる(P3の再現性契約に含める)。shallow cloneやfilterで履歴が取得できない場合は診断付きで失敗させ、履歴不足を「確認済み」として扱わない。

## 6. Change ImpactとRelease Coverage

### 6.1 Change Impact(PR単位)

base/head間のFeature版比較(現行`changes.rs`の`ChangeEvent`計算を流用)に加え、次を行う。

1. **双方向に変更集合を求める。** Featureの変更起点(変更されたFeature→`contributes_to`するRequirement)と、Requirementの変更起点(変更されたRequirement→関連するFeature・TestCase)の両方を辿る。Featureが変更されていないPRでもRequirementの変更を見落とさないため、探索をFeature変更の有無に依存させない。
2. **仕様側の変更は base/head 間の差分で判定する。** モードごとの判定対象は次の通りで、いずれも「base時点の内容」と「head時点の内容」を比較する。
   - `source: native`：`requirement.yml`自体のbase/head差分。粒度はRequirement単位で、外部ツールを必要としない。
   - `source: external`：`source_locator`が指す`.sdoc` blobのbase/head差分。`.sdoc`が**markharnessと同一のGitリポジトリで管理されている**ことを前提とし、`.sdoc`の構文解析を必要としない。粒度はファイル単位であり、同一ファイル内の別Requirementの変更でも「変更あり」と判定される(偽陽性を許容する。Requirement単位の粒度が必要になった時点でM3の`.sdoc`解析へ引き上げる)。
3. **固定参照の古さ(stale pin)は別項目として算出する。** externalモードで`source_revision`がhead時点のblob OIDと一致しない場合、「固定参照が古い」として出力する。これは2の変更検知とは独立した項目であり、`requirement repin`による参照更新が仕様変更の検知を打ち消してはならない(同一PR内で`.sdoc`を変更しrepinしても、2の差分は成立する)。
4. 変更されたTestCase・Requirementそれぞれについて、Alignment checkの状態(§5.3の三値)を算出する。
5. 影響を受けるTestCase一覧、関連Requirement一覧、Alignment checkの状態別一覧、stale pin一覧を出力する。

この方式により、Change Impact(M1)は`.sdoc`パーサ(M3)にも、StrictDocの導入有無にも依存しない。`repin`は固定参照を現在値へ進める操作にすぎず、対応確認の代替ではない(確認の記録は§5.3のトレーラーだけが担う)。repin後の無変更PRでは、base/head間に差分がないため新たな仕様変更としては報告されない。

### 6.2 Release Coverage(リリース単位)

指定したRequirement/Feature集合全体について、次を一覧化する。

- 各TestCaseに`ExecutionBinding`が存在するか、`mode`は何か。
- 各Requirementに`contributes_to`するFeatureが存在するか(coverage gap)。
- 対象Featureに具体的なScenario/TestCaseが一つも存在しないか(coverage gap)。Featureが関連付けられていても検証例がゼロなら、不足を示すTestCase行自体が出力されないため、Feature単位で明示する。

Change Impactが「今回の差分で何が変わったか」を示すのに対し、Release Coverageは「リリース対象全体を取りこぼしなく見渡せるか」を示す補助情報であり、リリース判断時にChange Impactと併用する。

Release Coverageは指定したGit ref(既定はHEAD)の内容で評価する。`--release`を指定しない場合、出力の意味は**「その時点でKnowledgeに登録されていたTestCaseと検証手段の一覧」**であり、実行された事実ではない。`mode`の存在を「実行済み」と表示しない(§5.2)。

`--release <release-id>`を指定した場合は、当該`ReleaseScope`(§5.2、[0024](../decisions/0024-release-scope-selection-list.md))を読み、次を追加で示す。

- 選定された各TestCaseに`ExecutionBinding`があるか、`mode`は何か。
- 対象Requirement/Feature配下にありながら選定リストに入っていないTestCase(選定漏れ候補)。
- 選定リストにあるが、その時点のKnowledgeに存在しないCase UID(削除・未生成)。

§1の問い3(前回リリースでどのテストが検証スコープに入っていたか)は、リリースtagを`--at`に、そのリリースの`release_id`を`--release`に渡して答える。`ReleaseScope`が記録されていないリリースについては、答えられるのは登録状態の再現までである。`ExecutionBinding`も`ReleaseScope`も日時を持たないため、時点の指定はGit refに委ねる(P3)。選定リストは人が記録した「選んだ」という宣言であり、実行された証跡ではない。

## 7. CLI案

```text
markharness requirement link --feature <feature-id> --requirement <requirement-id>
markharness requirement unlink --feature <feature-id> --requirement <requirement-id>
markharness requirement repin --requirement <requirement-id>   # externalのみ。source_revisionをhead時点のblob OIDへ更新
markharness binding set --case-uid <case-uid> --mode automated --reference src/tests/login.spec.ts
markharness binding set --case-uid <case-uid> --mode manual
markharness release scope set --release <release-id> --case-uid <case-uid> [--case-uid ...]   # 選定リストを置換
markharness release scope show --release <release-id> [--at <ref>] --format json
markharness impact --base <ref> --head <ref> --format json
markharness coverage --requirements <requirement-ids-or-all> [--release <release-id>] --at <ref> --format json
```

`requirement link`/`unlink`は`feature.yml`の`requirement_uids`を編集するコマンドであり、新しい格納先は作らない(§5.2)。出力はCLI/JSONのみとし、ローカルサーバーやダッシュボードはMVPに含めない(§8)。終了コード・JSON schemaのversioning方針は実装時に確定する。廃止するCLIは§9.1で扱う。

## 8. 非目標

- StrictDoc要件編集UI、独自要件承認workflow。
- テストケースCRUDの新規UI(現行`knowledge/`編集フローを維持するのみ)。
- 実行結果の詳細管理(pass/fail・証跡本体・実行環境matrix)。別ツールの責務とする。`ReleaseScope`は「選んだ」という宣言のみを記録し、実行結果は扱わない([0024](../decisions/0024-release-scope-selection-list.md))。
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
| `execution.rs`(target_revision/environment等) | 縮小 | [0020](../decisions/0020-execution-status-lightweight-model.md)・[0025](../decisions/0025-v2-forward-compatible-evolution.md)の`ExecutionBinding`へ置き換え |
| `plan.rs`(Evidence適用可能性判定)・`markharness plan` | 廃止 | 厳密な突合ロジックは[0020](../decisions/0020-execution-status-lightweight-model.md)で不要になり、残る「`ExecutionBinding`の有無を返す」機能は`coverage`と重複する([0026](../decisions/0026-module-inventory-and-plan-removal.md))。`application.rs`・`presentation.rs`のplan経路も同時に削除する |
| `src/identity/feature_ops.rs`の`retire_entity`/`restore_entity`/`release_id`/`reissue_entity`とそのエラー型(約524行)、`event.rs`の`IdentityMutation::{Retired, Restored, Released, Reissued}`、`engine.rs`の該当status遷移と`Status::Retired`、`migration_manifest.rs`のreissue依存部 | 縮小 | [0021](../decisions/0021-identity-retire-simplification.md)。2026-09-11の実測により対象ファイルを特定した(`recovery.rs`・`lock.rs`に現れる`release`は`IdentityLock`のファイルロック解放であり本件と無関係、`audit.rs`には該当語の出現が無い)。[0026](../decisions/0026-module-inventory-and-plan-removal.md) |
| `src/identity/`(UID発行/rename部分) | 維持 | [0021](../decisions/0021-identity-retire-simplification.md)の対象外 |
| `src/git.rs`・`fs_safety.rs` | 維持 | 不変ref読出し・原子的操作は今回の変更と独立 |
| `src/canonical.rs`(ImportSourceArg等) | 縮小 | `markharness import`(native/junit)専用に縮小する。`plan`廃止に伴い`CanonicalEvidence`/`EvidenceResult`等のplan専用型を削除。StrictDoc取込は将来別Adapterとして再設計([0026](../decisions/0026-module-inventory-and-plan-removal.md)) |
| `knowledge/requirements/`(native Requirement実体) | 維持・拡張 | nativeは`label`/`description`を含めそのまま維持する。`source: external`を選んだRequirementでのみ本文相当を持たず固定参照になる([0023](../decisions/0023-requirement-native-and-external-source.md)、§5.2.1) |
| `src/traceability.rs`(Requirement索引) | 維持 | Requirement⇄TestCaseの逆引きは既存実装をそのまま使う |
| `src/server.rs`・`ui/`・`markharness serve`(ADR 0008 Stage 3のdashboard) | 廃止 | 現行UIは`plan`/evidence出力に依存し、plan縮小と同時に壊れる([0022](../decisions/0022-remove-stage3-dashboard.md))。§9.1 |
| `src/milestone.rs`・`src/lineage.rs` | 維持 | `src/changes.rs`が`milestone::verify_audit_matches_tag`(fail-closedゲート)と`lineage::classify`(merge分類)を内部利用しており、削除すると`changes compute`がコンパイル不能になる([0026](../decisions/0026-module-inventory-and-plan-removal.md)) |
| `src/backfill.rs`・`src/verify.rs` | 維持 | 逆依存ゼロだが[0020](../decisions/0020-execution-status-lightweight-model.md)〜[0025](../decisions/0025-v2-forward-compatible-evolution.md)のいずれとも衝突せず、廃止する積極的な理由が無い([0026](../decisions/0026-module-inventory-and-plan-removal.md)) |
| `src/derived_index.rs`・`markharness cache index` | 廃止 | 入力である`plan::BoundVersions`と`execution::read_all_results`が廃止・置換される。派生キャッシュはChange Impact/Release Coverageの設計に登場せず、CLI統合テストも無い([0026](../decisions/0026-module-inventory-and-plan-removal.md)) |
| `src/audit_scope.rs` | 維持 | `identity migrate --json`・`identity audit --json`・`changes compute --json`の出力契約([0013](../decisions/0013-immutable-identity-model.md)検証規則)に含まれる |
| `identity` CLIの`retire`/`restore`/`release`/`reissue` | 廃止 | [0021](../decisions/0021-identity-retire-simplification.md)。既存イベントログの扱いは§9.1 |

### 9.1 既存CLI・既存データ・既存UIの扱い

- **廃止するCLI**：`identity retire`/`restore`/`release`/`reissue`([0021](../decisions/0021-identity-retire-simplification.md))、`plan`([0026](../decisions/0026-module-inventory-and-plan-removal.md))、`execution record`(`binding set`へ置換、[0020](../decisions/0020-execution-status-lightweight-model.md)・[0025](../decisions/0025-v2-forward-compatible-evolution.md))、`serve`([0022](../decisions/0022-remove-stage3-dashboard.md))、`cache index`([0026](../decisions/0026-module-inventory-and-plan-removal.md))。削除範囲は実装時のチェックリストで確定する。
- **既存データ**：**過去のスキーマ・データは最初から存在しなかったものとして扱う**([CLAUDE.md](../../../CLAUDE.md)の後方互換を想定しない設計ルール)。`ExecutionBinding`は新しい保存先`.markharness/bindings/`のみを読み、旧`.markharness/executions/`配下の実行記録は参照しない。廃止したevent種別は`IdentityMutation`から削除されるため、それを含むログは読み取り経路に存在しない。自動変換も、互換replayも、旧データを名指しする診断も実装しない——いずれも互換コードであり、本方針の排除対象である。旧ディレクトリがworktreeに残っていても新コードのどの経路も読まないため、動作には影響しない。
- **既存dashboard**：`src/server.rs`・`ui/`・`markharness serve`・frontendのbinary同梱を削除する([0022](../decisions/0022-remove-stage3-dashboard.md))。削除は`plan`縮小と同じタイミングで行い、`tests/server.rs`等の関連テストも同時に削除する。リポジトリ外のviewerが`plan`出力を参照している場合は、Change Impact/Release Coverage出力への切替が必要になる。

### 9.2 将来拡張性としてV2に残す契約

V2は将来の完全モデルを部分実装するものではない。V2単体でNorth Starの4問へ答えられる小さな製品として完成させ、その後にStrictDoc→markharness→Playwrightの流れを実運用して、必要性が確認された概念だけを追加する。詳細な決定理由は[0025](../decisions/0025-v2-forward-compatible-evolution.md)を正とする。

#### 9.2.1 後から変えると高価な共通基盤

V2の時点で、次の契約を安定させる。

- Requirement、Feature、Behavior、Scenarioにはkindを区別したUIDを持たせ、表示ID・label・pathと同一性を分離する。
- 1 Scenario = 1 TestCaseとし、Case UIDはScenario UIDから決定的に導出する。
- Case revisionは実効的な検証内容から計算し、Requirement関連、実行結果、Release Scope、表示情報を混ぜない。
- external Requirementは、外部key、同一Git内のlocator、固定revisionを区別する。V2の変更検知がファイル単位でも、将来のStrictDoc AdapterがRequirement単位の内容を解決できる識別情報を失わない。
- Playwrightとの対応にはtest titleやファイル名ではなくCase UIDを使う。`reference`は移動可能な案内であり、照合のIdentityにはしない。
- Change ImpactとRelease Coverageの公開JSONはtop-levelに`schema_version`を持ち、解決済みの完全なGit commit ID、入力schema version、判定に影響する規則versionを含める。同じ入力を別のAI・CLI・CIが読んでも判定根拠を再現できる形にする。

これらは将来機能の先行実装ではなく、後から変更すると既存Knowledge・関連・履歴の移行が必要になる最小の永続契約である。

#### 9.2.2 簡略モデルを強い事実へ読み替えない

V2と将来モデルの関係を次のように固定する。

| V2の記録 | V2が保証する事実 | 将来追加し得る別の記録 | 禁止する読み替え |
|---|---|---|---|
| `ExecutionBinding` | Case UIDに自動または手動の検証手段と参照先がある | `ExecutionFact` | bindingがあるため実行済み・合格とみなす |
| `ReleaseScope` | そのreleaseでCase UIDを選定した | `ReleasePlan` | 選定一覧を版・理由・build・環境まで確定した計画とみなす |
| `Spec-Reviewed` trailer | commit時点で対象の対応確認を記録した | `ImpactDecision`、`HumanAttestation` | 欠落しているLink・policy digestや承認を補完する |
| Git上の削除・再登場 | その時点でファイルが無い、または再び存在する | `retire`、`restore` event | 削除意図や同一Identityとしての復元を推測する |

永続レコードには`schema_version`を持たせる。複数種類のレコードを同じ保存領域または出力に載せる場合は`record_kind`等で種類を明示する。ただし`schema_version`は**全種別で`1`に固定し、今後も上げない**。これは過去のレコードを読むための互換機構ではなく、将来別種のレコードを追加したときに種類を取り違えないための前方向の契約だからである。旧版を読む必要が生じた場合も版で分岐せず、新しい型として追加する(§9.1の既存データ方針)。`record_kind`の値は`execution_binding`・`release_scope`・`requirement`・`change_impact`・`release_coverage`とする。将来の型に必要そうなフィールドをすべてoptionalとしてV2へ足さない。完全モデルは別の型・保存契約として追加し、V2に存在しない情報は`unknown`または`legacy`とする。

#### 9.2.3 Adapterと読み取りの発展方法

StrictDoc固有の構文解析とPlaywright固有のreporter形式をDomainへ入れない。ただし、実在する形式が一つしかない段階で汎用plugin interfaceを作らない。最初の実装はApplication境界で正規化し、二つ目の実在Adapterまたは交換要求が現れた時点で共通seamを抽出する。

将来形式を追加するときは、旧レコードを破壊的に変換するのではなく、必要に応じて複数readerから同じ読み取りモデルへ正規化する。

```text
ExecutionBinding reader ─┐
ExecutionFact reader ────┴→ release verification read model

Trailer decision reader ─┐
Structured decision reader┴→ alignment resolution read model
```

上図は将来の発展方向であり、V2で空のreaderやseamを実装する要求ではない。二つ目の入力が存在するまで、型の区別と保存場所の衝突回避だけを維持する。

#### 9.2.4 StrictDoc・Playwright実運用で観測する事項

M3・M4では機能実装だけでなく、次の事実を記録して次段階の設計入力にする。

- StrictDocのファイル単位変更検知で生じた偽陽性の割合と、Requirement単位解析が必要だった事例。
- Requirement変更から対象TestCaseを確定するまでの時間、AI・規則が提示した候補の採用・追加・除外と理由。
- Case UIDに対応するPlaywright testが0件または複数件になる頻度と、その正当な例外。
- parameterized test、Playwright project、retryをLogical TestCaseと別の実行単位として扱う必要性。
- ReleaseScopeと実際に実行された集合との差、およびCase revision・対象commit・build・環境の違いで結果を採用できなかった実例。
- commit trailerを後から追加・訂正したくなった事例と、CLI/JSONだけでは判断を説明しにくかった事例。

これらの観測で具体的な不足が確認されたときだけ、Release Plan、Execution Fact、構造化Decision、dashboard等を別ADRで昇格させる。

#### 9.2.5 将来保証のcutover

V2期間中に保存しなかった情報を、Git履歴や自然言語から完全な事実として推測しない。将来、Case revision・build・環境まで照合するExecution Fact、または`retire`・`restore`・ID予約を含む完全なIdentity lifecycleを導入する場合は、保証開始commitを明示する。

cutover前の記録は次のように扱う。

- `ExecutionBinding`はそのままbindingとして有効だが、過去に実行されたFactへ変換しない。
- `ReleaseScope`は選定事実として有効だが、理由・対象版・実行条件は`unknown`とする。
- V2期間の削除・再登場は、明示的なmigration manifestで採用したものを除き、`retire`・`restore`へ変換しない。
- 完全なIdentity lifecycleを始める場合、cutover時点のactive identityと、継続追跡が必要なretired identityだけをmigration manifestで確定する。それ以降のeventにのみ完全保証を与える。

このcutoverは、過去データを捨てるためではなく、V2が実際に記録した弱い事実と、将来記録する強い事実を混同しないための境界である。

#### 9.2.6 先行実装しないもの

将来性を残す目的で、次をV2へ追加しない。

- 汎用runner plugin基盤と、未使用のAdapter interface。
- build・environment・attempt・evidenceを空値で持つ`ExecutionBinding`。
- 承認状態を持たない空の`ImpactDecision`または`HumanAttestation`。
- 使用実績のない`retire`・`restore`・ID予約状態遷移。
- 将来のRelease Planを想定した大量のoptionalフィールドを持つ`ReleaseScope`。

V2の拡張容易性は、未来のフィールドを予約することではなく、現在の型の意味を狭く保ち、別の事実を別の型として後から追加できることによって確保する。

## 10. ロードマップ

| 段階 | 作るもの | 完了条件 |
|---|---|---|
| M0 | `Requirement`の新schema(native/externalの二モード)・`ExecutionBinding`のschema、`feature.requirement_uids`による関連付け、CLI(§7)、Alignment check(§5.3)の自動判定、対話作成フローの更新(§5.2.1) | native運用(StrictDocなし)とexternal運用の双方でFeature⇄Requirementの対応とTestCaseの`ExecutionBinding`記録がGit/CLI経路で完結し、モードの混在した`requirement.yml`が拒否される |
| M1 | Change Impact(§6.1) | PR base/head間で影響Feature・Requirement・未確認Alignment checkを一覧できる(`.sdoc`解析=M3に依存しない) |
| M2 | Release Coverage(§6.2)と`ReleaseScope`(§5.2) | 指定Requirement集合全体のcoverage gapを一覧でき、選定リストを記録したリリースでは選定・選定漏れ・不在Case UIDを併せて一覧できる |
| M3(将来) | StrictDoc `.sdoc`取込(Git管理された要件の実体反映) | 需要確認後に着手。自前パーサの要否を含め別途設計する |
| M4(将来) | Playwright連携の実運用検証 | 要望が出た時点で着手。まずCase UIDと`ExecutionBinding`による接続・外部reportの観測を行い、結果を永続的なExecution Factとして取り込むかは§9.2の観測後に別ADRで決める |

MVPはM0〜M2とする。M3・M4は本書の時点では着手を約束しない。

## 11. 受け入れ条件

| ID | シナリオ | 期待結果 |
|---|---|---|
| AC01 | FeatureをRequirementへ`contributes_to`で関連付ける | 関連はFeature側が正本を持ち、逆引き一覧は派生する |
| AC02 | `source: external`のRequirementの本文(`label`/`description`)をmarkharnessから編集しようとする | 拒否する。externalではmarkharnessは固定参照のみ保持する([0023](../decisions/0023-requirement-native-and-external-source.md)) |
| AC02b | `source: native`のRequirementの`label`/`description`を編集する | 成功する。nativeではmarkharnessが本文の正本を持つ |
| AC03 | Requirementが変更されたのに関連TestCaseが更新されていない | Change Impactの出力で「未確認」として明示する |
| AC04 | TestCase変更コミットに、対象を特定した`Spec-Reviewed: no-change-required (req-xxx)`が付与され、相手側のCase UIDが一意に解決できる | Alignment checkは当該の組について「確認済み」と判定する(§5.3) |
| AC05 | TestCaseに`ExecutionBinding(mode=manual)`を記録し、日時や実行者は渡さない | 記録が成立する。日時・実行者フィールドは存在しない |
| AC06 | 同一入力から複数回Change Impact/Release Coverageを計算する | 同じ出力を再現する(P3) |
| AC07 | 削除したTestCaseと同じ内容のScenarioをCLIで新規作成する | 新しいScenario UIDが発行され、そこから導出されるCase UIDも別値になる。内容の一致を理由に旧UIDを推定しない([0021](../decisions/0021-identity-retire-simplification.md)) |
| AC07b | 削除したScenarioのファイルをGit履歴から復元する(`git checkout <ref> -- <path>`等) | ファイル内の`uid:`が戻るため、当時のScenario UID・Case UIDが復活する。これはmarkharnessの`restore`機能ではなくGit履歴操作であり、markharnessはこれを禁止も検出もしない([0021](../decisions/0021-identity-retire-simplification.md)§2) |
| AC08 | Requirementに`contributes_to`するFeatureが一つもない | Release Coverageでcoverage gapとして一覧される |
| AC09 | `source: external`なのに`source_locator`/`source_revision`を持たない`requirement.yml`を置く | `validate`が拒否する(§5.2.1) |
| AC09b | `source`を省略した`requirement.yml`(`label`あり)を置く | `validate`が拒否する。モード判定を暗黙のdefaultに委ねない(§9.1) |
| AC09c | `label`と`source_locator`を両方持つ`requirement.yml`を置く | `validate`が拒否する(モード混在) |
| AC10 | `source: external`のRequirementで、`source_locator`が指す`.sdoc` blobがbaseとheadで異なる | Change Impactが仕様側変更として検出する。`.sdoc`の構文解析は行わない(§6.1手順2) |
| AC10b | `source: native`のRequirementの`label`/`description`をbase/head間で変更する | Change Impactが仕様側変更として検出する(§6.1) |
| AC10c | `.sdoc`はbase/head間で変更されていないが、`source_revision`がhead時点のblob OIDと一致しない | stale pinとしてのみ出力する。仕様側変更としては報告しない(§6.1手順3) |
| AC11 | 過去のリリースtagを`--at`に指定してRelease Coverageを算出する | 当時のKnowledge・`ExecutionBinding`に基づく一覧を再現する(§6.2) |
| AC12 | 1コミットで複数のRequirementに触れ、対象を書かないtrailerを付与する | どの対応確認が済んだか判定できないため「未確認」のまま残る(§5.3) |
| AC13 | Scenarioの表示idをrenameする | `ExecutionBinding`はCase UID参照のため維持される(§5.2) |
| AC14 | C1でRequirement Rを変更し`Spec-Reviewed`を付与、同一PRのC2でRをさらに変更する | C1の確認は無効になり、Rは「未確認」として出力される(§5.3規則2) |
| AC15 | RequirementとTestCaseが同一PRで変更されているが、`Spec-Reviewed`が無い | 「追随変更あり」として出力し、「確認済み」とはしない(§5.3) |
| AC16 | 対象を書かない`Spec-Reviewed`トレーラーを、複数Requirementに触れるコミットに付与する | どのRequirementも確認済みにならない(§5.3規則1) |
| AC17 | shallow cloneなどで`base..head`のコミット履歴を取得できない | 診断付きで失敗する。履歴不足を「確認済み」として出力しない(§5.3規則5) |
| AC18 | 同一PRで`.sdoc`を変更し、同じPR内で`requirement repin`も実行する | 仕様変更として検出される。repinは検知を打ち消さない(§6.1手順3) |
| AC19 | repin後、内容を変更しない次のPRを評価する | 新たな仕様変更としては報告されない。固定参照が古い場合のみstale pinとして出力する(§6.1手順3) |
| AC20 | Requirementのみが変更され、関連Featureは変更されていない | 関連Feature・TestCaseを逆引きし、影響とAlignment checkを出力する(§6.1手順1) |
| AC21 | RequirementにFeatureは関連付いているが、そのFeature配下にScenarioが一つもない | Release Coverageが当該Featureをcoverage gapとして明示する(§6.2) |
| AC23 | 発行(`issued`)とrenameのみで構成された`identity-events`を読み込む | 決定的にreplayでき、UIDとidの対応を再現する(§9.1) |
| AC24 | `ReleaseScope`を記録し、過去のリリースtagを`--at`、`release_id`を`--release`に渡してRelease Coverageを算出する | 当時選定されたTestCaseと、その検証手段の有無を再現する(§6.2) |
| AC25 | 対象Requirement配下にあるが選定リストに入っていないTestCaseがある | 選定漏れ候補として一覧される(§6.2) |
| AC26 | 選定リストに、その時点のKnowledgeに存在しないCase UIDが含まれる | 不在のCase UIDとして明示する。選定リストを自動的に書き換えない(§6.2) |
| AC27 | `ReleaseScope`に選定日時・担当者・合否を渡そうとする | フィールドが存在せず記録できない([0024](../decisions/0024-release-scope-selection-list.md)) |
| AC28 | `release_id`に`../../etc/passwd`、`..`、`/abs/path`、先頭ドットなどを渡す | 書き込み前に拒否し、`.markharness/releases/`の外にも中にもファイルを作らない(§5.2) |
| AC29 | C1でケースAを変更し要件Rの変更不要を確認、C2でAだけを再変更する | Rが不変でも組(R,A)の確認は無効。確認済みとはせず、この例では未確認を出力する |
| AC30 | 片側指定のトレーラーから複数の相手ケースが候補になる | 一括確認しない。両側を明示して組を一意に解決できる記録だけ採用する |
| AC31 | 組(R,A)の確認後、別ケースBが追加される | Bへ確認を流用しない。元の組の内容が不変ならその確認は維持する |
| AC32 | `ExecutionBinding`へresult、executed_at、build、environmentを渡す | V2のbinding schemaに存在しないフィールドとして拒否し、実行事実として保存しない(§9.2.2) |
| AC33 | 将来のreaderがV2の`ReleaseScope`を読む | 選定されたCase UIDだけを既知とし、理由・Case revision・build・環境は`unknown`として扱う(§9.2.2) |
| AC34 | Playwrightのtest titleまたはファイルパスを変更し、Case UIDのannotationは維持する | 同じTestCaseへのbindingとして解決し、titleやpathをIdentityとして扱わない(§9.2.1) |
| AC35 | Playwright report内で一つのCase UIDが0件または複数件へ解決される | 実運用観測へ明示的に記録し、自動的に任意の1件を選ばない(§9.2.4) |
| AC36 | 将来の完全なIdentity lifecycle導入前に、V2で削除・再登場した要素がある | migration manifestで明示されない限りretire/restoreを推定せず、cutover前のlifecycleを`legacy`または`unknown`として扱う(§9.2.5) |
| AC37 | 同じbase/headと規則versionでChange Impactを再計算する | JSONの`schema_version`、解決済みcommit ID、規則versionを含めて同じ判定を再現できる(§9.2.1) |
