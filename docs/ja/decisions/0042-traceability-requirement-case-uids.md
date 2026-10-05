# 0042: `traceability`で、要求ごとの紐づくケースを`coverage`と同じ規則で示す

## ステータス

Accepted(2026-10-06決定)。

## 背景

GUIは、要求の一覧で、各要求に紐づくケースの数と題名を見せたい。「現在の作業」を見せるには、未コミットの編集を含む作業ツリーを読む`traceability`を使う([0033](./0033-traceability-defaults-to-working-tree.md))。しかし、要求とケースの紐づきは、`coverage`の`requirements[].cases`にしかなかった。`coverage`はコミット済みの内容しか読まない。

`traceability`の`relations`から、GUIが紐づきを計算すると、`coverage`と食い違う。ケースが要求に紐づく規則は、Scenarioが`contributes_to`を1件以上持てば、そのScenarioが指す要求にだけ紐づき、持たなければFeatureの`contributes_to`にフォールバックする([0031](./0031-scenario-level-requirement-contribution.md))。`relations`のFeature由来とScenario由来を足すと、この規則と合わず、実際にケース数が食い違った(Issue #113)。規則をGUIが写して持つと、コアの規則が変わるたびにずれる。

## 決定

### 1. `requirements[]`の各項目に`case_uids`を加える

`case_uids`は、その要求に紐づくTestCaseの`case_uid`の配列で、昇順に並べる。`requirement_uid`が`null`の要求では空配列にする。`case_uid`を持たないTestCase(`identity migrate`未実行)は、`case_uid`で示せないので含めない。

`traceability`は`--at`の有無にかかわらず出す。`--at`なしでは作業ツリーを読むので、未コミットの編集が反映される。

### 2. 規則は、`coverage`と同じ関数を呼ぶ

「どのTestCaseが、どの要求に紐づくか」は、`generate::testcases_for_requirement`の1か所にだけ置く。`coverage`と`traceability`の両方がこれを呼ぶ。TestCaseが持つ有効な`requirement_uids`(Scenarioの上書きを解決済み)と照合する。2つの出力が食い違わないことは、テストで固定する。

### 3. `relations`には足さない

`relations`は、Feature・Scenarioが要求へ`contributes_to`する宣言と、TestCaseが生成元のScenarioを指す`generated_from`という、書かれた関係をそのまま返す。要求からケースへの紐づきは、Scenarioの上書きを解決した導出結果である。同じ配列に混ぜると、宣言と導出の区別が消え、`contributes_to`を足し合わせる誤読を招く。

### 4. 出力の契約

`schema/traceability-read-model.schema.json`の`requirement`に、必須の`case_uids`を加える。既存のフィールドは変えない。`schema_version`は1のままにする([0039](./0039-read-output-schema-version-frozen-in-prototype.md))。

## 検討したが採用しない選択肢

- **`relations`に、要求からケースへの新しい種類(例: `covered_by`)を足す**: 上記3の理由で採らない。
- **GUIが`relations`から計算する**: Issue #113のとおり、規則が食い違う。規則の二重実装を避けるのが、GUIの方針である。
- **`coverage`を作業ツリー対応にする**: `binding`と`reference_status`がコミットを前提とし、変更の範囲が広い。必要になった時点で扱う。
