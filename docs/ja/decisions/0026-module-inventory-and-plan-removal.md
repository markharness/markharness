# 0026: 既存モジュールの棚卸し結果と`plan`の廃止、および後方互換を考えない方針

## ステータス

Accepted(2026-09-11決定、2026-09-12実装完了。`checklist-v2-core.md`参照)。[markharness-v2-design.md](../design/markharness-v2-design.md)§9が「棚卸し対象」「要判断」として保留していたモジュールの帰結を確定する。あわせて[0019](0019-alignment-check-commit-trailer.md)〜[0025](0025-v2-forward-compatible-evolution.md)の実装にあたり、後方互換を一切考えないことを明文化する。

## 背景

[markharness-v2-design.md](../design/markharness-v2-design.md)§9は、`src/verify.rs`・`src/audit_scope.rs`・`src/derived_index.rs`・`src/lineage.rs`を「棚卸し対象(本書では未分類)」、`src/milestone.rs`・`src/backfill.rs`を「要判断」、`src/canonical.rs`を「見直し」として、M0着手時に判定することとしていた。

2026-09-11の実装前grillingセッションで、各モジュールの実際の呼び出し関係を計測した。その結果、設計書および[0021](0021-identity-retire-simplification.md)の前提の一部が事実と異なること、および削除の可否がモジュールごとに大きく異なることが判明した。

## 決定

### 1. `plan`を廃止する

`src/plan.rs`、`markharness plan`サブコマンド、`src/application.rs`・`src/presentation.rs`のplan経路(`CommandOutcome::PlanBuilt`、`plan_exit_code`、`build_verification_plan_value`等)、`tests/plan_domain.rs`・`tests/plan_cli.rs`を削除する。

[0020](0020-execution-status-lightweight-model.md)がEvidence適用可能性の厳密な突合を不要とした結果、`plan`に残る機能は「TestCaseに`ExecutionBinding`があるか」を返すことだけになる。これは[markharness-v2-design.md](../design/markharness-v2-design.md)§6.2の`coverage`が返す情報の部分集合であり、二つのサブコマンドで同じ問いに答える状態になる。縮小して残すより廃止するほうが小さい。

### 2. `src/canonical.rs`は`import`専用に縮小する

`markharness import`(native/JUnit)は維持する。[0020](0020-execution-status-lightweight-model.md)が否定したのは証跡の保存・管理であり、外部テスト結果の取込経路そのものではない。`CanonicalEvidence`・`EvidenceResult`・`RelationOriginKind`のうち`plan`専用の型は§1の削除に伴って削除する。

StrictDoc取込を将来追加する場合は、[markharness-v2-design.md](../design/markharness-v2-design.md)§9のとおり別Adapterとして設計する。本ADRはその設計を先取りしない。

### 3. `src/derived_index.rs`と`markharness cache index`を廃止する

`derived_index.rs`は`plan::BoundVersions`と`execution::read_all_results`を入力とする。§1で`plan.rs`が削除され、`execution.rs`が[0025](0025-v2-forward-compatible-evolution.md)の`ExecutionBinding`へ置換されるため、入力の両方が失われる。`.markharness-cache/index/`の派生インデックスはChange Impact・Release Coverageの算出経路に登場せず、CLI統合テストも存在しない。

### 4. `src/lineage.rs`・`src/milestone.rs`は維持する

両モジュールは`src/changes.rs`から内部利用されている。`changes.rs`は`lineage::classify`でmerge commitの親関係を分類し、`milestone::verify_audit_matches_tag`をfail-closedゲートとして呼ぶ。`changes.rs`は[markharness-v2-design.md](../design/markharness-v2-design.md)§6.1でChange Impactの基盤として維持されるため、両モジュールを削除すると`changes compute`と`backfill run`がコンパイル不能になる。

CLIサブコマンド`changes lineage`・`milestone init`も維持する。`milestone init`が作る`.markharness/executions/<tag>/milestone.yml`は[0020](0020-execution-status-lightweight-model.md)が廃止する実行記録とは別物(schema versionの監査コピー)であり、`changes compute`のゲートの入力である。CLIを削除するとこの入力を人が用意できなくなる。

### 5. `src/verify.rs`・`src/backfill.rs`・`src/audit_scope.rs`は維持する

`verify.rs`(生成物とコミット済み生成物の差分検査)と`backfill.rs`(過去milestone間のChangeEvent一括計算)は、逆向きの依存を持たず単独で削除できる。しかし[0019](0019-alignment-check-commit-trailer.md)〜[0025](0025-v2-forward-compatible-evolution.md)のいずれとも衝突せず、削除する積極的な理由が無い。[CLAUDE.md](../../../CLAUDE.md)のYAGNI原則は「要求されていない実装を足さない」ことを求めるものであり、既に動作し衝突しないコードを削除する根拠にはならない。

`audit_scope.rs`(62行)は`identity migrate --json`・`identity audit --json`・`changes compute --json`の出力に含まれる`audit_scope`フィールドの型であり、[0013](0013-immutable-identity-model.md)の検証規則が定める出力契約の一部である。

### 6. `execution::iso8601_utc_now`を`src/time.rs`へ移設する

この関数は`src/identity/feature_ops.rs`と`src/identity/migration_manifest.rs`から利用されている。[0025](0025-v2-forward-compatible-evolution.md)§1により`ExecutionBinding`は実行日時を持たないため、時刻生成関数が`execution`モジュールに残ると「`ExecutionBinding`は日時を持たないのに`execution`が時刻関数を公開している」という、読み手を誤らせる構造になる。identity eventは日時を持ち続けるため関数自体は必要であり、責務に対応する位置へ移す。

