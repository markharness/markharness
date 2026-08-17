# 0008: 製品ロードマップをPR Verification Plan中心に再定義する

## ステータス

Accepted(Stage 0は一部実行済み。ChangeEventのgolden contractとCLI JSON契約のversioning方針は確定済み。canonical snapshot・plan statusのgolden datasetは各機能の実装時に追加する)。

## コンテキスト

`docs/Markharness_改善・実装検討_統合設計文書.md`(2026-08-17付、以下「統合設計文書」。採否判断が本ADRおよび[verification-plan-canonical-model-design.md](../design/verification-plan-canonical-model-design.md)への転記により完了したため、ファイル自体は削除済み。`git log -- docs/`で復元可能)は、現行のmarkharness設計(テスト知識をprimary artifact、TestCaseをderived artifactとするGit-nativeモデル、第2.4節・第3章の通り実装済み)を土台に、次の3方向の拡張を検討した。

1. Doorstop・StrictDoc・TestRail・Gherkin/Cucumber・Playwright・JUnit等を入力元とする正規化(canonical import)基盤。
2. PRのcode/spec/knowledge diffから、影響済みテストと新規に必要なテストを含むVerification Planを生成する機能。
3. Release Verification Dashboard・Feature History等、Markharness固有モデルを可視化するGUI。

現状のmarkharness CLIは、単一Gitリポジトリ内の`knowledge/`を対象に、マイルストーン境界の`ChangeEvent`自動計算(`changes compute`)・実行証拠のversion bind(`verify trace`/`verify pending`)までを実装済みである(論文`git-native-model-for-test-knowledge-management.md`第3章)。一方、PR単位(base/head任意の2点)でのVerification Plan生成、外部ツールからのインポート、GUIはいずれも未着手である。

統合設計文書が指摘する主要なリスクは、この3方向を無秩序に並行着手すると、(a)import/normalize機能が前面に出すぎて「version-aware middleware」に縮小して見える、(b)TestCase CRUDの再実装に陥る、(c)GUIが独自のstatus modelを持ちDomain Engineと乖離する、の3点である。本ADRは、この3方向をどの順序・どの境界で着手するかを決定する。

## 決定

### 1. 製品ビジョンを次の1文に固定する

> Markharness turns a change into a reviewable verification plan. Git remains the source of truth.

競争軸をdashboard・RBAC・SSO等の機能数に置かず、「この変更で、何をテストすれば十分か」への回答速度と品質に置く。この文言はREADME等の対外説明文の更新時に参照する一次情報とする。

### 2. 着手順序をStage 0〜3の順に固定する

既存の`changes compute`(マイルストーン境界)を拡張し、PRのbase/head任意の2点を第一級のversion rangeとして扱えるようにする方向で、以下の順に進める。優先順位の理由は、canonical modelがPlanの安定した入力になり、PlanがGUIの意味のあるread modelになるという依存関係に基づく(統合設計文書第8章)。

| Stage | スコープ | Exit Criteria |
|---|---|---|
| Stage 0 | 現行domain model・用語をADR/schema文書に固定。fixture repositoryとgolden dataset化。CLI JSON contractのversioning方針を定義。 | 同一fixtureから同一canonical snapshot・change・plan statusを再生成できる。 |
| Stage 1 | canonical artifact/version/relation/evidence schema。Markharness native importerとJUnit evidence importer。stored/derived traceのorigin区別。`import --format json`。 | native knowledgeとJUnit resultから、version-awareなplan statusをCIで再現できる。 |
| Stage 2 | base/head diff収集。stored/derived traceによるaffected existing tests。rule-basedのmissing test検査。optionalなAI proposal adapter。`markharness plan --base --head --format json`。 | 履歴PRデータセットで、人間のplanと比較した評価結果(precision/recall等)が得られる。 |
| Stage 3 | `markharness serve`によるread-only Release Verification Dashboard・Feature History。 | 対象ユーザーがCLI/ファイルのみより短時間でreleaseの残検証を説明できる。 |

