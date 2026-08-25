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

両者とも同じ`changes::compute_changes_between_refs`(`ChangeAnalyzer::compute` → `compute_changes`、`backfill_run_with_policy`はこれを直接呼ぶ)を経由するため、バージョンゲートは1箇所に実装するだけで両方に自動的に適用される。

### 5. 未対応のバージョン組み合わせはfail closedにする

`from`と`to`が異なる既知バージョンを報告した場合、またはいずれかがこのCLIビルドが知らない未来のバージョンを報告した場合、`compute_changes_between_refs`は何も計算する前に`ErrorKind::Unsupported`を持つ`io::Error`を返す。ChangeEventは生成されず、呼び出し元(`application::compute_changes`)は`replace_file`による書き込みに到達しないため、既存の`changes/<to>.yaml`は変更されずに残る。これは新しいJSON専用エラーチャンネルを追加するのではなく、既存の`error: {err}` → stderr → exit 1 の経路(`src/main.rs`)をそのまま再利用する。

`backfill run`は同じ`Unsupported`エラーを、run全体を中断する理由ではなく、そのペアだけのスキップとして扱う。該当ペアは`BackfillReport.incompatible`に記録され、`git notes`には記録されない。そのため後続のrunは自動的に再試行し、converterが実装された時点で手動の再実行操作なしに成功するようになる。

### 6. バージョンが記録されていないrefはlegacyスキーマバージョン1とみなす

`.markharness/knowledge/`は本機能導入以前の全ての未変更refにおいてバージョン情報を持たない。そのため`config.toml`に`[knowledge]`テーブルがない(あるいは`config.toml`自体が存在しない)refは、legacyスキーマバージョン1とみなし、`changes compute`はこの推定を無言で行わずwarningとして表示する。このwarningは構造化フィールド(`CommandOutcome::ChangesComputed.warnings: Vec<String>`)であり、`HumanPresenter`(`warning: ...`行)と`JsonPresenter`(既存JSON envelope内の`"warnings"`配列)の両方でレンダリングされる。既にバージョン管理されたJSON contractへのフィールド追加は非破壊であり、そのenvelope自体の`schema_version`のbumpは不要(`docs/ja/design/verification-plan-canonical-model-design.md`の既存規約: フィールドの削除・リネームのみがbumpを要する)。

### 対象外

issue #29が明示した対象外と一致する: 異なるスキーマ間のconverter、schema-only migrationを除外する意味的差分(semantic diff)、semantic hashおよびcanonicalization rule versionの変更、`--allow-raw-schema-diff`のようなescape hatch、既存Knowledgeを新スキーマへ書き換えるmigrationコマンド。本ADRはバージョンを解決可能にし、未対応の比較を無言で行わず安全に停止させることのみを行う。

## 対応内容

- `src/knowledge_schema.rs`(新規): `resolve`(ref → `ResolvedSchemaVersion { version, is_legacy }`)、`ensure_compatible`(fail closedのゲート)、`legacy_warning`、`CURRENT_KNOWLEDGE_SCHEMA_VERSION`。
- `src/git.rs`: `milestone.yml`の監査用`commit_oid`のために`resolve_commit_oid`を追加。
- `src/milestone.rs`: `milestone_init`が`id`に加えて`commit_oid`と`knowledge_schema_version`を書き込む。冪等性の挙動は変更なし。
- `src/changes.rs`: `compute_changes_between_refs`がtree SHA比較の前に`ensure_compatible`を呼ぶ。
- `src/backfill.rs`: `BackfillReport`に`incompatible: Vec<String>`を追加。`backfill_run_with_policy`は`ErrorKind::Unsupported`のペアをrunの中断ではなくスキップとして扱う。
- `src/application.rs` / `src/presentation.rs`: `CommandOutcome::ChangesComputed`に`warnings: Vec<String>`を追加し、両Presenterでレンダリングする。
- `src/init.rs`: `markharness init`が既存のトップレベル`schema_version = 1`に加えて`[knowledge]\nschema_version = 1`を書き込む。
