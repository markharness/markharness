# ChangeEvent連動：実行状態追跡（Verification Status Tracking）仕様書

**位置づけ**：本資料は「テスト知識管理のGit-nativeモデル_統合版V2.md」（以下「統合版V2」）第3.5節・図4が構想する「ChangeEventを起点とした影響TestCase特定→再確認」のうち、**実行結果側との連動**を具体化するための追加仕様である。統合版V2はChangeEventの自動生成とimpacted_testcasesの特定までを核心的貢献としており、「その後、実際に再実行されたか」を自動判定する仕組みは第7章（Future Work）相当の未確定領域だった。本資料はこの領域を仕様化する。

対象読者：`markharness`（またはその後継ツール）の実装者。

---

## 1. 解決したい問題

現状（MMフォルダの実装）で自動的に答えられない、しかし運用上は必ず聞かれる2つの問いを解決する。

- **Q1（遡及）**：「このTestExecutionの結果は、Featureのどの変更を反映した状態に対する実行か」
- **Q2（前方）**：「ChangeEventでimpacted_testcasesに挙がったTestCaseのうち、まだ再実行されていないものはどれか」

現行実装では、`executions/<milestone>/results.yml`が`case_id / result / executor / executed_at`のみを保持し、`changes/<from>-<to>.yaml`のimpacted_testcasesとの突き合わせは人間が目視で行っている。この突合を自動化する。

---

## 2. データモデル拡張

### 2.1 TESTEXECUTIONへのフィールド追加

統合版V2のER図（第3.1節）における`TESTEXECUTION`に、以下を追加する。

```yaml
# executions/<milestone>/results.yml の1レコード
case_id: tc-edit-existing-todo-001
result: pass
executor: soreiyu52
executed_at: 2026-08-08T16:38:52Z
verified_feature_tree_shas:        # 追加フィールド
  todo-edit: 4f2c9a1e8b3d5670012ab34cd56ef7890a1b2c3
```

- `verified_feature_tree_shas`：そのTestCaseの`generated_from`（第2.2節参照）に列挙されるFeatureそれぞれについて、**実行時点のマイルストーンでのFeatureディレクトリ全体のtree SHA**（feature.yml自身だけでなく、配下のBehavior/Condition/ExpectedResultを含むディレクトリ全体のGitツリーオブジェクトSHA）を記録する。feature.yml単体のblob SHAではない点に注意（第7章参照：単体blobだとCondition/ExpectedResultの変更を検知できない）。
- 記録タイミングは実行結果の登録時。値は`id_index`キャッシュ（統合版V2第3.3節）から、対象マイルストーンにおける該当FeatureディレクトリのtreeオブジェクトSHAを引いて機械的に埋める。人間が手入力する項目ではない。
- 複数Featureにまたがる複合TestCase（将来的にBehaviorが複数Featureを跨ぐ場合）にも対応できるよう、単一値ではなくマップ形式とする。

### 2.2 TESTCASEの`generated_from`は既存のまま利用

`generated/testcases/*.yml`の`generated_from.feature`（例：`todo-edit`）は既にFeature idを保持しているため、`verified_feature_tree_shas`のキーとして流用できる。スキーマ変更は不要。

### 2.3 ChangeEventへの`resolved`状態は持たせない

ChangeEvent（`changes/*.yaml`）自体に「再確認済みフラグ」を持たせる設計は採らない。理由：

- ChangeEventはマイルストーン境界の差分という**不変の事実記録**であり、後から書き換える対象にすべきではない（統合版V2第3.4節の設計思想と整合）。
- 「再確認済みか」はChangeEventとTestExecutionという2つの独立した事実系列を**都度計算**すれば導出できる派生情報であり、どちらかのソースに書き戻す必要がない。

---

## 3. 判定アルゴリズム

### 3.1 Q1：この結果はどの変更の後の実行か

入力：`case_id`, `milestone`（対象TestExecutionレコード）