Stage 4(外部import拡張・PR check/comment統合)・Stage 5(collaborative SaaS)は、Stage 0〜3の完了と利用実績を前提とする条件付き着手とし、本ADRでは着手順序を確定しない(第4節)。

### 3. 境界(やらないことの明文化)

以下を製品の前面に出さない境界として明記する。実装時にこの境界を越える提案が出た場合は、新たなADRとして是非を判断する。

- **TestCase個別CRUDをprimary UIにしない**：TestCaseはderived artifactの例外編集としてのみ扱う(現行の論文モデルと整合、変更なし)。
- **canonical importを製品の主役にしない**：import/normalizeは内部基盤とplugin boundaryに留め、ユーザー向けの説明は常に「既存のテスト資産を捨てずに、PRごとのVerification Planを作れる」に置く。
- **AI proposalを自動commitしない**：AIはbehavior change・missing test・obsolete testの候補生成にのみ利用し、canonical YAML/filesへの反映は人間のreviewとGit diffを必須とする。
- **GUI専用DBをsource of truthにしない**：GUI(Stage 3)はGit repositoryから導出したVerification Planのread-only viewerとし、検索indexやcacheを持つ場合もGitから再構築可能にする。Git管理ファイルを変更するeditor機能はStage 3へ含めず、必要性が確認された場合に別ADRで判断する。

### 4. Stage 4・5は条件付き着手とする

Stage 5(collaborative SaaS: RBAC・SSO・共有DB等)は、次の条件が確認された場合のみ着手を検討する。本ADRの時点では着手しない。

- 複数チームからshared dashboard/assignment需要が継続的にある。
- Git/CI integrationだけでは解けないcollaboration課題が明確である。
- hosted metadataがなくてもcanonical stateをGitへexportできる設計を維持できる。

Stage 4(TestRail等の既存TMSからのインポータ、GitHub/GitLab PR check連携)は、Stage 2のVerification Plan PoCが実用的な精度(precision/recall)を示した後に着手する。TestRail importerは需要確認後とし、Stage 1のimporter導入順序は「Markharness native → JUnit XML → Gherkin → Playwright → Doorstop/StrictDoc → TestRail」の順とする(ファイルベースを先行させ、SaaS API固有の認証・pagination・rate limit対応を後段に送る、統合設計文書第3.8節)。

### 5. 既存機能の扱い

統合設計文書第9節の提案を踏まえ、以下を維持・縮小の方針として採用する。

**維持・強化**：structured test knowledge、deterministic TestCase generation、Git tree SHAによるFeature version、milestone/snapshot diff、change→affected TestCase導出、execution evidenceのversion bind、derived pending/re-verification、file/CLI workflow。いずれも現行実装(論文第3章)の中核であり変更しない。

**縮小・再設計**：milestone-only UXを、PR base/headをfirst-classに追加した共通version rangeへ一般化する(Stage 2で着手)。human-oriented text出力のみに依存せず、stable JSON/schemaを同等以上に重視する(Stage 0〜1で着手)。単純なPASS/FAIL表示は、既存の`verify trace`/`verify pending`が持つvalid/stale/unknown区分(論文第3.7節)を追跡入力として活用する形に統合する。

### 6. 実装アーキテクチャをモジュラーモノリスに固定する

Stage 0〜3は、Gitをsource of truthとする単一製品・単一リポジトリのモジュラーモノリスとして実装する。Webサーバー、共有DB、常駐ワーカー、マイクロサービスをDomain Engineの前提にしない。既存のRust CLIを全面再実装せず、現在の決定的生成、Git tree SHA比較、実Git fixtureテスト、`fs_safety`、`changes compute`のbackfill再利用を維持する。

