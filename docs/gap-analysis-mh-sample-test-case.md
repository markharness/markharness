# mh-sample-test-case 実行結果と設計書（統合版）の相違点

**Status**: Survey(調査時点のスナップショット)
**関連ドキュメント**: [テスト知識管理のGit-nativeモデル_統合版.md](./テスト知識管理のGit-nativeモデル_統合版.md)（以下「設計書」）、[gap-analysis-mm-folder.md](./gap-analysis-mm-folder.md)(`mm`フォルダを対象にした別調査)
**調査範囲**：本リポジトリ（`mh-sample-test-case`）の追跡対象ファイル・作業ツリー・`git log`

**位置づけ**：本資料は設計書を、TODOアプリを題材にした実際のケーススタディ運用リポジトリ `mh-sample-test-case`（本フォルダ、`markharness` CLIを実際に3マイルストーン分運用した記録）と突き合わせ、確認できた相違点を整理したもの。設計書自身が§3.6「実装状況まとめ」で `markharness`（CLI本体、`C:\Users\papa\work\markharness`）のコード実装と設計の相違をすでに整理済みだが、本資料はそれとは別に、**実際にこのCLIを使って運用した結果（本リポジトリのコミット履歴・生成物）が設計書の記述とどう異なるか**を確認する。

---

## 1. 全体評価

本リポジトリの運用は、設計書が核心的貢献と位置づける3点をいずれも実地で確認できている。

- Feature系譜キーとしてtree SHAを使い、`feature.yml`単体が不変でもConditionやExpectedResult配下の変更が検知される（後述2章の実例で確認）。
- TestCaseが`knowledge/`から分離され`generated/`配下の派生物として管理されている。
- `changes/test2.yaml`・`changes/test3.yaml`がマイルストーン境界（`git tag test1/test2/test3`）で自動生成されている。

その一方で、**設計書の本文にはまったく記述がない「TestExecutionとChangeEventの自動突合」機能（`verified_feature_tree_shas` / `markharness verify trace` / `markharness verify pending`）が、本リポジトリの全実行結果に既に組み込まれて使われている**（3章）。これは設計書の欠落というより、設計書執筆後にCLI側で追加された機能が、本ケーススタディ側では先行して使われている状態であり、設計書と実運用のずれとして記録に値する。

その他、`forked_from`・`change_type`・`schema/`バリデーション・`markharness changes lineage`など、CLI側には実装済みでも本ケーススタディの運用では一度も使われていない機能がある（4章）。

---

## 2. tree SHAベースの系譜検知（設計書第3.1節）：実例で確認

設計書第3.1節は「`feature.yml`単体のblob SHAではなく、Featureディレクトリ全体のtree SHAを比較する」という修正を記す。本リポジトリの`test2`→`test3`間のChangeEventがこれを裏付ける実例になっている。

```
$ git diff test2 test3 -- knowledge/todo-simple/todo-add
diff --git a/knowledge/todo-simple/todo-add/todo-add-from-form/todo-add-valid-title/expected/004.yml
+++ (新規ファイル追加のみ、feature.yml自体は無変更)
```

`todo-add/feature.yml`自体は`test2`→`test3`間で一切変更されていないが、配下の`expected/004.yml`が新規追加されたことで`changes/test3.yaml`に`todo-add`のChangeEvent（`from_tree_sha: 79324b98…` → `to_tree_sha: ef424d86…`）が正しく記録されている。もし設計初期案どおり`feature.yml`単体のblob SHAで判定していれば、この変更は検知漏れになっていたはずであり、設計書が指摘する不具合と修正が実データで再現・確認できる形になっている。

---

## 3. TestExecution↔ChangeEvent連動（設計書には記述なし）

本リポジトリの`executions/*/results.yml`は、設計書のER図・第3.1節・第3.5節が説明する`TESTEXECUTION`のフィールド（`case_id` / `result` / 実行者・日時相当）に加え、**設計書に一度も登場しない`verified_feature_tree_shas`フィールド**を全レコードに持つ。

```yaml
- case_id: tc-edit-existing-todo-001
  result: pass
  executor: soreiyu52
  executed_at: 2026-08-09T17:08:29Z
  verified_feature_tree_shas:
    todo-edit: 0b769f0d5ed46a92798107bcd4256c1513a21e8e
```

