# 0045: 手順(procedures)の参照整合性をvalidateとreconcileで保つ

## ステータス

Accepted(2026-10-08決定)。[0027](./0027-declarative-knowledge-reconciliation.md)§5の「置換後の各要素は参照規則を満たさなければならない」が、置換される`procedures`を`use:`で参照する他のScenarioに及ぶことを定める。

## 背景

Scenarioの`use:`ステップは、所属するBehaviorの`procedures`に宣言された手順を名前で参照する([0017](./0017-scenario-case-revision-and-execution-evidence.md)§2)。手順は独立したUIDを持たず、Behaviorの値である。そのため`knowledge reconcile`で`procedures`を明示すると、collection全体が置換される(0027 §5)。

置換で手順を外す、または別名にすると、その手順を`use:`していたScenarioの参照が解決しなくなる。ところが実装では、次の2つがそれを検出しなかった。

- `knowledge reconcile`は、`use:`を、Intentに記載したScenarioについてしか検査しなかった。Intentに記載していないScenarioは、`procedures`の置換後も未検査のまま保存された。
- `markharness validate`は、`procedures`と`use:`を突き合わせなかった。

その結果、`reconcile`も`validate`も成功した後で、手順を展開する`generate`・`verify`・`traceability`が`phase step references unknown procedure`で失敗した。`validate`が通る状態と、後段が成功する状態が一致していなかった。

0027 §5の「置換後の各要素は参照規則を満たさなければならない」は、置換されたcollection自身の要素について述べた文で、置換される手順を参照する他のScenarioは対象に含むのか、明記されていなかった。

## 決定

### 1. `validate`は、すべてのScenarioの`use:`を所属Behaviorの`procedures`と突き合わせる

`use:`が、所属Behaviorの`procedures`にない名前を指していれば、そのScenarioファイルの問題として報告する。`reconcile`を経由しない手編集も、同じ規則で検出する。

### 2. `reconcile`は、`procedures`が変わるBehaviorの、Intentに記載していないScenarioも検査する

既存Behaviorの`procedures`が現在の内容から変わる場合、そのBehaviorの下にあり、このIntentが記載していないScenarioの`use:`を、置換後の`procedures`に対して検査する。解決しなければ`invalid_procedure_reference`で拒否し、何も書かない。検査の境界は次のとおり。

- Intentが記載したScenarioは、これまでどおりpatch後の内容で検査する。同じIntentで`use:`をやめるScenarioを書けば、手順を外せる。
- このIntentが別のBehaviorへ移すScenarioは、移動先のBehaviorで検査されるため、移動元の検査から外す。Intentの中での記載順に依存しない。
- `procedures`を省略したBehaviorと、`procedures`の内容が変わらないBehaviorは、検査しない。手順が変わらないので、新たに壊れる参照がない。

### 3. `use:`の自動書き換えは作らない

手順の改名は、旧名を外して新名を足す置換と区別しない。参照するScenarioの`use:`を追従して書き換える機能は作らない。拒否された場合、利用者は手順を残すか、同じIntentにScenarioの更新を記載する。要求されていない機能であり、必要になった時点でその要求に合わせて足す(YAGNI)。

### 4. 判定は1つの関数にまとめる

`use:`が解決するかどうかの判定は、`knowledge`モジュールの1つの関数(`unresolved_procedure_uses`)にまとめ、`validate`と`reconcile`の両方が同じ規則を使う。`generate`の展開時のエラーは、そのまま残す。

## 影響

- 手順を外す、または別名にするIntentは、その手順を使うScenarioが残っていると拒否される。既存のプロジェクトで、すでに壊れた参照を持つScenarioは、`validate`が新たに報告する。
- 拒否のdiagnosticには、Intent上の位置がないScenarioを指すため、Scenarioファイルのパスと、Scenarioのid・手順名・位置をメッセージに含める。

## 検討したが採用しない選択肢

- **`reconcile`は今のままとし、`validate`だけで検出する**: 壊れた状態が保存されてから気づくことになる。`reconcile`のfail-closedな原子的保存([0027](./0027-declarative-knowledge-reconciliation.md))の方針にも合わない。
- **改名時に、参照するScenarioの`use:`を自動で書き換える**: 手順は名前をキーとするvalueで、改名を表す操作が存在しない。追従のための機構を、要求がないまま足すことになる(YAGNI)。
- **`procedures`を明示したBehaviorを、内容が変わらなくても常に検査する**: 手順が変わらないなら新たに壊れる参照はなく、既存の壊れた参照を、無関係な更新の拒否理由にしてしまう。
