# 0008: 製品ロードマップをPR Verification Plan中心に再定義する

## ステータス

Proposed(未着手。本ADRは方向性の決定であり、実装計画は本文中のロードマップの通り段階的に着手する)。

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
- **GUI専用DBをsource of truthにしない**：GUI(Stage 3)はGit repositoryのviewer/editorとし、検索indexやcacheを持つ場合もGitから再構築可能にする。

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

## 結果

- ロードマップの優先順位が明文化されることで、import/normalize機能への偏重(統合設計文書が指摘するリスク)を構造的に避けやすくなる。
- Stage 0で現行domain modelをADR/schema文書に固定するため、後続のStage 1〜3の実装が現行論文モデル(`ChangeEvent`・`verified_feature_tree_shas`等)と整合していることを都度検証できる。
- Stage 4・5に明示的な着手条件を設けることで、「Git-nativeであることの優位性を損なう」collaborative SaaSへの早期投資を防ぐ。
- 本ADRはStage 0〜3のスコープと順序のみを決定するものであり、各Stageの詳細な canonical schema・Verification Plan JSON契約・bounded componentsの設計は別途design文書([verification-plan-canonical-model-design.md](../design/verification-plan-canonical-model-design.md))に委ねる。

## 検討したが採用しない選択肢

- **GUI(Stage 3)を先に着手する**：Markharness固有モデルの可視化は訴求力があるが、安定したPlanのJSON契約が無い状態でGUIを作ると、GUI側が独自のstatus modelを持ちDomain Engineと乖離するリスクが高い(統合設計文書第5.7節)。canonical model→Planの順で安定させてから着手する。
- **TestRail等の主要TMSからのインポータを最優先で実装する**：需要が明確な機能だが、SaaS APIの認証・pagination・rate limit対応はfile-basedのimporter(native/JUnit/Gherkin)より実装コストが高く、Stage 1の検証速度を落とす。ファイルベースのimporterで先にcanonical schemaを検証してから着手する。
- **AI proposalを早期にVerification Planの既定の情報源にする**：missing-test発見の精度向上に直結するが、baseline(stored trace・derived trace・rule-based gap analysis)との比較評価が無い状態でAIを既定にすると、Plan生成の再現性・説明可能性が損なわれる。Stage 2ではAIをoptionalなvariantとして追加し、baselineとの差分効果を計測する。