これは`generated_from.feature`（TestCaseの生成元Feature）ごとに、実行時点でのFeatureディレクトリのtree SHAを記録するもので、`markharness verify trace <case_id> --milestone <m>`（その実行がどのChangeEventを反映しているか）・`markharness verify pending`（未再検証のTestCaseを機械的に検出）というQ1/Q2判定を可能にする。設計書第3.5節・図4が述べる「変更影響の伝播」→「再確認が必要なTestCase集合」は、設計書内では静的な生成グラフ止まりで説明されているが、実際のCLIはさらに一歩進んで**実行結果側からも変更の反映状況を機械的に追跡できる機能を持ち、本リポジトリはそれを最初の実行(`test1`)から一貫して使っている**。

この機能とその設計意図は、設計書と同じ`markharness`リポジトリ内の別紙「[`change-event-verification-tracking-spec.md`](./change-event-verification-tracking-spec.md)」にのみ記載されており、統合版本文（本フォルダ`docs/`にあるファイル）には章立ても言及もない。設計書を単体で読む限りこの機能の存在は分からず、**「論文の完成度」と「CLIの実際の機能」に乖離がある**点は特筆に値する。

同様に、`generate`が同時生成する`generated/traceability-index.json`（Requirement→Feature→Behavior→Condition→TestCaseの索引、`axis`はRequirement/Feature/Behaviorの3階層をunionしたもの）も設計書第3.5節のディレクトリ構造図に登場しない生成物であり、本リポジトリには実在し`markharness verify`の差分検証対象にもなっている。

---

## 4. 設計書に記述はあるが本ケーススタディでは未使用の機能

CLI側（`markharness`）には実装済みだが、本リポジトリの実際の運用（`memo.md`に記録された操作列、および`git log`）では一度も使われていないもの。

| 機能 | 設計書の該当節 | 本リポジトリでの使用状況 |
|---|---|---|
| `forked_from`（概念的派生の手動記述） | 第3.1節 | 4つのFeature（`todo-add`/`todo-complete`/`todo-delete`/`todo-edit`）いずれも`feature.yml`に`forked_from`キー自体が存在しない。分岐なしのTODOアプリという題材上、使う場面が発生していない |
| `change_type`（`markharness changes annotate`による事後入力） | 第3.5節 | `changes/test2.yaml`・`changes/test3.yaml`の`change_type`はいずれも`null`のまま。`annotate`コマンドは`memo.md`の操作列に一度も現れない |
| `markharness validate`（JSON Schemaバリデーション＋axis/forked_from相互参照チェック） | 第3.5節・第3.6節 | `schema/*.schema.json`一式は`markharness init`により配置されGit管理下にあるが（唯一Git追跡対象になっている本資料関連ディレクトリ）、`memo.md`の操作列に`markharness validate`の呼び出しが一度もない。スキーマが実際に本リポジトリの`knowledge/`・`axes/`を検証済みかは確認できない |
| `markharness changes lineage --commit <sha>`（`git merge-base`祖先探索・2親分岐監査） | 第3.2節 | `git log --all --oneline --graph`の通り、本リポジトリの履歴は`first commit`から`test2-test3 ChangeEventを自動計算`まで完全な線形履歴（ブランチ分岐・マージなし）であり、`lineage`が扱う「真の分岐」ケース自体が発生していない |

これらはいずれも「実装されていない」のではなく、「単一ブランチ・単一担当者の小規模ケーススタディでは発生しない、または使う動機がなかった」機能であり、設計書第5.2節が評価対象とする「複数世代・複数ブランチにまたがる変更影響識別タスク」の検証には、本リポジトリのような単純な逐次運用だけでは不十分であることを示唆する。

---

## 5. ディレクトリ構造・付随ファイルの相違（設計書第3.5節）

設計書§3.5「実装状況」は`REQUIREMENT`の明示ファイル化・`changes/`のマイルストーン単位ファイル形式については既に注記済みで、本リポジトリの実データもこれと一致する（`knowledge/todo-simple/requirement.yml`、`changes/test2.yaml`が1区間1ファイル・複数イベント配列）。

その上で、設計書の記述からは読み取れない本リポジトリ固有の運用上の相違が2点ある。

