# 0023: Requirementの正本をnative/externalの二モードにする

## ステータス

Accepted(2026-09-11決定、2026-09-12実装完了。`checklist-v2-core.md`参照)。[0017](0017-scenario-case-revision-and-execution-evidence.md)§1・§3が定めるnative Requirement(Feature⇄Requirementの多対多関連、Feature側が関係の正本)を維持したうえで、正本を外部仕様書に置くモードを追加する。

## 背景

[markharness-v2-design.md](../design/markharness-v2-design.md)の再検討(2026-09-11)では、仕様(Requirement)の正本をStrictDoc(`.sdoc`、Git管理)に置き、markharnessは固定参照(`source_locator`/`source_revision`)だけを保持する方針を確定した。しかし、この方針を`requirement.yml`の唯一の形として実装すると次の問題が生じる。

1. **StrictDocを導入していない運用でmarkharnessが使えなくなる。** North Starの4問(影響確認・修正漏れ検知・過去の検証スコープ確認・リリース影響判断)は、いずれも外部仕様書の存在を前提としていない。
2. **MVPの期間中、固定参照が実質的に機能しない。** `.sdoc`の取込・解析は同設計書のM3(将来)であり、M0〜M2では外部要件の実体を読まない。参照先が存在しないまま`source_locator`を必須にすると、「誰も読まないファイルのblob OIDを固定するだけ」の状態になる。
3. **現行実装と現在の利用形態がnative前提である。** `knowledge/requirements/<id>/requirement.yml`は`label`/`description`/`axis`とUIDを持つnative実体であり(`src/knowledge.rs`)、本リポジトリ自身を含め、StrictDocを併用していない利用が存在する。

## 決定

### 1. `requirement.yml`に`source`を持たせる

`source: native | external`を追加し、省略時は`native`とみなす。

### 2. nativeモード

`label`(必須)・`description`(任意)をmarkharnessが正本として保持する。`source_locator`/`source_revision`は書けない。仕様側の変更検知は`requirement.yml`自体のbase/head差分で行う(粒度はRequirement単位であり、外部ツールを必要としない)。

### 3. externalモード

`source_locator`(markharnessと同一Gitリポジトリ内の`.sdoc`パス)と`source_revision`(固定したGit blob OID)が必須。`label`/`description`は持てない(外部正本の複製禁止、[markharness-v2-design.md](../design/markharness-v2-design.md)のP1)。仕様側の変更検知は`source_locator`が指す`.sdoc` blobのbase/head差分で行う。固定参照とheadのblob OIDの不一致はstale pinとして別に算出する。repinは仕様変更を打ち消さず、対応確認の代替にもならない(同設計書§6.1)。2026-09-11のレビュー修正により、従来の固定参照対headを変更検知に用いる規則を本規則へ訂正する。

### 4. 混在は拒否する

両モードのフィールドを併せ持つ、あるいはどちらのモードとしても不完全な`requirement.yml`は`validate`で拒否する。

### 5. `axis`は両モードで保持する

`axis`はmarkharness自身の分類であり、外部正本の複製ではないため、externalモードでも保持する。

## 影響範囲

- 既存の`requirement.yml`は`source`省略=nativeとしてそのまま有効であり、変換は不要。
- `markharness requirement repin`(固定参照の更新)はexternalモードのみに適用される。
- Change Impact/Release Coverageの出力形はモードによらない。Requirementごとに検知方式が変わるだけである。
- 対話作成フロー(`src/interactive.rs`・`knowledge_draft.rs`)は、nativeでは現行のまま、externalを選んだ場合のみ`source_locator`の入力へ切り替える。

## 検討したが採用しない選択肢

- **externalのみ(正本を常にStrictDocへ固定する)**：P1には最も忠実だが、StrictDoc未導入のチームがmarkharnessを使えず、`.sdoc`解析がM3である以上MVP自体が成立しない。
- **nativeのみ(externalは需要が出てから)**：仕様の正本をStrictDocに置く運用方針は既に確定しており、対応確認([0019](0019-alignment-check-commit-trailer.md))の判定対象を定義するには外部参照のデータ形を先に固定する必要がある。二モードの実装差は`validate`の分岐と変更検知の分岐に限られ、汎用化のための抽象化ではない。
- **nativeの`label`を残したままexternal参照も持たせる(両立)**：外部正本の複製が発生し、どちらが正しいかを判定する規則が新たに必要になる。モードを排他にすることでその規則自体を不要にする。
