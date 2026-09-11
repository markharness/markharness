# 0019: 仕様・テストケースの対応確認をcommit trailerで記録する

## ステータス

Accepted(設計合意済み、未実装)。[markharness-v2-design.md](../design/markharness-v2-design.md)の再設計grillingセッション(2026-09-11)に基づく。

## 背景

開発者が機能・仕様(Requirement)・テストケースのいずれかを変更した際、チームは「関連する他方が追随して更新されたか、更新不要と確認済みか」を知りたい。単に「両方のファイルが同じPRで更新された」ことや「再実行がpassした」ことだけでは、人が意味的な整合を確認したとは判定できない。

当初案([markharness-v2-design.md](../design/markharness-v2-design.md)旧版§6.7)は、`AlignmentObligation`/`AlignmentDecision`という独立したDomain型を導入し、actor・rationale・review_referenceを持つ確認記録をGit管理下に保存する設計だった。grillingセッションでは、この確認記録を「極力軽量にしたい」「運用でgit commentやnoteなどで残せれば十分」という要望が示された。

`git notes`を候補として検討したが、次の理由で不採用とした。

- `git push`/`git fetch`のデフォルト対象に含まれず、明示的なrefspec設定なしにはチームへ共有されない。
- GitHubのWeb UI上に一切表示されず、PRレビューの場で見えない。
- rebaseでcommit SHAが変わると、`notes.rewriteRef`を設定しない限り追随しない。

markharness自身が`backfill run`の進捗管理にgit notesを使っている前例はあるが、それは人間が直接参照する必要のない内部状態の保存であり、今回想定する「チームがレビューで見る確認記録」という用途には当てはまらない。

## 決定

### 1. 確認記録はcommit trailerに書く

仕様変更またはテストケース変更を含むコミットのメッセージ末尾に、確認内容を示すtrailerを付与する。

```text
Spec-Reviewed: no-change-required
```

変更が不要と判断した場合は`no-change-required`を書く。trailerが必要になるのは「見て、あえて変更しなかった」ことを明示する場合に限る。

対応する変更を別コミットで行った場合、その変更自体は「関連側も変更されている」という事実を示すにとどまり、意味の整合を人が確認した証拠にはならない(本ADR背景の前提)。したがってCoreの出力は**追随変更あり／確認済み／未確認**の三値とし、同時更新を「確認済み」と同一視しない([markharness-v2-design.md](../design/markharness-v2-design.md)§5.3)。

確認は、トレーラーを含むコミット時点のRequirement UIDとCase UIDの組に結び付ける。対象IDは当該コミットで解決し、片側だけの指定から変更内容・関連を用いて相手を一意に特定できない場合は採用しない。その場合は両側を明示する。版はGitから解決し、手入力の版文字列や独立した保存型は要求しない。同一区間内の後続コミットで、組のどちらかの実効内容(TestCaseはCase revision、Requirementは`requirement.yml`または`.sdoc` blob)が変更された場合、その組の確認を無効化する。別の組への確認の流用や、後から追加されたケースへの拡張はしない。無効な記録は「確認済み」の根拠にせず、有効な別記録がなければ§5.3の規則に従い「追随変更あり」または「未確認」を出力する。

trailerのvalueは**対象要素を識別できる形**にする(例: `Spec-Reviewed: no-change-required (req-login-01)`)。1つのコミットが複数のRequirement/TestCaseに触れる場合、対象を持たないtrailerではどの対応確認が済んだのか判定できず、自動判定が「未確認」を落とす。

判定実装は`git log base..head`の各コミット本文を走査する。squash mergeされたPRでは元のtrailer行がmerge commit本文の途中に埋め込まれ得るため、「最終行のtrailerだけを見る」実装にしない。

trailerの具体的なkey名・value語彙・複数件の記法は実装設計で確定する。本ADRはcommit trailerという記録場所と、対象を識別する必要があること、およびその選定理由を確定するものであり、書式の詳細確定ではない。

### 2. 記録は変更のコミットと同時に行うことを前提とする

運用は、変更を加える本人が、変更のコミットと同時にtrailerを書き添える形を基本とする。commit trailerは他のcommit本文と同様にpush/fetch/GitHub表示すべてに標準対応するが、コミット後に追記することはできない(新しいコミットが必要)。この制約は、テストケース修正が仕様変更より先行することが多いという実務(要件確認セッションでの合意)に照らし、当面許容する。

### 3. 独立したDomain型・承認ワークフローは作らない

`AlignmentObligation`/`AlignmentDecision`のような専用のDomain型、担当者割当、承認ステータス遷移は導入しない。対応確認の要否検出(自動判定)自体はCoreの計算対象とするが、確認済みの記録はGitのcommit履歴そのものに委ね、専用の永続ストアを持たない。

## 検討したが採用しない選択肢

- **git notes**：背景節の理由により不採用。
- **PRの説明・コメント**：レビュー時の自然な置き場所だが、markharnessから機械的に読み取るにはGitHub等の外部API連携が必要で、「Gitだけで完結する」という制約に反する。将来、外部連携が必要になった時点で別ADRとして再検討する。
- **リポジトリ内の専用ファイル**(例: `.markharness/alignment/`配下)：Git管理下に置けるが、専用ファイル形式・スキーマ管理が新たに必要になり、commit trailerより複雑になる。「後から追記できない」制約が実運用上の障害になった場合の代替として保留する。

## 「後から追記できない」制約への対応が必要になった場合

将来、「コミット後に気づいた確認」を頻繁に記録する必要が生じた場合は、リポジトリ内の専用ファイル方式への切替を別ADRで検討する。本ADRの時点では、そのニーズが実際に生じたという証拠がないため、先回りして専用ファイル形式を導入しない([CLAUDE.md](../../../CLAUDE.md)所定のYAGNI原則)。