- **`docs/`・`memo.md`・`.markharness-cache/`・`tmp/`が`.gitignore`で除外されている**：設計書は`.markharness-cache/`の非コミット化のみを明記するが（第3.3節・第3.5節、これは設計通り）、本リポジトリでは設計書自体を含む`docs/`と、操作ログである`memo.md`もGit管理対象外になっている。そのため、このリポジトリの`git log`だけを見ても「どの設計書のどのバージョンに基づいて運用したか」「どのコマンドをどの順で実行したか」はGit履歴からは追跡できず、作業ツリーの現物（`memo.md`、本資料が参照している`docs/`）に依存する。
- **`tmp/`が未取込Featureの下書き置き場として使われている**：`tmp/todo-reopen`・`tmp/todo-search`・`tmp/todo-show`という3つの未使用Feature下書きが存在し、実際に`knowledge/`へ取り込まれたのは`tmp/todo-edit`（test2で取込）・`tmp/004.yml`（test3で取込）のみ。設計書には`tmp/`のような作業領域の説明は無く、CLI実装側にも`tmp/`を特別扱いする仕組みはない（単なる開発者の作業ディレクトリ運用）。

---

## 6. TestCaseファイル命名（旧MM資料の指摘との相違）

参考として、`markharness`リポジトリの旧調査資料（[`gap-analysis-mm-folder.md`](./gap-analysis-mm-folder.md)、対象は本リポジトリとは別の`c:\Users\papa\work\mm`フォルダ）は「ファイル名と`case_id`が対応していない」問題を指摘していたが、本リポジトリの`generated/testcases/*.yml`ではファイル名（例：`todo-add-valid-title.yml`）と`case_id`（`tc-todo-add-valid-title-001`）が`tc-`接頭辞と`-001`連番を除いて一致しており、体系的に対応が取れている。この点は旧資料からの改善として確認できる。

---

## 7. 総括

| 分類 | 内容 |
|---|---|
| 設計書と一致（実データで裏付け確認） | tree SHAベースのFeature系譜検知（feature.yml不変でもExpected追加を検知、2章）、TestCase派生管理、ChangeEventのマイルストーン境界自動生成、`.markharness-cache/`の非コミット化 |
| 設計書に記述がないが実運用で使われている | `verified_feature_tree_shas`によるTestExecution↔ChangeEvent連動（`verify trace`/`verify pending`用データ、3章）、`generated/traceability-index.json`（axis 3階層union索引、3章） |
| CLIには実装済みだが本ケーススタディでは未使用 | `forked_from`、`change_type`アノテーション、`markharness validate`、`markharness changes lineage`（4章） |
| 設計書には無い運用上の要素 | `docs/`・`memo.md`・`tmp/`の`.gitignore`除外、`tmp/`を下書き置き場として使う運用（5章） |

本リポジトリは設計書の核心的主張（tree SHAベースの版履歴・ChangeEvent自動化）を単一ブランチの小規模データで実地検証できている一方、（a）設計書がまだ文書化していない実装済み機能を先取りして使っている点、（b）ブランチ分岐・`forked_from`・`change_type`・スキーマ検証など、設計書が扱うがこのケーススタディ単体では検証されない機能が残っている点の両方が確認できた。第5章が評価対象とする「複数世代・複数ブランチにまたがる変更影響識別タスク」（層β）の検証には、本リポジトリのような線形・単純運用のケーススタディに加えて、分岐・マージを含むより複雑な運用データが別途必要になる。

**注(2026-08-10追記)**：上表4章「CLIには実装済みだが本ケーススタディでは未使用」の`markharness changes lineage`の行は、本資料の調査時点(`test1`〜`test3`の線形履歴のみ)の状態を指す。第8章で追記した通り、その後`test4`として分岐・マージシナリオを追加検証した。

---

## 8. 分岐・マージを含む検証シナリオ(test4、2026-08-10追記)

improvement-prompts.md項目3への対応として、本リポジトリに分岐・マージを含む新しいケーススタディシナリオを追加検証した。4章で述べた「線形履歴のみで`lineage`の真の分岐ケースが発生していない」という制約を解消する目的で行った。既存の`test1`〜`test3`のデータ・コミット・タグは一切変更していない。

### 8.1 実施手順

