# 0021: 退役・復元・ID予約解除の厳密な保証を廃止し、退役を単純化する

## ステータス

Accepted(2026-09-12実装完了。`checklist-v2-core.md`参照)。[markharness-v2-design.md](../design/markharness-v2-design.md)の再設計grillingセッション(2026-09-11)に基づく。[0013](0013-immutable-identity-model.md)のうち、retire・restore・release・reissue(同一UIDでの明示的な復元、旧idの予約解除)に関する部分を置き換える。UID発行、UIDとidの分離、rename時のUID維持など、それ以外の決定内容はそのまま有効。将来完全なlifecycleを再導入する場合のcutover原則は[0025](0025-v2-forward-compatible-evolution.md)で補足する。

## 背景

[0013](0013-immutable-identity-model.md)は、`.markharness/identity-events/`へのappend-only eventを唯一の正準情報源とし、retire・restore・release・reissueを含む厳密なidentity lifecycleを実装した(`src/identity/`配下、約9,000行)。これにより、退役した要素の同一UIDでの復元、退役後の人間向けIDの別UIDへの再割り当て(release)、それらの整合性監査(IdentityAuditor)が可能になっている。

grillingセッションで「既存の設計や概念に引っ張られない」前提でNorth Starから再検討した結果、次の判断に至った。

- 今回の要件(影響確認・修正漏れ検知・過去実行スコープ確認・リリース影響判断)は、いずれも「現在存在するFeature/TestCaseとその変更」を扱うものであり、退役後の同一性を厳密に復元できる必要があるという具体的なシナリオが挙がらなかった。
- 「退役したら別物として扱う」という単純な規則でも、North Starの4つの答えはすべて成立する。
- 実装済みの厳密な保証機構を維持するコスト(約9,000行のコード、event replay、IdentityAuditorの監査ロジック)に見合う具体的な必要性が、現時点の要件からは導出できない。

これは[CLAUDE.md](../../../CLAUDE.md)のYAGNI原則("いつか必要になるかもしれない"は実装理由にならない)に照らした判断である。

## 決定

### 1. 退役は「削除」として扱う

FeatureまたはTestCaseが`knowledge/`から削除された場合、それは単純にKnowledge treeから消えた要素として扱う。UIDの再利用禁止や、退役後の特別な状態遷移(`retired`)は管理しない。

### 2. 同一UIDでの復元は保証しない

削除した要素と同じ内容のFeature/TestCaseが再度追加された場合、それは新規の別要素として扱う。過去のUIDを引き継ぐ明示的な`restore` operationは提供しない。過去の対応関係(`feature.requirement_uids`等)を新しい要素で必要とする場合は、人が新しい要素に対して設定し直す。削除前のUIDを引き継ぐ判定ロジックは持たない(内容一致による自動復元は、本節冒頭の「新規の別要素として扱う」と矛盾するため導入しない)。

本決定が保証するのは**新規作成操作の側**である。すなわち、CLIで新しい要素を作れば新しいUIDが発行され、内容が過去の要素と一致することを理由に旧UIDを推定することはない。一方、削除したKnowledgeファイルをGit履歴からそのまま復元した場合、ファイル内の`uid:`が戻るため当時のUID(およびScenario UIDから決定的に導出されるCase UID、[0017](0017-scenario-case-revision-and-execution-evidence.md)§3)が復活する。これはmarkharnessの機能としての復元ではなくGit履歴操作であり、本ADRはこれを禁止も検出もしない。「同じ内容を再度追加すれば必ず別UIDになる」という、より強い主張はしない。rename(削除を伴わないid変更)時のUID維持は[0013](0013-immutable-identity-model.md)のまま有効であり、本ADRの対象外である。

### 3. `release` event・ID予約解除の仕組みは廃止する

退役した要素が使っていた人間向けID(`id:`)を別要素へ再割り当てする際の、明示的な`release`操作・予約解除記録は不要とする。ID重複時の挙動([0013](0013-immutable-identity-model.md)が定めるUID発行規則)は維持するが、退役済みIDの再利用可否を判定する専用ロジックは持たない。

### 4. `.markharness/identity-events/`のappend-only eventモデルは、UID発行・renameの範囲に縮小する

Identity lifecycleとしてeventで記録するのは、UIDの新規発行とrename(id変更時のUID維持)に限る。retire・restore・release・reissueのevent種別、およびそれらをreplayする`IdentityAuditor`の該当ロジックは実装しない。

## 影響範囲

- `src/identity/recovery.rs`(742行)・`src/identity/audit.rs`(816行)のうち、retire/restore/release/reissueに関する部分は、本ADRの方針に沿って実装時に縮小対象となる(実装作業は別途チェックリスト化する)。UID発行・rename・migrationに関する部分は維持する。
- [0013](0013-immutable-identity-model.md)のステータス欄に本ADRへの参照を追記する。ADR本文自体は歴史的決定記録として書き換えない([release-and-license instructions](../../../.github/instructions/release-and-license.instructions.md)のADR運用方針に従う)。

## 検討したが採用しない選択肢

- **既存の厳密な保証機構をそのまま維持する**：実装済みで動作している資産を捨てないという意味では手堅いが、今回のNorth Starに照らして必要性の説明ができず、コードの複雑さと保守コストだけが残る。実際に復元・ID再利用のニーズが具体的に生じた時点で、そのニーズの形を見てから再設計する方が適切([0004](0004-feature-id-change-migration.md)が同種の判断で採った方針と同じ考え方)。
- **retire/restoreだけ維持し、release/reissueだけ廃止する**：中間案として検討したが、「退役後の同一性」というシナリオ自体が今回挙がらなかった以上、一部だけ残す積極的な理由がない。まとめて単純化し、必要になった機能単位で個別に再導入する方が判断が明快。
