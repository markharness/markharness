# 0024: リリースの選定スコープを軽量な選定リストとして記録する

## ステータス

Accepted(2026-09-11、設計合意済み・未実装)。[0020](0020-execution-status-lightweight-model.md)が定める「実行結果・証跡をmarkharnessの責務から外す」方針は維持したまま、「そのリリースで何を検証対象に選んだか」だけを記録する最小の型を追加する。[0025](0025-v2-forward-compatible-evolution.md)により、将来の`ReleasePlan`とは別のrecord kindとして維持し、完全な計画へ読み替えない。

## 背景

[markharness-v2-design.md](../design/markharness-v2-design.md)§1の問い3は「前回リリースにおいて、どのテストが検証スコープに入っていたかを確認する」ことである。

[0020](0020-execution-status-lightweight-model.md)の`ExecutionStatus`は検証手段(`automated`/`manual`)とその参照先だけを持ち、日時もリリース番号も持たない。そのためGit refを指定して過去時点を再現しても、答えられるのは「その時点でKnowledgeに登録されていたTestCaseと検証手段」までであり、「そのリリースで実際に検証対象として選んだ集合」ではない。設計レビュー(2026-09-11、SP-05)はこの点を指摘し、登録状態と選定を混同しないことを求めた。

一方、旧v2設計の`Verification Plan`/`EvidenceSelection`/`Execution Manifest`のような重量級の契約オブジェクトは[0020](0020-execution-status-lightweight-model.md)で明確に否定されている。必要なのは「選んだものの一覧」だけであり、選定の承認・履歴・結果ではない。

## 決定

### 1. `ReleaseScope`を追加する

```text
ReleaseScope {
  release_id,            // リリースの表示名(Git tag名を推奨)
  case_uids: [case_uid], // そのリリースで検証対象に選んだTestCase
}
```

TestCaseの参照は表示IDではなくCase UIDで行う([0013](0013-immutable-identity-model.md)、rename耐性)。

### 2. 持たないもの

選定日時、選定者、承認状態・ステータス遷移、合否、実行結果、対象ビルド、実行環境、選定理由の構造化フィールドは持たない。これらが必要な場合は別ツールの責務とする([0020](0020-execution-status-lightweight-model.md)と同じ線引き)。選定の経緯はGit履歴が記録する。

### 3. 保存場所

`.markharness/releases/<release_id>.yml`としてGit管理下に置く。これにより`--at <ref>`で過去時点の選定リストをそのまま再現でき、算出の再現性(設計書P3)が保たれる。

`release_id`はこのパスの単一の構成要素になるため、値を安全な範囲に制限する。ASCII小文字英数字・ハイフン・ドットのみを許し、空文字、`.`・`..`そのもの、先頭がドットの値、パス区切り(`/`・`\`)やドライブ指定を含む値は、ファイルを作る前に拒否する。これは現行`src/generate.rs`の`require_valid_slug`が、`id:`フィールドが`generated/testcases/`のディレクトリ構成要素になることを理由に同じ検証を課しているのと同じ扱いであり、`.markharness/`外への書き出しやディレクトリ横断を構造的に不可能にする。`v1.2.0`のような一般的なtag名は許可される。書き込みは`src/fs_safety.rs`の原子的置換経路を用いる。

### 4. 記録は人が行う

`markharness release scope set`で人が選定リストを記録・置換する。markharnessは選定内容の妥当性を判定せず、自動生成もしない。選定リストが無いリリースについては、Release Coverageは従来どおり登録状態の一覧だけを返す。

### 5. 選定リストは実行の証拠ではない

`ReleaseScope`に含まれることは「選んだ」という宣言に過ぎず、実行された事実でも合格した事実でもない。出力でも「選定済み」と「実行済み」を同一視しない。

## 影響範囲

- `markharness coverage --release <release-id>`で、選定されたTestCaseの検証手段の有無、選定漏れ候補(対象Requirement/Feature配下にあるが選定リストに無いTestCase)、その時点のKnowledgeに存在しないCase UIDを一覧する(設計書§6.2)。
- 設計書§1の問い3は、`ReleaseScope`が記録されているリリースについてのみ「何を選んだか」まで答えられる。記録の無いリリースでは登録状態の再現までである。
- ロードマップ上はM2(Release Coverage)に含める。

## 検討したが採用しない選択肢

- **Git tagのみで代用する**：tagは時点を指すだけで、その時点のKnowledge全件と選定集合を区別できない。全件が対象だったのか一部だったのかを後から復元できない。
- **`ExecutionStatus`にリリース識別子を持たせる**：TestCase単位のレコードがリリースごとに増殖し、[0020](0020-execution-status-lightweight-model.md)が避けた「実行ごとの記録」へ逆戻りする。選定はリリース単位の集合であり、リリース側に置くほうが小さい。
- **選定理由・承認者・承認状態を持たせる**：承認ワークフローの再導入であり、[0019](0019-alignment-check-commit-trailer.md)が独立した承認機構を作らないとした判断と矛盾する。必要な経緯はcommit履歴に残る。