### 7. 後方互換を一切考えない

[0019](0019-alignment-check-commit-trailer.md)〜[0025](0025-v2-forward-compatible-evolution.md)の実装において、**過去のスキーマ・データは最初から存在しなかったものとして扱う**。互換コード、移行コード、旧データを名指しする診断、スキーマ版の引き上げは実装しない。これは[CLAUDE.md](../../../CLAUDE.md)の設計ルール(「後方互換性を想定せず互換のための設計は排除し、常に最善のプロダクトを目指す」)の適用であり、[markharness-v2-design.md](../design/markharness-v2-design.md)旧§9.1が定めていた「廃止したevent種別を含むログを診断付きで拒否する」規定を置き換える。

帰結は次の通り。

- `ExecutionBinding`は新しい保存先`.markharness/bindings/`のみを読み、旧`.markharness/executions/`の実行記録は参照しない。
- 廃止したevent種別は`IdentityMutation`から削除されるため、それを含むログには読み取り経路が存在しない。拒否用の診断コードも書かない。
- `requirement.yml`の`source`は**必須**とする([0023](0023-requirement-native-and-external-source.md)§1の「省略時はnative」は既存ファイルを無変更で通すための互換規定であったため適用しない)。モード判定を暗黙のdefaultに委ねない。
- `knowledge_schema_version`([0014](0014-knowledge-schema-version-persistence.md))は引き上げない。
- [0025](0025-v2-forward-compatible-evolution.md)§2の`schema_version`は残すが、全種別で`1`に固定し今後も上げない。これは過去を読むための互換機構ではなく、将来別種のレコードを追加したときに種類を取り違えないための前方向の契約だからである。

### 8. [0021](0021-identity-retire-simplification.md)影響範囲節の記述は実測と異なる

[0021](0021-identity-retire-simplification.md)「影響範囲」節は、retire/restore/release/reissueの縮小対象を`src/identity/recovery.rs`(742行)・`src/identity/audit.rs`(816行)としている。2026-09-11の実測ではこれは誤りであり、実際の対象は次の通りである。

- `src/identity/feature_ops.rs`の`ReleaseError`/`release_id`/`RetireError`/`retire_entity`/`RestoreError`/`restore_entity`/`ReissuedEntity`/`ReissueError`/`reissue_entity`(約524行)
- `src/identity/event.rs`の`IdentityMutation::{Retired, Restored, Released, Reissued}`
- `src/identity/engine.rs`のreplay時のstatus遷移と`Status::Retired`
- `src/identity/migration_manifest.rs`のreissue依存部
- `src/cli.rs`の`IdentityCommand::{Release, Retire, Restore, Reissue}`と`tests/identity_cli.rs`の該当テスト

`recovery.rs`と`lock.rs`に現れる`release`は`IdentityLock`のファイルロック解放であり、ID予約解除とは無関係である。`audit.rs`にはretire/restore/release/reissueの語が一度も出現しない。

[0021](0021-identity-retire-simplification.md)の**決定内容**(何を廃止するか)は正しく、誤っているのは影響範囲の見積もりのみであるため、決定の効力は変わらない。[release-and-license instructions](../../../.github/instructions/release-and-license.instructions.md)のADR運用方針に従い、[0021](0021-identity-retire-simplification.md)本文は歴史的決定記録として書き換えず、訂正を本ADRに記録する。[markharness-v2-design.md](../design/markharness-v2-design.md)§9は現在の計画を表す生きた文書であるため、そちらは訂正する。

## 影響範囲

- [markharness-v2-design.md](../design/markharness-v2-design.md)§9の表、§9.1、§9.2.2、および受け入れ条件AC09b・AC22を本ADRに合わせて更新する。AC22(廃止eventを含むログを診断付きで拒否)は§7により削除する。
- 廃止するCLIは`plan`・`cache index`が追加される(既存の`identity retire`/`restore`/`release`/`reissue`・`serve`・`execution record`に加えて)。
- CLIマニュアル(`docs/ja/cli-manual.md`・`docs/en/cli-manual.md`)の該当節を削除する。

## 検討したが採用しない選択肢

- **`plan`を`ExecutionBinding`参照へ書き換えて残す**：既存利用者の経路を保てるが、`coverage`と同じ問いに答えるサブコマンドが二つ並ぶ。後方互換を考えない方針(§7)の下では残す理由が無い。
- **`verify`・`backfill`も削除して表面積を最小化する**：削除自体は容易だが、どちらもv2の決定と衝突せず、動作している機能を判断の根拠なく削ることになる。必要性が否定された時点で個別に廃止する。
- **`lineage`・`milestone`の内部関数だけ残しCLIを廃止する**：`milestone init`が作る`milestone.yml`は`changes compute`のfail-closedゲートの入力であり、CLIを削ると人が入力を用意できなくなる。`changes lineage`も`changes compute`の分類結果を確認する経路で、維持コストは小さい。
- **旧データに対する診断だけは実装する**：利用者には親切だが、診断コードは互換コードの一種であり、[CLAUDE.md](../../../CLAUDE.md)の設計ルールが排除対象としている。新コードがどの経路でも旧データを読まない以上、動作上の危険も無い。
