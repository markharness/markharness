# 0030: external Requirementに外部key(`source_key`)を追加する

## ステータス

Accepted(2026-09-15決定)。[0023](0023-requirement-native-and-external-source.md)が定めたexternal Requirement(`source_locator`/`source_revision`)を拡張し、StrictDoc側のUID文字列を保持する第三の固定フィールド`source_key`を追加する。

## 背景

StrictDoc側のUID(`.sdoc`内の要件識別子)は自由記述であり、大文字小文字を含む任意の文字列を取り得る。実際にStrictDoc側のUIDをmarkharness側へ保持しようとしたところ、次の事故が発生した。

markharnessの`id`(表示ID)は`is_valid_slug`(`src/knowledge.rs`)によって小文字ASCII英数字とハイフンのみに制限されている。これはRequirement固有の制約ではなく、`id`がファイルシステム上のパス構成要素(`knowledge/requirements/<id>/requirement.yml`等)として使い回されるための、markharness全体で共通の安全側の制約である。StrictDoc側の大文字を含むUID(例: `REQ-Login-01`)をそのまま`id`へ書き込もうとした結果、この制約に弾かれ、やむなく小文字化して保存することになった。この時点でStrictDoc側の原表記とmarkharness側の保存値が乖離し、原表記のままの一致検索・目視照合ができなくなり、トレーサビリティを大きく損ねた。

[markharness-v2-design.md](../design/markharness-v2-design.md) §9.2.1は、将来のStrictDoc Adapter(M3)のための前方互換契約として「external Requirementは、外部key、同一Git内のlocator、固定revisionを区別する」とあらかじめ定めていたが、この「外部key」自体は未実装だった。本ADRはこの契約を先取りして実装し、StrictDoc UIDをmarkharnessのファイル名安全制約に従わせず、原表記のまま保持できるようにする。

## 決定

### 1. `Requirement`に`source_key`を追加する

`source_key: Option<String>`を`Requirement`(`src/knowledge.rs`)に追加する。`source_locator`/`source_revision`と同じ扱いとし、`source: external`では必須、`source: native`では禁止する。両方を持つ、あるいはどちらも欠く`requirement.yml`は`validate`で拒否する([0023](0023-requirement-native-and-external-source.md)§4と同じ排他ルール)。

現設計上`source: external`は常にStrictDoc(`.sdoc`)を指すモードであり、StrictDocを使わない場合は`source: native`を使う。したがって`source_key`をexternalの必須項目にしても、StrictDoc以外の外部仕様書を想定した拡張性を損なわない。

### 2. 生値をそのまま保存し、書き込み時の正規化は行わない

`source_key`にはStrictDoc側の識別子をそのまま保存する。大文字・小文字を含め、markharness側での自動変換・強制正規化は行わない。文字集合の制限も設けない(`id`の`is_valid_slug`制約は適用しない)。原表記を保持することが、StrictDoc側との目視照合・grep等の手動一致確認を可能にする前提だからである。

**保存する値として推奨するのはStrictDocのMID(Model ID)であり、事故の原因になった自由記述の`UID:`フィールドではない。** MIDはStrictDocが各ノードに自動生成する、常に小文字16進文字列の機械生成識別子であり(`ENABLE_MID: True`、または個々の要件への明示的な`MID:`行で`.sdoc`に書き出される)、人手による表記ゆれが構造的に発生しない。`source_key`フィールド自体はどのStrictDoc識別子を保存するかを型として強制しない(汎用の生値保持フィールドのまま)が、運用上はMIDの利用を推奨する。

### 3. 比較(重複検出・検索)は本ADRのスコープに含めない

同じ`source_key`(大文字正規化して比較した場合に一致する)を持つ複数のRequirementが存在しても、`validate`はこれを検出・拒否しない。markharness側に`source_key`による検索・lookupコマンドも追加しない。将来この種の比較が必要になった場合は、大文字正規化して比較する方針だけを本ADRで定め、実装は需要が具体的に確認された時点で別ADRとして追加する。

### 4. `source_key`はRequirementの同一性に一切関与しない

本ADRは[0013](0013-immutable-identity-model.md)が定めるRequirementの`uid`運用に一切影響しない。`source_key`は表示・照合用の付随情報であり、同一性の判定・rename耐性のロジック([0021](0021-identity-retire-simplification.md))には一切使わない。新規フィールドは既存の`Requirement` entityへの追加であり、新しい`EntityKind`は導入しない。

### 5. `schema_version`は変更しない

[markharness-v2-design.md](../design/markharness-v2-design.md) §9.2.2の既存方針([0018](0018-identity-schema-version-freeze.md)と同じ考え方)により、フィールド追加のみを理由に`schema_version`を進めない。プロトタイプ期であり、実際に比較・互換性ゲートを実装する必要が生じるまで値は`1`のまま固定する。

## 影響範囲

- `src/knowledge.rs`の`Requirement`構造体・YAMLシリアライズ、`src/validate.rs`の`check_requirement_source_mode`、`src/knowledge_reconcile/`(Intentスキーマ・plan構築・blank文字列チェック)、`schema/requirement.schema.json`に`source_key`を追加する。
- 既存の`requirement.yml`(現状このリポジトリには`source: external`の実体は存在しない)には影響しない。

## 検討したが採用しない選択肢

- **`id`(表示ID)をStrictDoc UIDとして流用する**: 実際にこの事故の直接の原因になった案。`id`はRequirement固有の値ではなく、markharness全体でファイル名など他の用途にも安全に使い回される前提の値であり、この制約(`is_valid_slug`)を緩めることは`id`を使う他のentity・他の用途すべての安全性を損なう。したがって`id`とは別の、StrictDoc UID専用のフィールドを設けた。
- **保存時に大文字へ強制変換する**: StrictDoc側の原表記との差異が生じ、目視照合・grepでの一致確認がかえって難しくなる。比較が必要な場面でのみ正規化すれば十分であり、保存値そのものを変える理由がない。
- **重複検出・検索コマンドの同時実装**: 今回のスコープはStrictDoc UIDを原表記のまま保持できるようにすることに限る。重複検出・検索は別の機能であり、需要を確認してから別ADRで検討する。
- **自由記述の`UID:`フィールドを推奨値のままにする**: 当初の事故対応としては「原表記のまま保持できればよい」で十分だったが、`UID:`フィールドは依然として人手で書かれる自由記述であり、将来同じ表記ゆれが再発する可能性を残す。MIDは機械生成のため表記ゆれが構造的に発生せず、より安全側に倒せる選択肢が判明したため、推奨値をMIDへ変更した。

## 将来再検討するトリガー

次のいずれかが成立した時点で、比較(重複検出・検索)を実装しない本決定の範囲を再検討する。

- 表記ゆれによる実際の重複登録・見落としが再度報告された場合。
- 検索コマンドの需要が具体的に生じた場合。
- M3でStrictDoc Adapterを実装し、Requirement単位のkey比較が必要になった場合。
