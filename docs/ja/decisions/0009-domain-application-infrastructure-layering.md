# 0009: CLIをDomain/Application/Infrastructureへレイヤー分離する

## ステータス

Accepted(Phase 1〜2は2026-08-18に実行済み。型付き変更計算・Git操作集約に加え、`generate`・`changes compute`・`verify pending`のApplication Use Case抽出、`CommandOutcome`、Human/JSON Presenterを実装。Phase 3〜5は未着手)。

## 背景

現在の実装(単一Rust crate)は機能別のフラットな`.rs`ファイル群で構成されており、コード量が増えるにつれて以下の問題が具体的に確認されている。

- `src/cli.rs`が2248行あり、引数解析・Use Case実行・人間向け/JSON出力・終了コード決定を一括で担当している(`process::exit`呼び出しが32箇所、`println!`/`eprintln!`が92箇所)。
- `src/changes.rs`の`compute_changes(root, from_milestone, to_milestone, use_cache: bool, use_current_tree: bool)`は意味の薄いbool引数を2つ取り、呼び出し側が意図を読み取りにくい。
- `changes.rs`内に`Command::new("git")`の直接呼び出しが5箇所分散しており、`src/git.rs`に集約されていない。
- `src/verify.rs`の`trace`/`pending`関数は`fs::read_to_string`を直接呼んでおり、判定ロジック(Current/Pending/Stale/Unknown相当の分岐)とファイルI/Oが分離されていない。
- `src/knowledge.rs`はYAMLのparse/serializeのみを提供し、正規化されたSnapshotの抽象がない。そのため`src/generate.rs`と`src/validate.rs`がそれぞれ独自に`fs::read_dir`で`knowledge/`を走査しており、走査ロジックが重複している。
- `changes.rs`の`historical_testcases_by_feature`は、マイルストーンごとに`git worktree add`→`generate_testcases`→`git worktree remove`を実行する。`markharness backfill run`(UC6、大規模既存リポジトリ向けの優先度付きバックフィル、PROJECT.md)は多数のマイルストーンペアを処理する設計であり、このworktree生成コストはbackfillのスケールに直結する。

ユーザーから提供された設計提案「markharness アーキテクチャ設計提案」(2026-08-18)は、これらをDomain/Application/Infrastructureへ分離するレイヤー構成を示した。レビューの結果、現状分析は実装と一致しており(上記の各行はコード確認済み)、13章「採用しない設計」の判断もCLAUDE.mdの「後方互換性のための設計を排除し常に最善を目指す」「デメリットに工数を含めない」という運用ルールと整合していた。一方で、提案の`ChangeAnalyzer`インターフェースが[decisions/0008](./0008-verification-plan-product-roadmap.md)のロードマップと1点衝突していたため、本ADRで修正の上、採否を決定する。

## 決定

### 1. 5つのDomain Moduleを採用する

`KnowledgeWorkspace`・`TestcaseCompiler`・`ChangeAnalyzer`・`VerificationEngine`・`BackfillCoordinator`をDomain層の中核とする。各Moduleが呼び出し側に見せるInterfaceと内部へ隠す処理の詳細は設計文書を正とする。

### 2. CLI → Application → Domain → Infrastructureの一方向依存を採用する

`CommandOutcome`型と`Presenter` traitを導入し、Domain層・Application層から`println!`/`eprintln!`/`std::process::exit`を排除する。人間向け出力とJSON出力は同じ`CommandOutcome`から生成する。

### 3. `ChangeAnalyzer`の版参照は`MilestoneRef`固定ではなく`CommitRef`ベースに一般化する(元提案からの修正)

元提案は`ChangeAnalyzer::compute(from: MilestoneRef, to: MilestoneRef, options: ChangeOptions)`としていたが、[decisions/0008](./0008-verification-plan-product-roadmap.md) Stage 2は「milestone-only UXを、PR base/headをfirst-classに追加した共通version rangeへ一般化する」ことを既に決定済みである。`MilestoneRef`に型を固定したままPhase4で`ChangeAnalyzer`を確定させると、Stage 2着手時に中核Interfaceの再設計が必要になり手戻りが生じる。そのため本ADRでは以下を採用する。

```rust
enum CommitRef {
    Milestone(MilestoneId),  // タグ名。内部でgit tagをcommitへ解決する
    Commit(CommitId),        // 任意のcommit(PRのbase/head SHA等)
}

impl ChangeAnalyzer {
    fn compute(
        &self,
        from: CommitRef,
        to: CommitRef,
        options: ChangeOptions,
    ) -> Result<ChangeSet>;
}
```

既存の`markharness changes compute`と`backfill run`は`CommitRef::Milestone`を使い続ける(挙動は変わらない)。[decisions/0008](./0008-verification-plan-product-roadmap.md) Stage 2で追加するPR Verification Plan機能は、同じ`ChangeAnalyzer`に対して`CommitRef::Commit`を渡すだけで済み、Interfaceの再設計を要しない。