1. `results.yml`から対象レコードの`verified_feature_tree_shas`を取得する。
2. 各Feature idについて、`changes/`配下の全ChangeEventレコードを`to_tree_sha == verified_feature_tree_shas[feature_id]`で検索する。
3. 一致するChangeEventの`event_id`・`from_milestone`・`to_milestone`を「この結果が反映している変更」として返す。
4. 一致するChangeEventが無い場合（＝そのマイルストーンで対象Featureに変更が無かった場合）、直近の`derived_from`鎖を遡り「最後に変更があったマイルストーン」を返す。

出力例：

```
$ markharness verify trace tc-edit-existing-todo-001 --milestone test2
case_id: tc-edit-existing-todo-001
feature: todo-edit
executed_at: 2026-08-08T16:38:52Z
reflects_change: todo-edit--test1--test2
  from_milestone: test1
  to_milestone: test2
  change_type: (未記録)
```

### 3.2 Q2：変更があったのに未実行のTestCaseはどれか

入力：`from_milestone`, `to_milestone`（比較したいマイルストーン区間。省略時は直近の隣接ペア）

1. `changes/<from>-<to>.yaml`から対象区間の全ChangeEventを読み、`impacted_testcases`を統合した集合 `Impacted` を作る。
2. `to_milestone`以降（`to_milestone`自身を含む、以降の全マイルストーン）の`results.yml`を走査し、各`case_id`について`verified_feature_tree_shas[feature_id] == changes[event].to_tree_sha`を満たすレコードが1件でもあれば「再検証済み」とする。
3. `Impacted - 再検証済み集合` を「未再実行」として出力する。
4. `to_milestone`より後に、対象Featureがさらに変更されている場合（＝to_tree_shaがすでに古くなっている場合）は、「未再実行」ではなく「対象自体が陳腐化（stale）」の区分で別掲する（3.3節）。

出力例：

```
$ markharness verify pending --from test1 --to test2
pending (再実行なし):
  - tc-edit-existing-todo-001  (todo-edit の変更 test1→test2 の影響、未実行)

stale (影響範囲がさらに変更済み):
  (なし)
```

### 3.3 「pending」と「stale」の区別

Q2をマイルストーン跨ぎで運用すると、「再実行される前に対象Featureがさらに変わってしまった」ケースが必ず発生する。これを一律「未実行」として扱うと、テスターは「どの版に対して確認すればよいか」を見失う。そこで2区分に分ける。

- **pending**：ChangeEvent発生時点のto_tree_shaに対して、まだ一度も実行記録が無い。
- **stale**：ChangeEvent発生時点のto_tree_shaに対する実行記録が無いまま、当該Featureのtree SHAがさらに新しいものに変わった（＝古い版への確認はもはや意味を持たない）。この場合、最新のChangeEventを「実質的な確認対象」として提示し直す。

判定：対象Feature idについて、`Impacted`集合の生成元ChangeEventの`to_tree_sha`が、**現在の**（問い合わせ時点の）tree SHAと一致するかを`id_index`キャッシュで確認する。一致すれば pending、不一致なら stale。

---

## 4. ツールインターフェース仕様

markharness本体のCLIサブコマンドとして以下2コマンドを実装する。

| コマンド | 用途 | 対応する問い |
|---|---|---|
| `markharness verify trace <case_id> --milestone <m>` | 指定した実行結果がどのChangeEventを反映しているかを表示 | Q1 |
| `markharness verify pending [--from <m1> --to <m2>]` | 未再実行／陳腐化したTestCase一覧を表示 | Q2 |

- 両コマンドとも読み取り専用（ファイルへの書き込みを行わない）。既存の`verified_feature_tree_shas`・`changes/*.yaml`・`.markharness-cache/`（id_index）のみを入力とする。
- CIへの組み込み：`verify pending`をマイルストーンのリリースゲートで実行し、`pending`が1件でもあれば非ゼロ終了コードを返すオプション（`--fail-on-pending`）を用意する。これにより「変更影響テストの再確認漏れ」をCIで機械的にブロックできる。

---

## 5. 既存MM実装データでのトレース例

