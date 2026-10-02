# 0039: 0.xの間は読み取り出力JSONの`schema_version`を上げない

## ステータス

Accepted(2026-10-02決定)。`schema_version`を上げる基準そのものは、1.0の基準を満たす時点で別のADRで決める(末尾「再検討の条件」)。

## 背景

`traceability`・`impact`・`coverage`の出力JSONは、`schema_version`と`record_kind`を持つ。外部ツールはこの値で、受け取ったJSONを読めるかを判断できる。

`traceability`の`behaviors[]`は、Scenarioを持たないBehaviorを出していなかった(設計書 cli-read-model-design.md §5.3)。`knowledge reconcile`はScenarioを持たないBehaviorを作れるため、外部ツールがそのBehaviorにScenarioを追加しようとしても、`behavior_uid`を引けなかった。これを直すと`behaviors[]`の要素が増える。既存のフィールドは変わらないが、「`schema_version`を上げるべきか」を決める必要が出た。

`schema_version`を上げる基準は、まだ決まっていない。

## 決定

**0.xの間は、読み取り出力JSON(`traceability`・`impact`・`coverage`)の`schema_version`を上げない。** フィールドの追加・削除、要素の増減、意味の訂正のいずれでも上げない。

今回の`behaviors[]`の修正(Knowledgeに存在する全Behaviorを出す)は、この決定を適用する最初の例であり、`traceability`の`schema_version`は1のままとする。

## 理由

- [release-and-license instructions](../../../.github/instructions/release-and-license.instructions.md)は、0.xのマイナーバージョン間で互換性を破ってよいと定めている。互換を保証しない期間に版番号を刻んでも、外部ツールが頼れる契約にならない。
- Knowledgeのスキーマ版も、同じ理由でプロトタイプ期は上げない([0014](./0014-knowledge-schema-version-persistence.md) §11)。出力JSONだけ別の扱いにすると、「どの版番号が何を保証するのか」が混乱する。
- 基準を決めずに版を上げると、次の変更で上げるかどうかを毎回その場で決めることになり、版番号の意味が一貫しない。

## 検討した代替案

- **今回の変更で`traceability`を1から2へ上げる。** 版を見て表示を止める外部ツールが、要素が増えただけの変更で止まる。基準も無いまま上げた番号は、次の変更の先例として使えない。採用しない。
- **「既存フィールドの意味が変わるときだけ上げる」という基準を今決める。** 互換契約を必要とする外部ツールの実データがまだ無く、基準が正しいかを検証できない。必要になった時点で、その時点の要求に合わせて決める(YAGNI)。採用しない。

## 帰結

- 外部ツールは、0.xの間は読み取り出力が変わりうることを前提にする。`schema_version`の一致は、出力が変わっていないことを保証しない。
- 出力を変えるときは、設計書とCLIマニュアルを同じ変更で更新する。

## 再検討の条件

[PROJECT.md](../../../PROJECT.md)が定める1.0の基準を満たす時点で、`schema_version`を上げる基準を決める。検討の出発点として、「既存のフィールドの意味・型が変わる、または既存の読み方が誤りになるときに上げ、要素の追加は互換とする」案がある。これは未決であり、本ADRでは採用しない。
