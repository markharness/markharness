# 0018: `[identity].schema_version`を1に凍結し、ADR 0013の不要な引き上げを訂正する

## ステータス

Accepted(実装済み)。ADR 0013「移行」節が`.markharness/config.toml`の`[identity]`markerへ書くと定めた`schema_version = 2`を、本ADRが`schema_version = 1`へ訂正する。ADR 0013の他の決定内容(UID発行、identity event、migration手順等)は本ADRの対象外であり、そのまま有効。

## 背景

ADR 0013「移行」節は、UID mode公開cutover完了時に`.markharness/config.toml`へ次を書き込むと定めていた。

```toml
[identity]
schema_version = 2
mode = "uid"
```

しかし実装([src/identity/marker.rs](../../../src/identity/marker.rs))を確認すると、cutover済みかどうかの判定(`is_uid_mode`)は`mode`フィールドのみを読み、`schema_version`は書き込まれるだけでどのコードからも読み取られていない。`changes compute`が`[knowledge].schema_version`を実際にref間比較へ使う([0014](./0014-knowledge-schema-version-persistence.md))のとは異なり、`[identity].schema_version`には対応する比較・互換性ゲートの実装も計画もない。

[0014](./0014-knowledge-schema-version-persistence.md)は、プロトタイプ期に実データも比較用途もないまま`[knowledge].schema_version`を刻むことを、ADR履歴の意味を薄めるとして退け、スキーマが安定するまで1に凍結する運用を定めた。ADR 0013が`[identity].schema_version`を1から2へ引き上げたのは、まさに[0014](./0014-knowledge-schema-version-persistence.md)が退けたのと同じパターン——読み取り側の実需要がないまま値だけを進める変更——であり、YAGNI([CLAUDE.md](../../../CLAUDE.md)所定の原則)に反する不要な実装だったと判断する。

## 決定内容

### 1. `[identity].schema_version`を1に凍結する

`IDENTITY_SCHEMA_VERSION`定数([src/identity/marker.rs](../../../src/identity/marker.rs))を1とし、UID mode cutover完了時も`schema_version`は1のまま書き込む。フィールド自体は`config.toml`内の他の`schema_version`系フィールド(トップレベルmarker、`[knowledge].schema_version`)との構造的対称性のために保持するが、値は比較・互換性判定の実装が実際に必要になるまで動かさない。

```toml
[identity]
schema_version = 1
mode = "uid"
```

UID mode cutoverが完了したかどうかの判定は、[0013](./0013-immutable-identity-model.md)が定めるとおり引き続き`mode = "uid"`の有無だけで行う。`schema_version`はこの判定に関与しない。

### 2. 将来`[identity].schema_version`を上げる条件

実際にある`schema_version`の値を読んで分岐する比較・互換性ゲート(`[knowledge].schema_version`に対する`changes compute`の`ensure_compatible`に相当するもの)が新たに実装され、かつその時点でスキーマ形状に破壊的変更が入る場合に限り、`[identity].schema_version`を引き上げる。実装予定のないまま将来の破壊的変更に備えて値を進めることはしない。

### 3. `mode`と`schema_version`の役割は分離したまま維持する

`mode`はcutover完了の唯一の権威あるフラグであり続ける。`schema_version`はcutoverの発生有無ではなく、UID mode内でのidentity dataの構造版を表すためだけの値であり、両者を混同する実装(例:`schema_version`の値でcutover判定を行う)は導入しない。

## この訂正が影響する範囲

- `src/identity/marker.rs`: `IDENTITY_SCHEMA_VERSION`定数とテスト。
- `tests/identity_cutover.rs`: doc commentの「schema version 2 public cutover」という表現。
- `docs/ja/cli-manual.md`・`docs/en/cli-manual.md`: cutover時に書き込む値の記述。
- `docs/ja/design/immutable-identity-model-design.md`・英語版: Phase 5の記述。
- `README.ja.md`: cutover後の検証規則の記述。
- 過去に`schema_version = 2`で移行済みのprojectとの後方互換性は考慮しない(本プロジェクトは0.xのプロトタイプ段階であり、実データを持つ既存projectは存在しない前提。[0014](./0014-knowledge-schema-version-persistence.md)背景と同じ判断根拠)。

## 0013との関係

ADR 0013の「移行」節に記載された`schema_version = 2`という具体的な値の記述のみを本ADRが訂正する。ADR 0013の本文自体は歴史的決定記録として書き換えず、そのステータス欄に本ADRへの参照を追記するにとどめる([release-and-license instructions](../../../.github/instructions/release-and-license.instructions.md)のADR運用方針に従い、ファイル移動や内容の書き換えは行わない)。