| Module | 責務 | 主なInterface |
|---|---|---|
| `KnowledgeWorkspace` | `knowledge/`・`axes/`の読込、正規化、参照・schema検証、Knowledge Snapshot構築 | `load`、`validate`、`snapshot`、`apply` |
| `TestcaseCompiler` | SnapshotからTestCaseとtraceability indexを決定的に生成 | `compile(snapshot) -> GeneratedArtifacts` |
| `ChangeAnalyzer` | `CommitRef`で表した任意のfrom/to間のFeature tree SHA比較、ChangeEvent・影響TestCase・`true_divergences`導出 | `compute(from, to, options) -> ChangeSet` |
| `VerificationEngine` | ChangeEventとversion bindされたexecution evidenceからvalid/pending/stale/unknownを導出 | `trace(input)`、`pending(input)` |
| `BackfillCoordinator` | 未処理version rangeの選択、ChangeAnalyzer呼出し、Git notesへの進捗記録 | `run_once(policy) -> BackfillSummary` |

CLI・CI summary・Stage 3 GUIはDomain Engineの計算を再実装せず、Application Use Caseが返す同一の結果型を異なるPresentationで表現する。依存方向は`Presentation -> Application -> Domain -> Infrastructure`とする。ModuleのInterface、`CommitRef`、`KnowledgeSource`、生成物更新の原子性、段階的移行は[decisions/0009](./0009-domain-application-infrastructure-layering.md)を決定記録、[domain-application-infrastructure-layering-design.md](../design/domain-application-infrastructure-layering-design.md)を詳細設計の正とする。

### 7. スケーラビリティの優先順位を固定する

Stage 0〜3で最初に改善するスケールは、サーバー台数ではなく、機能数・コード量・テスト数・開発人数である。Domain ModuleとApplication Use CaseのInterfaceを安定させ、変更の局所性とCLI/CI/GUI間の再利用性を高める。

Knowledge件数、milestone数、execution件数に対する性能改善は、正確性を保つ次の順序で行う。

1. 同一コマンド内で正規化済みKnowledge Snapshotを共有する。
2. `GitTreeKnowledgeSource`で過去commitをGit tree/blobから直接読み込む。
3. Feature・ChangeEvent・Executionの再構築可能な索引を追加する。
4. `backfill run`へ処理量制限を追加する。
5. 計測結果に基づき増分生成または限定的な並列処理を追加する。

全生成・全検証を正準動作として維持し、増分処理と索引は削除・再構築可能な最適化に限定する。Stage 5の着手条件が成立するまでは、水平スケール、共有DB、分散ジョブキューを導入しない。詳細なPhase 1〜5とテスト戦略はdecisions/0009およびその設計文書へ委ねる。

### 8. Stage 3 GUIの配置と配布を固定する

Stage 3 GUIは別製品・別リポジトリとして開始せず、markharnessと同一リポジトリ内の独立frontend packageとして実装する。プロダクトは一つに保ち、コード上はDomain Engine、Application、CLI、localhost server、frontendを別Module/packageに分ける。

```text
markharness/
  crates/
    markharness-domain/
    markharness-application/
    markharness-git/
    markharness-cli/
    markharness-server/
  ui/
  schema/
  tests/fixtures/
  tests/golden/
```

上記は目標とする責務配置を示すものであり、Stage 0で直ちにcrate分割することを要求しない。まず既存crate内でInterfaceと依存方向を確立し、独立ビルド・依存管理・リリース上の価値が生じたModuleからworkspace crateへ分離する。

`markharness serve --dir <repository>`はlocalhostに限定したread-only serverとして、Domain Engineが生成するversioned read modelと静的GUI assetsを配信する。GUIはChangeEvent、evidence freshness、Verification Plan statusを独自計算せず、Domain Engineが返した`valid`/`pending`/`stale`/`unknown`およびreason/source/confidenceを表示する。

公開契約は`markharness plan --format json`と対応するversioned JSON Schemaとする。Stage 3のlocalhost HTTP Interfaceは同一リリース内の内部Interfaceとして開始し、初期段階では外部クライアント向けの長期互換性を約束しない。リリース時はfrontendの静的成果物をRustバイナリへ同梱し、利用者にNode.js環境を要求しない。