### 4. `GitRepository` traitは今は導入せず、まず`git.rs`への集約を先行する(元提案からの修正)

元提案7.1は`GitRepository` trait定義を完全な形で先出ししていたが、これは提案自身の13章が掲げる「一つしか実装がない依存まで抽象化しない」という原則と整合しない。本ADRでは、Phase1で`changes.rs`内の直接git呼び出しを`git.rs`へ集約するところまでを決定する。trait化は、テスト用のfake実装が具体的に必要になった、または複数Adapterが要件化した、など明確な必要性が生じた段階の判断に委ね、現時点でtraitの型は確定しない。

### 5. `KnowledgeSource` traitを採用する

`WorkingTreeKnowledgeSource`/`GitTreeKnowledgeSource`の2 Adapterが最初から明確なため、上記4とは異なりtrait化の妥当性がある。`GitTreeKnowledgeSource`により、`historical_testcases_by_feature`の`git worktree add`/`remove`をcommit配下のtree/blob直接読込に置き換え、backfillのスケールコストを下げる。

### 6. 生成物更新の原子性を採用する

`generate`によるTestCaseとtraceability indexの更新を、ディレクトリ全体でトランザクション化する(一時ディレクトリへ全生成→検証→`generated/`への切り替え)。途中失敗時は既存生成物を保持する。

### 7. 全生成を正準動作として維持し、増分生成は最適化として追加する

初期段階から増分生成のみで運用することは採用しない。増分生成を追加する場合も、CIでの定期的な全生成による検証を前提とする(13章の判断を維持)。

### 8. 段階的移行計画を採用する

| Phase | 内容 |
|---|---|
| Phase 1 | `compute_changes`のbool引数を`ChangeOptions`へ、`changes.rs`内の直接git呼び出しを`git.rs`へ集約。既存動作とCLI契約をCharacterization Testで固定。ディレクトリ構成は変更しない。 |
| Phase 2 | `CommandOutcome`導入。CLIから`std::process::exit`をPresenterへ移動。人間向け/JSON Presenterを分離。`generate`・`changes compute`・`verify pending`からApplication Use Caseを抽出。 |
| Phase 3 | TestCaseとtraceability indexの生成を一時領域で行い、成功後に`generated/`へ反映する原子性を追加。 |
| Phase 4 | `KnowledgeSnapshot`導入。`TestcaseCompiler`をファイルシステムから分離。VerificationのData LoaderとEngineを分離。`ChangeAnalyzer`は決定3の`CommitRef`ベースで確定させる。 |
| Phase 5 | `KnowledgeSource`導入。`GitTreeKnowledgeSource`でworktreeを置き換え。Feature/ChangeEvent/Executionの再構築可能な索引を追加。Backfillに`--max-pairs`/`--time-budget`等の処理量制限を追加。 |

詳細なInterface定義、Mermaid図、コード構成、テスト戦略は[domain-application-infrastructure-layering-design.md](../design/domain-application-infrastructure-layering-design.md)を参照。

## 結果

- `cli.rs`への変更集中が解消され、機能追加時の変更範囲が予測しやすくなる。
- `ChangeAnalyzer`が[decisions/0008](./0008-verification-plan-product-roadmap.md) Stage 2のPR Verification Plan機能を、後方非互換な再設計なしに受け入れられる。
- `GitRepository` traitの導入を先送りすることで、単一実装しかない依存を抽象化する無駄なInterfaceを避けられる。
- Phase1〜3は既存のCLI契約(終了コード、JSON出力の形)を変えないため、ユーザー影響なくリファクタリングできる。
- 増分生成の追加後も全生成が正準動作であり続けるため、キャッシュ不整合の発見が容易な状態を維持できる。

## 検討したが採用しない選択肢

- **マイクロサービス化**:ローカルGitリポジトリ内の知識管理という性質に対し、ネットワーク・認証・分散トランザクション・運用基盤の複雑さが過大になるため採用しない。
- **正準データとしてのRDB**:Gitの履歴・レビュー・branch/tag運用と二重の正準データが生じるため採用しない。SQLite等を使う場合も再構築可能な索引用途に限定する。
- **全依存のtrait化(`GitRepository`を含む)**:一つしか実装がない依存まで抽象化するとInterfaceが増え保守性を下げる。`KnowledgeSource`のように複数Adapterが具体的に必要なseamだけを抽象化する(決定4・5)。
- **初期段階からの増分生成のみの運用**:キャッシュ破損・削除検出・正規化ルール変更による不整合を発見しにくいため採用しない。
- **`ChangeAnalyzer`を`MilestoneRef`固定のまま採用する(元提案どおり)**:[decisions/0008](./0008-verification-plan-product-roadmap.md) Stage 2との衝突により、PR Verification Plan着手時に中核Interfaceの再設計という手戻りが生じるため不採用とし、`CommitRef`へ一般化する(決定3)。
