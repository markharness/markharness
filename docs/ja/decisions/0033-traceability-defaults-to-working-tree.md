# 0033: `traceability`は`--at`省略時に作業ツリーを読む

## ステータス

Accepted(2026-09-16決定)。[0032](0032-cli-read-model-seam.md)が導入した`traceability`コマンドの入力ソースを拡張する。

## 背景

[0032](0032-cli-read-model-seam.md)は、`traceability`・`impact`・`coverage`の3コマンドを、いずれも指定したGit ref時点のコミット内容だけを読む設計に揃えた。実装した`traceability --at <ref>`(既定値`HEAD`)は、`GitTreeKnowledgeSource`経由でGitオブジェクトから読むため、対象refに最低1回のコミットが存在しないと動作しない(未コミットの空リポジトリでは`HEAD`が解決できず`git ls-tree`が失敗する)。

これは`markharness-view`の実際の利用シーンで不便であることが判明した。GUIやviewが、利用者がKnowledgeを編集している最中にリアルタイムで関係を表示しようとすると、変更のたびにコミットを要求するのは編集ループとして成立しない。

一方、`impact`・`coverage`にはこの制約を外す動機がない。

- `impact`は`base..head`という2点間の比較そのものが目的であり、両方とも既にコミットされていなければ比較対象として成立しない。
- `coverage`はリリース監査(「あるコミット時点で何が検証対象だったか」)が主眼であり、未コミットの作業ツリー状態を混ぜると再現性(設計原則P3)が壊れる。

`traceability`には2点比較も監査要件もなく、「今のKnowledgeの関係を見る」という単純な問い合わせである。`generate`・`verify`は既に`WorkingTreeKnowledgeSource`経由で作業ツリーを直接読んでおり、`traceability`だけがGit ref必須である理由は本質的にはない。

## 決定

### 1. `--at`はrequiredではなくoptionalにし、省略時は作業ツリーを読む

`markharness traceability`の`--at <ref>`を省略した場合、`GitTreeKnowledgeSource`ではなく`WorkingTreeKnowledgeSource`(`generate`・`verify`と同じ経路)を使い、作業ツリーの現在の内容をそのまま読む。`--at <ref>`を指定した場合は、[0032](0032-cli-read-model-seam.md)決定1のとおり従来通りGit ref時点を読む。

デフォルト値を`HEAD`から「省略」に変える(`--at`に`default_value`を設けない)。「省略時は作業ツリー」という振る舞いは、`generate`・`verify`が既に確立している既存の直感と一致する。

### 2. 出力の`at`フィールドで読み取り元を区別できるようにする

作業ツリーを読んだ場合、`at`フィールドには固定文字列`"working-tree"`を格納する。Git ref指定時は、従来通り利用者が指定した文字列(解決前のref名)をそのまま格納する。これにより、出力を見るだけでどちらのソースを読んだ結果かが判別できる。

`working-tree`という値は、`record_kind`と同様「レコードの取り違えを防ぐための識別情報」の一種であり、Git refの値と衝突しない予約語として扱う(利用者がGit refに`working-tree`という名前を付けた場合との衝突は、既存の`HEAD`等と同様、実用上のリスクとして許容する)。

### 3. Requirement・Featureの読み取りに作業ツリー版を追加する

現在の`requirements_at`・`features_at`(`src/traceability.rs`)は、`git::ls_tree_recursive`/`git::show_blob_by_sha`によるGitオブジェクト読み取りのみを実装している。作業ツリー読み取りに対応するには、同等の内容をファイルシステムから直接読む実装を追加する必要がある。具体的な実装方法(関数を分けるか、`KnowledgeSource`同様の抽象化を導入するか)は、本ADRでは決定せず、実装フェーズの設計判断とする。

### 4. `impact`・`coverage`は対象外とする

両コマンドに作業ツリー読み取りへの対応を広げる具体的な必要性は、現時点で確認されていない(YAGNI)。将来必要になれば、その時点で別のADRとして扱う。

### 5. 出力契約(`record_kind`・`schema_version`・フィールド構成)は変更しない

本ADRは入力ソースの選択肢を増やすものであり、`TraceabilityReadModel`のフィールド構成自体には影響しない。

## 影響範囲

- `src/traceability.rs`: `compute`の引数(`git_ref: &str`から、省略可能な形へ変更)、`requirements_at`・`features_at`の作業ツリー版実装、`at`フィールドの値決定ロジック
- `src/cli.rs`: `Traceability`コマンドの`at`引数を`Option<String>`にし、`default_value = "HEAD"`を外す
- `docs/ja/design/cli-read-model-design.md`・`docs/en/design/cli-read-model-design.md`: `traceability`の`--at`説明、コマンド対応表を更新(本ADRに合わせて更新済み)
- `docs/ja/cli-manual.md`・`docs/en/cli-manual.md`(§1.25): `--at`省略時の挙動を追記(実装時に反映)
- **変更不要**: `src/impact.rs`・`src/coverage.rs`、両コマンドのCLI引数定義

## 検討したが採用しない選択肢

- **現状維持(常にコミットを要求する)**: `markharness-view`の編集中プレビューという実際の困りごとを解決できない。
- **`--at working-tree`のような明示的な値を新設する**: `--at`を省略した場合の既定動作として作業ツリーを読む方が、`generate`・`verify`と同じ「省略時は作業ツリー」という直感に合う。明示的な特別値を追加すると、利用者が「`--at`を指定しないと何が起きるか」を覚える負担が増えるだけで、具体的な利点がない。
- **`impact`・`coverage`にも同様に作業ツリー対応を広げる**: 両コマンドの目的(2点比較・リリース監査)には未コミット状態を混ぜる動機がなく、具体的な要求も確認されていない(YAGNI)。