GUIの別リポジトリ化は、次のいずれかが成立した場合に新しいADRで再検討する。

- CLI/Domain EngineとGUIのリリース周期が継続的に異なる。
- GUIを独立チームが所有する。
- 複数versionのmarkharness serverへ接続する互換性が必要になる。
- GUI単独での配布・hosting、複数repository横断表示が必要になる。
- Stage 5のRBAC・SSO・shared metadataを持つcollaborative SaaSへ着手する。

## 結果

- ロードマップの優先順位が明文化されることで、import/normalize機能への偏重(統合設計文書が指摘するリスク)を構造的に避けやすくなる。
- Stage 0で現行domain modelをADR/schema文書に固定するため、後続のStage 1〜3の実装が現行論文モデル(`ChangeEvent`・`verified_feature_tree_shas`等)と整合していることを都度検証できる。
- Stage 4・5に明示的な着手条件を設けることで、「Git-nativeであることの優位性を損なう」collaborative SaaSへの早期投資を防ぐ。
- Domain EngineをCLI・CI・GUIから共有するため、Presentationごとのstatus model分岐を防ぎ、変更と検証を一箇所へ集約できる。
- Stage 3 GUIを同一リポジトリの独立packageとして開始することで、Stage 0〜2で発展するschema・golden fixture・JSON契約を一つの変更として検証できる。一方、独立配布の条件が成立した場合の再判断も妨げない。
- 全生成を正準動作、cache/index/増分処理を再構築可能な最適化とするため、大規模化による性能改善がGit-nativeな正確性を置き換えない。
- 本ADRはStage 0〜3のスコープと順序のみを決定するものであり、各Stageの詳細な canonical schema・Verification Plan JSON契約・bounded componentsの設計は別途design文書([verification-plan-canonical-model-design.md](../design/verification-plan-canonical-model-design.md))に委ねる。

## 検討したが採用しない選択肢

- **GUI(Stage 3)を先に着手する**：Markharness固有モデルの可視化は訴求力があるが、安定したPlanのJSON契約が無い状態でGUIを作ると、GUI側が独自のstatus modelを持ちDomain Engineと乖離するリスクが高い(統合設計文書第5.7節)。canonical model→Planの順で安定させてから着手する。
- **Stage 3 GUIを最初から別リポジトリ・独立製品にする**：責務分離はできるが、Stage 0〜2で発展するDomain語彙、JSON Schema、golden fixture、リリース順序の同期コストが早期に発生する。独立frontend packageとして同一リポジトリに置き、公開JSON契約の成熟またはStage 5の要件成立後に再検討する。
- **GUIを既存CLI Moduleへ直接組み込む**：単一配布は容易だが、CLI引数・process exit・表示・server lifecycle・frontend asset管理が同じModuleへ集中する。単一製品・単一リポジトリを維持しつつ、CLI、server、frontendは別Module/packageとする。
- **正準データを共有DBへ移す**：横断検索やSaaS化には有利だが、Stage 0〜3ではGitとの二重source of truthを生む。DBまたはSQLiteはGitから再構築可能なread model/indexに限定する。
- **TestRail等の主要TMSからのインポータを最優先で実装する**：需要が明確な機能だが、SaaS APIの認証・pagination・rate limit対応はfile-basedのimporter(native/JUnit/Gherkin)より実装コストが高く、Stage 1の検証速度を落とす。ファイルベースのimporterで先にcanonical schemaを検証してから着手する。
- **AI proposalを早期にVerification Planの既定の情報源にする**：missing-test発見の精度向上に直結するが、baseline(stored trace・derived trace・rule-based gap analysis)との比較評価が無い状態でAIを既定にすると、Plan生成の再現性・説明可能性が損なわれる。Stage 2ではAIをoptionalなvariantとして追加し、baselineとの差分効果を計測する。
