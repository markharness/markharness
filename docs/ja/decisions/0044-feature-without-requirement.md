# 0044: RequirementのないFeatureを正当な状態にする

## ステータス

Accepted(2026-10-07決定)。[0027](./0027-declarative-knowledge-reconciliation.md)§5の「各フィールドの必須・非空規則を満たす」のうち、`contributes_to`に非空規則を課す読み方を否定する。

## 背景

`knowledge reconcile`は、`contributes_to`を省略した新規Featureと、`contributes_to: []`を指定したFeatureを受理し、`requirement_uids`が空の`feature.yml`を書く。`knowledge remove`も、最後のRequirementを削除すると、参照していたFeatureの`requirement_uids`を空にして残す([0034](./0034-knowledge-remove-command.md)§3)。ところが`feature.schema.json`は`requirement_uids`に`minItems: 1`を課していたため、これらの経路が書いた直後の`markharness validate`が`[] has less than 1 item at /requirement_uids`で失敗した(Issue #84)。

`minItems: 1`は、Featureが必須の単一の`requirement`を持っていた旧形式から、配列への変更時に機械的に引き継がれたもので、ADRで決めた規則ではない。[論文§3.1](../テスト知識管理のGit-nativeモデル_統合版.md)のER図は、FeatureとRequirementの関連を`}o--o{`(両側ともゼロ以上)で示す。同じ図は、必須の関連を`||`で区別している。[0034](./0034-knowledge-remove-command.md)も`requirement_uids`を「任意の多対多参照」と呼ぶ。一方、0027 §5は、`contributes_to`を含むvalue collectionが「各フィールドの必須・非空・参照規則を満たさなければならない」と読める書き方をしていた。

## 決定

### 1. Featureの`requirement_uids`は0件以上とする

`feature.schema.json`から`minItems: 1`を外す。RequirementのないFeatureは正当な状態であり、`knowledge reconcile`(省略と`[]`の両方)、`knowledge remove`、`markharness validate`は、この状態について同じ規則に従う。

### 2. RequirementのないFeatureを報告する機能は作らない

`traceability`・`coverage`・`validate`は、この状態を警告やgapとして報告しない。必要になった時点で、その時点の要求に合わせて足す(YAGNI)。

### 3. 新規に作成するプロジェクトを前提とし、`[knowledge].schema_version`は上げない

この変更は、旧schemaで有効だったデータを、新schemaでも有効なままにする。[0014](./0014-knowledge-schema-version-persistence.md)がプロトタイプ期に求める、旧形式との誤比較のリスクはない。

### 4. 0027 §5を実際の規則に合わせて書き換える

`phases`・`steps`・`results`は1件以上が必要で、`axis`・`contributes_to`・`procedures`には非空規則がないことを、0027 §5に明記する。

## 影響

- RequirementのないFeatureは、Requirementの変更から辿る影響の探索には現れない。Featureの変更から辿る探索では到達する(設計書§6.1手順1)。
- `feature.schema.json`の変更により、新規に`markharness init`したプロジェクトの`validate`は、RequirementのないFeatureを受理する。

## 検討したが採用しない選択肢

- **Featureに1件以上のRequirementを必須とする**: reconcileで省略と`[]`を拒否し、`knowledge remove`で最後のRequirementの削除を拒否するか、参照するFeatureを連鎖削除する必要がある。Requirementを後から付ける作成途中の状態が表現できなくなり、論文のER図の`}o--o{`とも合わない。
- **RequirementのないFeatureをgapとして報告する**: 現時点で要求がなく、必要な報告の形も決まっていない(YAGNI)。
