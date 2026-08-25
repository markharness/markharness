# 0014: KnowledgeスキーマバージョンをGit履歴へ永続化する

## ステータス

Accepted (2026-08-25)

## 背景

[`changes compute`](../cli-manual.md)は、2つのref間で各Featureディレクトリの Git tree SHA を比較することで変更を検出している(`src/changes.rs`)。Knowledgeスキーマの移行によって全ての`feature.yml`/`behavior.yml`等が機械的に書き換えられた場合、意味は変わっていなくても全FeatureのtreeSHAが変化するため、全FeatureにChangeEventが生成され、全ての生成テストケースが`impacted_testcases`に含まれてしまう。実際の内容変更と区別できない。

将来のcanonical model converter(本ADRの対象外、後述)が、任意のGit refについて「そのrefで有効だったKnowledgeスキーマバージョンは何か」を知る手段が必要になる。現状これを記録する仕組みは存在しない。`.markharness/knowledge/*.yml`にバージョンマーカーはなく、`.markharness/config.toml`唯一のフィールドはマーカーファイル形式自体の`schema_version`で、[0010](./0010-project-root-auto-detection.md)はこれを互換性チェックのために読み取ることを明示的に先送りしていた(「読み取り側の互換性チェックは実装しない(将来必要になった時点で追加する、YAGNI)」)。[Issue #29](https://github.com/markharness/markharness/issues/29)は、この必要性が具体化したものである。

[0013](./0013-immutable-identity-model.md)は既に本ADRが踏襲するパターンを確立していた: `config.toml`内の独立したテーブルに置かれた、狭いスコープを持つ2つ目の`schema_version`(`[identity].schema_version`)であり、マーカーファイル自体のトップレベル`schema_version`とは独立している。リポジトリには他にも複数の独立した`schema_version`相当の値が既に存在する — バージョン付きJSON出力envelope(`src/presentation.rs`の`"schema_version":1`)、id解決/identity registryキャッシュキーの`CANONICALIZATION_RULE_VERSION`/`ID_INDEX_SCHEMA_VERSION`(`src/id_cache.rs`、`src/identity/registry.rs`)。これらは互いに共有されない。本ADRはKnowledgeコンテンツ自体のためのスコープを持つ、もう一つのバージョンを追加する。

## 決定内容

### 1. `config.toml`専用の`[knowledge].schema_version`

```toml
schema_version = 1

[knowledge]
schema_version = 1
```

トップレベルのマーカーファイル`schema_version`、`[identity].schema_version`、JSON出力envelopeの`schema_version`、idキャッシュの`CANONICALIZATION_RULE_VERSION`/`ID_INDEX_SCHEMA_VERSION`とはスコープを分離し、いずれとも一緒に読み書きしない。

### 2. 比較対象refの`config.toml`を正本とする

`changes compute <from> <to>`は、`from`・`to`双方の`[knowledge].schema_version`をそれぞれのref自身がコミットしている`config.toml`から(`git ls-tree`+`git cat-file`経由で、ワーキングツリーではなく)解決する。実行中のCLIバージョンからは推定せず、`milestone.yml`からも読まない。これにより、マイルストーンタグ・任意コミット・将来のPR base/head比較のいずれにも同じ解決方法が一律に使える。

### 3. `milestone.yml`は監査用の複写を持つが、正本にはしない

`milestone init`は`commit_oid`と`knowledge_schema_version`を追加で解決・記録する。

```yaml
id: v2
commit_oid: 0123456789abcdef...
knowledge_schema_version: 1
```

これらはtagから解決した値の監査・表示用複写であり、`changes compute`はこれを参照しない。`milestone init`は本変更後も冪等なままであり、既存の`milestone.yml`は変更前と同様に不変とする。

### 4. `changes compute`と`backfill run`は比較前にバージョンを解決する

両者とも同じ`changes::compute_changes_with_warnings`を経由する(`application::compute_changes`と`backfill_run_with_policy`がそれぞれ直接呼ぶ。`ChangeAnalyzer::compute`/`compute_changes`/`compute_changes_between_refs`は、ChangeEventだけを必要とする呼び出し元向けの薄いラッパー)。そのためバージョンゲートは1箇所に実装するだけで全ての呼び出し元に自動的に適用される。

### 5. 未対応のバージョン組み合わせはfail closedにする

`from`と`to`が異なる既知バージョンを報告した場合、またはいずれかがこのCLIビルドが知らない未来のバージョンを報告した場合、`compute_changes_between_refs`は何も計算する前に`ErrorKind::Unsupported`を持つ`io::Error`を返す。ChangeEventは生成されず、呼び出し元(`application::compute_changes`)は`replace_file`による書き込みに到達しないため、既存の`changes/<to>.yaml`は変更されずに残る。これは新しいJSON専用エラーチャンネルを追加するのではなく、既存の`error: {err}` → stderr → exit 1 の経路(`src/main.rs`)をそのまま再利用する。

`backfill run`は同じ`Unsupported`エラーを、run全体を中断する理由ではなく、そのペアだけのスキップとして扱う。該当ペアは`BackfillReport.incompatible`に記録され、`git notes`には記録されない。そのため後続のrunは自動的に再試行し、converterが実装された時点で手動の再実行操作なしに成功するようになる。

### 6. バージョンが記録されていないrefはlegacyスキーマバージョン1とみなす

`.markharness/knowledge/`は本機能導入以前の全ての未変更refにおいてバージョン情報を持たない。そのため`config.toml`に`[knowledge]`テーブルがない(あるいは`config.toml`自体が存在しない)refは、legacyスキーマバージョン1とみなし、`changes compute`はこの推定を無言で行わずwarningとして表示する。このwarningは構造化フィールド(`CommandOutcome::ChangesComputed.warnings: Vec<String>`)であり、`HumanPresenter`(`warning: ...`行)と`JsonPresenter`(既存JSON envelope内の`"warnings"`配列。ただし何も警告がない場合は`[]`ではなくキー自体を省略する — §9参照)の両方でレンダリングされる。

### 7. `milestone.yml`の監査コピーは書き込むだけでなく検証もする

`changes compute`・`backfill run`はいずれも、単一の共有関数`compute_changes_with_warnings`を通じて各refのKnowledgeスキーマバージョンを解決する。この関数は各refのバージョンを解決した直後(§8参照。監査検証はその解決結果をそのまま再利用し、再解決はしない)、各ref自身の`.markharness/executions/<name>/milestone.yml`(その名前のものが存在する場合)を、その名前が指すはずのtagと突き合わせて検証する — 記録された`commit_oid`と`knowledge_schema_version`が、そのtagが今実際に解決する値と一致していなければならない(バージョン解決ポリシー表の「`milestone.yml` とtag内の正本が不一致 | エラーとして報告する」行。従来は未実装だった)。これらのフィールドを持たない`milestone.yml`(本ADR以前のもの)は検証対象外とし、表の次の行の通りtagの正本のみを信頼する。不一致(tagの移動、または手編集)はfail closedの`Unsupported`スキップではなく、ハードな`InvalidData`エラーとする。そのため`backfill run`は、未対応スキーマバージョンのペアのように黙ってスキップし後で再試行する、という扱いをしない — 古い・改ざんされた監査コピーはconverterではなく人間の確認を必要とするため。

### 8. バージョン解決は関心事ごとではなくref単位で一度だけ行う

`compute_changes_with_warnings`は各refのKnowledgeスキーマバージョンをちょうど一度だけ解決し、その同じ`ResolvedSchemaVersion`を3つの利用箇所すべて — §7の`milestone.yml`監査検証、fail-closedゲート、legacy warningのテキスト — で再利用する。それぞれが独立に再解決することはしない(Standardsレビューでの指摘: 最初は`application::compute_changes`と`changes::compute_changes`の間で、次に`milestone::verify_audit_matches_tag`とその呼び出し元の間で、同じ重複が2回検出された。Git読み取りの重複に加え、2回の解決結果が(例えばtag更新と競合した場合などに)食い違えば、3つのうち2つが一致しなくなるリスクが実在した)。そのため`verify_audit_matches_tag`は自ら解決するのではなく、呼び出し元が既に解決した`ResolvedSchemaVersion`を引数として受け取る。

### 9. `warnings`はoptionalなJSONフィールドとし、`backfill run`も`changes compute`と同じ情報を報告する

`changes compute --json`の出力では、報告すべき警告が何もない場合`"warnings"`キー自体を省略し、`"warnings":[]`としては出力しない — 設計ドキュメントのJSON contract規約(§5)は、同一`schema_version`内での追加を*optionalなフィールド*に限って許可しており、常に存在するフィールド(空配列であっても)は実質的にrequiredなフィールドであり、既存の全利用者に対してv1 contractの形を変えてしまう。

`backfill run`も、`changes compute`と同じlegacy schema versionのwarningを収集する(ペアごとに`compute_changes_with_warnings`を1回呼ぶ)。`backfill run`には現状`--json`モードがないため、`warning: ...`行として出力し、非互換としてスキップした各ペアの名前も列挙する — fail-closedゲートだけでなく`changes compute`と同じポリシーに従う。バージョン非互換なペアは、run自体は成功していても実際には未処理の作業を残すため、`BackfillReport.incompatible`が空でない場合`backfill run`は終了コード`0`ではなく`1`で終了する。見落とされかねない「クリーンな成功」を報告しないようにするためである。

### 10. スキップされたペアの診断情報は汎用メッセージに置き換えず保持する

`BackfillReport.incompatible`は名前だけでなく`IncompatiblePair { to_milestone, reason }`を保持する — `reason`はfail-closedゲート自身の`io::Error`メッセージそのもの(Specレビューでの指摘: 以前のバージョンは`Unsupported`エラーを捕捉した上で捨て、代わりに汎用的な「非互換」の1行だけを出力していた)。issue #29 §5は、fail-closedの診断が両側のバージョンを名指しし、CLI更新またはmigrationが必要であることを述べることを要求している。`backfill run`はスキップした各ペアについてこの同じメッセージを出力し、利用者が理由を知るために手動で`changes compute`を再実行する必要がないようにする。`compute_changes_with_warnings`は、該当するlegacy fallbackのwarningテキストも、その同じ`Unsupported`エラーのメッセージに折り込んでから返す(§6のwarningは`ComputeChangesOutcome`の`Ok`パスにしか存在しないため、そうしないとゲートに失敗したペアはそのコンテキストを完全に失ってしまう)。

### 対象外

issue #29が明示した対象外と一致する: 異なるスキーマ間のconverter、schema-only migrationを除外する意味的差分(semantic diff)、semantic hashおよびcanonicalization rule versionの変更、`--allow-raw-schema-diff`のようなescape hatch、既存Knowledgeを新スキーマへ書き換えるmigrationコマンド。本ADRはバージョンを解決可能にし、未対応の比較を無言で行わず安全に停止させることのみを行う。

## 対応内容

- `src/knowledge_schema.rs`(新規): `resolve`(ref → `ResolvedSchemaVersion { version, is_legacy }`。記録された値が不正(非整数、u32範囲外、`[knowledge]`自体がテーブルでない)な場合は「未記録」と黙って同一視せずハードエラーにする)、`ensure_compatible`(fail closedのゲート。相違・未来バージョンに加えversion 0も拒否)、`legacy_warning`、`CURRENT_KNOWLEDGE_SCHEMA_VERSION`。
- `src/git.rs`: `milestone.yml`の監査用`commit_oid`のために`resolve_commit_oid`を追加。
- `src/milestone.rs`: `milestone_init`が`id`に加えて`commit_oid`と`knowledge_schema_version`を書き込む。冪等性の挙動は変更なし。`milestone.yml`の記録値をtagの現在の解決結果と突き合わせる`verify_audit_matches_tag`を追加。
- `src/changes.rs`: `compute_changes_with_warnings`を追加(各refのスキーマバージョンを一度だけ解決し、`milestone::verify_audit_matches_tag`と`knowledge_schema::ensure_compatible`を実行した上でChangeEventとlegacy warningの両方を返す。fail closedで`Err`となった場合も、該当するlegacy warningのテキストをその同じエラーのメッセージに折り込んでから返し、破棄しない)。`compute_changes`/`compute_changes_between_refs`はこれを呼ぶ薄いラッパーとなり、全呼び出し元が単一の解決経路を共有する。
- `src/backfill.rs`: `BackfillReport`に`incompatible: Vec<IncompatiblePair>`(`{ to_milestone, reason }`。`reason`はfail-closedゲートのエラーメッセージそのもの)と`warnings: Vec<String>`を追加。`backfill_run_with_policy`は`compute_changes_with_warnings`を呼び、`ErrorKind::Unsupported`のペアをrunの中断ではなくスキップとして扱い(エラーテキストは破棄せず保持する)、warningを収集する。
- `src/application.rs` / `src/presentation.rs`: `CommandOutcome::ChangesComputed`に`warnings: Vec<String>`を追加し、両Presenterでレンダリングする。`JsonPresenter`は空の場合`"warnings":[]`ではなくキー自体を省略する。
- `src/cli.rs`: `backfill run`は収集したwarningと非互換ペアの実際の理由を出力し、1件でもスキップがあれば終了コード`1`で終了する。
- `src/init.rs`: `markharness init`が既存のトップレベル`schema_version = 1`に加えて`[knowledge]\nschema_version = 1`を書き込む。