1. `main`(タグ`test3`が指すコミットまでの状態)から作業ブランチ`markharness-lineage-scenario-feature`を作成した。
2. 作業ブランチ側で、`todo-add`Featureの`todo-add-valid-title`Conditionに新しい期待結果`expected/005.yml`(Enterキーショートカットでの追加)を追加してコミットした。
3. `main`側では、同じ`todo-add-valid-title`Conditionの既存`expected/004.yml`(成功ポップアップの説明文)を書き換えてコミットした。異なるファイルへの変更のため、マージ時のコンフリクトは発生しない設計にした。
4. `main`に作業ブランチを`--no-ff`でマージし、マージコミットに`test4`タグを付けた。
5. `markharness changes compute test3 test4`と`markharness changes lineage --commit <test4のコミットSHA>`をそれぞれ実行した。

### 8.2 実行結果

`markharness changes lineage --commit <merge-sha>`の出力:

```
todo-add: true_divergence
todo-complete: single_parent
todo-delete: single_parent
todo-edit: single_parent
```

`markharness changes compute test3 test4`が生成した`changes/test4.yaml`:

```yaml
- event_id: todo-add--test3--test4
  feature_id: todo-add
  from_milestone: test3
  to_milestone: test4
  from_tree_sha: ef424d86ed44f5810063ab8e8b44d2595257c7bf
  to_tree_sha: 44ad6d3f88ebac3b10eedeee5ed810b81cb92720
  impacted_testcases:
  - tc-todo-add-valid-title-001
  change_type: null
  from_tree_shas:
  - 2f878abf04e222b5b7e553db42bba54b8007179a
  - f0f91f81d3f584ff269703b17a9277f114eb282f
```

生成された`.markharness-cache/test4.json`(抜粋、`markharness changes compute`を`--no-cache`なしで再実行して確認):

```json
{"key":{"tree_sha":"027cd6309a4cd5149338833e2ffb3dce3ceb7ddf","canonicalization_rule_version":"1","id_index_schema_version":"1","tool_version":"0.1.0"},"entries":[{"id":"todo-add","path":"knowledge/todo-simple/todo-add","tree_sha":"44ad6d3f88ebac3b10eedeee5ed810b81cb92720"}, ...]}
```

### 8.3 想定通りだった点

- `todo-add`のみが`true_divergence`と判定され、分岐・マージに関与していない他の3 Feature(`todo-complete`/`todo-delete`/`todo-edit`)はいずれも`single_parent`と判定された。設計書§3.2の場合分けと一致する。
- `changes/test4.yaml`の`from_tree_shas`に、`lineage`コマンドが個別に報告した`true_divergence`のケースと同じ2つの親tree SHAが記録された。これは本改善サイクル(improvement-prompts.md項目2)で実装した「`to_milestone`が直接マージコミットの場合の`lineage`統合」が、単体テスト(`markharness`リポジトリ側のtempdir上のテスト)だけでなく、実際の複数コミット・複数Featureを持つケーススタディリポジトリでも設計通りに機能することを確認できた初めての実例である。

### 8.4 想定と異なった点・留意事項

- `from_tree_sha`(単一値)には`test3`時点のtree SHAがそのまま記録され、`from_tree_shas`(2親)と共存する形になった。設計書はこの2つのフィールドの併存について「線形履歴の表現として`from_tree_sha`を維持する」とのみ記しており、実際に両方が同時に埋まったレコードを見るのは今回が初めてである。値として矛盾はしていない(`from_tree_sha`は主系譜の単純な2点比較結果、`from_tree_shas`はマージコミット固有の2親情報)が、`verify trace`/`verify pending`(§3.7)のようにこのレコードを消費する将来のツールが両フィールドをどう使い分けるかは、本シナリオでは検証しておらず今後の課題として残る。
- 本シナリオはあくまで「`to_milestone`タグが直接マージコミットを指す」最も単純なケースであり、改善プロンプト項目2で明記した統合範囲の限界(マイルストーン区間内の任意の位置でのマージには非対応)は未検証のまま残っている。

### 8.5 リポジトリへの影響

- 新規ブランチ`markharness-lineage-scenario-feature`、マージコミット、`test4`タグを追加した。いずれもリモートへはpushしていない(ローカルのみ)。
- 既存の`test1`〜`test3`のコミット・タグ・`changes/test2.yaml`・`changes/test3.yaml`・`executions/`配下は変更していない。新規追加は`changes/test4.yaml`と`.markharness-cache/test4.json`(`.gitignore`対象で非コミット)のみ。
