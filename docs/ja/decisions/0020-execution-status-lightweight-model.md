# 0020: 実行ステータスを軽量化し、証跡管理をmarkharnessの責務から外す

## ステータス

Accepted(2026-09-12実装完了。`checklist-v2-core.md`参照)。[markharness-v2-design.md](../design/markharness-v2-design.md)の再設計grillingセッション(2026-09-11)に基づく。[0017](0017-scenario-case-revision-and-execution-evidence.md)§5「実行証跡と適用可能性」が定めるEvidence適用可能性モデルを、本ADRの範囲(TestCaseに付与する実行実績の記録)について置き換える。§1〜4(Scenario/共通手順/Case revision/分割・統合)は本ADRの対象外であり、そのまま有効。[0025](0025-v2-forward-compatible-evolution.md)により型名は`ExecutionBinding`へ変更されたが、本ADRの軽量化判断と保持情報は有効である。

## 背景

[0017](0017-scenario-case-revision-and-execution-evidence.md)は、実行結果・適用可能性・証跡の有無を分離し、Case UID・Case revision・対象ビルド・要求環境がすべて一致した場合のみ証跡を合格に採用する、という厳密なEvidence Applicabilityモデルを構想していた。これを土台に、後続の[markharness-v2-design.md](../design/markharness-v2-design.md)旧版はさらにEvidence/EvidenceSelection/Execution Manifest/Implementation revision/Environment matrixという重量級の型群へ拡張していた。

grillingセッションでNorth Star(このツールを使い終えた人が得たい答え)を改めて言語化した結果、実行結果について実際に必要なのは次の水準であることが分かった。

- 「前回リリースでどのテストが実行されたか」は、pass/fail結果よりも実行スコープ(対象に入っていたか)が主目的であり、結果は補助情報でよい。
- 詳細な証跡(スクリーンショット・ログ・実行日時・実行者)の管理はmarkharnessの責務ではなく、別ツール(現状はExcelでのチェック記録)に委ねる。
- 一方で、「自動実行か手動実行か」の区別だけは、レビュー時に「テストコードを直接確認できるか、別資料を参照する必要があるか」の判断に直結するため必要。
- テスト実行環境(ブラウザ/OS等)の区別は、テストケース自体の性質ではなく仕様書側・実行ツール(Playwright等)側の責務であり、markharnessのドメインに含めない。

この水準は、[0017](0017-scenario-case-revision-and-execution-evidence.md)§5が想定していたビルド・環境まで含めた厳密な突合よりも明確に小さく、`Evidence`/`EvidenceSelection`/`Manifest`/`Implementation revision`/`Environment`という型群を新たに実装するコストに見合わない(YAGNI)。

## 決定

### 1. Execution statusを1軸+任意の参照文字列に限定する

TestCaseに付与する実行実績は、次の2フィールドのみを持つ。

```text
ExecutionStatus {
  mode: automated | manual,
  reference: string (optional, free text — テストコードへのパスやURL等)
}
```

pass/fail/skip等の詳細結果、実行日時、実行者、対象ビルド、実行環境、証跡本体(スクリーンショット等)は保持しない。これらが必要な場合は、`reference`が指す先(別ツール)を参照する。

TestCaseの参照は表示IDではなく**Case UID**で行う([0013](0013-immutable-identity-model.md))。表示idのrenameで記録が切れないようにするためである。

用語上の注意：この記録は「実行された事実」ではなく**検証手段(自動/手動)とその参照先**を表す。実行日時・回数・合否を持たないため、値の存在を「最新のCase revisionで実行済み」と読んではならない。「Execution status」という名前はこの限界を含意しないため、実装時にフィールド名を変える場合も、意味は本節の定義を正とする。

### 2. 「実行されたか/されていないか」の判定に環境・ビルドの一致は求めない

[0017](0017-scenario-case-revision-and-execution-evidence.md)§5が要求していた「Case UID・Case revision・対象ビルド・要求環境の一致」という厳密な適用可能性判定は導入しない。Change Impact/Release Coverageの算出では、TestCaseに紐づくExecution statusの有無と`mode`だけを参照する。ビルド・環境単位での正確な合否管理が必要になった場合は、その具体的な要求が出た時点で別ADRとして再検討する。

### 3. 証跡の保存・管理はmarkharnessの範囲外とする

Evidence本体の不変保存、EvidenceSelection、Execution Manifestという概念・型はmarkharnessに導入しない。CI連携による自動証跡取込も、要望が出た時点で改めて設計する後回し機能とし、MVPでは`ExecutionStatus`をCLIで人が記録する運用にとどめる。

### 4. データ構造はPlaywrightとの将来連携を想定して定義する

自動テストツールへの直接連携(CI連携、reporter取込)はMVP範囲外とするが、`mode: automated`かつ`reference`にテストファイルパスを持つという形は、将来Playwrightのinventory/annotationと接続する際に無理なく対応づけられるよう意識する。ツール固有のフィールド(project、locator等)は現時点では追加しない。

## 影響範囲

- [0017](0017-scenario-case-revision-and-execution-evidence.md)§5で構想されていたEvidence Applicabilityモデルは実装しない。§5のステータス欄に本ADRへの参照を追記する。
- 現行の`src/execution.rs`が持つ`target_revision`・`environment`フィールドは、本ADRの方針に沿って実装時に縮小対象となる(実装作業は別途チェックリスト化する)。
- [markharness-v2-design.md](../design/markharness-v2-design.md)は本ADRの内容に合わせて全面的に書き直す。

## 検討したが採用しない選択肢

- **Environment matrixを維持する**：ブラウザ/OS単位の正確な検証状況が追えるが、テストケース自体を環境に依存させる設計になり、「テストケースは機能に対する検証であり環境に依存しない」という今回の合意(grillingセッション)に反する。環境ごとの検証状況は仕様書側・実行ツール側で管理する。
- **pass/fail結果を保持する**：判断の一助として有用だが、詳細な証跡管理(いつ・誰が・どのビルドで)を伴わないpass/fail単体は誤った安心感を生みやすく、証跡管理を別ツールに委ねるという方針と整合しない。単純な「実行されたか」の1軸に留める。