現行の`changes/test2.yaml`・`executions/test2/results.yml`を本仕様に沿って再構成すると、以下のようになる（`verified_feature_tree_shas`は本仕様導入後に新規実行分から付与される想定であり、既存レコードには遡及適用しない。第6章参照。`to_tree_sha`はFeatureディレクトリ全体のtree SHAであり、`changes compute`の再実行によって値が変わる。以下は例示用の値）。

```yaml
# changes/test2.yaml（例示。tree SHA化により実際の値は変わる）
- event_id: todo-edit--test1--test2
  feature_id: todo-edit
  from_milestone: test1
  to_milestone: test2
  from_tree_sha: null
  to_tree_sha: 4f2c9a1e8b3d5670012ab34cd56ef7890a1b2c3
  impacted_testcases:
  - tc-edit-existing-todo-001
```

```yaml
# executions/test2/results.yml（本仕様導入後の形）
- case_id: tc-edit-existing-todo-001
  result: pass
  executor: soreiyu52
  executed_at: 2026-08-08T16:38:52Z
  verified_feature_tree_shas:
    todo-edit: 4f2c9a1e8b3d5670012ab34cd56ef7890a1b2c3
```

この2つを突き合わせると、`to_tree_sha`と`verified_feature_tree_shas.todo-edit`が一致するため、`markharness verify pending --from test1 --to test2`は`tc-edit-existing-todo-001`を pending 扱いにせず、正しく「再検証済み」と判定する。

---

## 6. 導入・移行方針

- **遡及適用はしない**：既存の`results.yml`（test1〜test3）は`verified_feature_tree_shas`を持たないため、本仕様導入前の実行記録はQ1／Q2の判定対象外（「不明」扱い）とする。統合版V2第4章のバックフィル方針と同様、id_indexキャッシュから当時のtree SHAを機械的に補完することは理論上可能だが、当面はスコープ外とし、Future Workとする。
- **`change_type`未実装との関係**：本仕様のQ1出力例に`change_type: (未記録)`とあるように、ChangeEventに`change_type`（統合版V2第3.5節、人間が入力する想定）が無い現状でも、本仕様のQ1／Q2判定自体はtree SHA比較のみで完結し、`change_type`の有無に依存しない。`change_type`が将来実装されれば、`verify pending`の出力に変更種別（仕様変更／バグ修正）でのフィルタ・グルーピングを追加できる。
- **スキーマ**：`schema/`（現状空）に`verified_feature_tree_shas`をTESTEXECUTIONスキーマの任意フィールドとして追加する。必須化はしない（人間が直接resultsファイルを編集する運用を妨げないため、CLIから記録した場合のみ自動付与される）。

---

## 7. Threats / 留意事項

- `verified_feature_tree_shas`はTestCase生成元のFeature単位で記録するため、Condition・ExpectedResultレベルの変更は自動的に捕捉される：ConditionはFeature配下のツリーの一部であり、Conditionが変わればFeatureディレクトリ全体のtree SHAも変わる。**この捕捉は`id_cache::resolve_feature_versions`がFeatureディレクトリ全体のtree SHA（`git ls-tree -r -t`でディレクトリのtreeオブジェクトSHAを取得）を比較することで実現しており、feature.yml単体のblob SHAだけを比較する実装では成立しない**（実装初期はfeature.yml単体のblobを比較しており、Condition/ExpectedResultの追加・変更を見逃す既知の不具合があった。この節の記述はその修正を前提にしている）。ただし**Feature自体は変わらずAxisレジストリ（axes/*.yml）側だけが変わるケース**は本仕様の追跡対象外であり、別途検討が必要。
- 複数のFeatureにまたがるTestCase（現状のMM実装には存在しないが、将来Behaviorが複数Featureを横断する設計になった場合）では、`verified_feature_tree_shas`が複数キーを持つため、「一部のFeatureだけ再検証され、他は未検証」という部分再検証状態が発生しうる。3.2節のアルゴリズムはこの場合、いずれか1つでも不一致があれば pending として扱う（保守的判定）。
