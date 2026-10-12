# 0046: `impact --worktree`でHEADと作業ツリーの未コミット変更を比較する

## ステータス

Accepted(2026-10-12決定)。[0033](0033-traceability-defaults-to-working-tree.md)の決定4のうち、`impact`を作業ツリー対応の対象外とする部分を置き換える(`coverage`は対象外のまま)。

## 背景

`impact`は、`--base`と`--head`をGit refとして解決し、要求・Feature・TestCaseをそのrefのツリーから読む。そのため、要求やFeatureを編集している最中に「この変更がどの要求とTestCaseに影響するか」を確かめるには、いったんコミットする必要があった。[0033](0033-traceability-defaults-to-working-tree.md)決定4は「2点比較に未コミット状態を混ぜる動機が確認されていない(YAGNI)」として`impact`を対象外にしたが、コミット前に影響を確かめたいという要求(Issue #129)が、その動機にあたる。

## 決定

### 1. `impact --worktree`を追加する

`--worktree`は、HEADを基準とし、HEADから作業ツリーまでの未コミット変更に基づいて影響を分析する。`--base`・`--head`・`--fail-on-findings`のいずれとも併用できない(clapの`conflicts_with`)。基準をHEAD固定とするのは、任意のrefとの比較はコミットすれば既存の`--base`/`--head`でできるためで、「任意のrefから作業ツリー」は動機が確認されていないので扱わない(YAGNI)。

### 2. 対象は追跡中のファイルの現在の内容とする

ステージ済みの変更と未ステージの変更を区別せず、追跡されているファイルの作業ツリー上の現在の内容を対象にする。削除も反映する。`git add`したが未コミットの新規ファイルは追跡対象として扱う。未追跡のファイルは対象外で、`git add`すると結果に現れる。この制約はcli-manualとコマンドのhelpに明記する。

### 3. 作業ツリーは一時インデックス経由でツリーオブジェクトにして読む

実インデックスのコピーを`GIT_INDEX_FILE`に指定し、`git add -u`、`git write-tree`の順に実行して、作業ツリーに対応するツリーOIDを得る。このOIDを既存のref向けの処理(`ls-tree`、blob SHAの比較、`id_cache`のtree SHA比較、`GitTreeKnowledgeSource`)にそのまま渡す。コミット間の`impact`と、差分と識別子の意味が同一になる。実インデックスとHEADは変更しない。

副作用は、`git add -u`が書き込むblobがオブジェクトDBに増えることである。HEADと同じ内容のファイルは既存のblobと同一ハッシュなので増えず、増えるのは編集中のファイルの分だけである。見積もりは1回あたり数十KB〜数百KBで、繰り返し実行しても参照されないblobは`git gc`が回収する。無視できる量と判断した。

### 4. 結果は暫定であり、`record_kind`で区別する

`record_kind`は`change_impact_worktree`とする。`schema_version`は全種別で`1`に固定されており、版では分岐しない([markharness-v2-design.md](../design/markharness-v2-design.md)§9.2.2)。フィールド構成は`change_impact`と同一にして、受け取る側が同じパーサーで読めるようにする。値だけが次のとおり異なる。

- `base_commit`: HEADのコミットOID。
- `head_commit`: 固定文字列`"worktree"`。コミットOIDではない。作業ツリーに対応するコミットは存在しないため、OIDを装わない。
- `rejected_trailers`: 常に空配列。

`record_kind`が異なるので、`change_impact`を想定した利用側が暫定結果を正式な結果として取り込む事故を防げる。作業ツリーは変わり続けるため、同じ入力で同じ結果を返すというコミット間の`impact`の性質(AC37)はこの結果には成り立たない。

### 5. `Spec-Reviewed`は数えない

範囲`HEAD..作業ツリー`の間にコミットが無いため、確認の記録(`Spec-Reviewed`トレーラー)は数えられない。判定は`followed_up`と`unconfirmed`の2つになり、`confirmed`は出ない。そのため、変更に触れた要求がある限り`--fail-on-findings`は常に終了コード2になるので、併用を拒否する(決定1)。

### 6. 履歴を辿らないので浅いクローンでも動く

HEADのコミットだけを使い、コミット範囲を辿らない。そのため、浅いクローンの検査はしない。HEADが存在しない(最初のコミット前の)リポジトリでは、比較する基準がないのでエラーで終了する。

## 影響範囲

- `src/impact.rs`: `compute_worktree`を追加し、`compute`と共通の分析部分を`analyze`として抽出
- `src/git.rs`: 追跡ファイルだけを対象にする一時インデックスの`write_tree_of_tracked_worktree`を追加
- `src/cli.rs`: `impact`に`--worktree`を追加、`--worktree`指定時は`--base`・`--head`を不要にする
- `tests/impact_cli.rs`: ステージ済み・未ステージ・未追跡・実インデックス不変・併用拒否・浅いクローン・HEADなしのテスト
- `docs/ja/cli-manual.md`・`docs/en/cli-manual.md`(§1.13)、`docs/ja/design/cli-read-model-design.md`・`docs/en/design/cli-read-model-design.md`、`docs/ja/design/markharness-v2-design.md`・`docs/en/design/markharness-v2-design.md`(§9.2.2の`record_kind`一覧)

## 検討したが採用しない選択肢

- **現状維持(コミットしてから`impact`を実行する)**: 一時的なコミットを作って戻す運用になり、履歴を汚すか後始末の手間が要る。
- **`WorkingTreeKnowledgeSource`のようにファイルシステムから直接読む**: `impact`が使う`id_cache`のtree SHA比較、`changed_between`、`blob_sha_at`を作業ツリー版として書き直す必要がある。差分の意味がコミット間と食い違うリスクもある。
- **未追跡のファイルも含める**: 無視すべきファイルの区別や`.gitignore`との整合を決める必要があり、初期実装の範囲を超える。必要になった時点で別に扱う。
- **ステージ済みの変更だけを対象にする**: `git add`の有無で結果が変わるため採らない。
- **`record_kind`は変えず`provisional`項目を足す**: 項目を見ない利用側が暫定結果を正式な結果として扱う。[markharness-v2-design.md](../design/markharness-v2-design.md)§9.2.2の「将来必要そうなフィールドをoptionalとして足さない」にも沿わない。
- **`head_commit`を持たない別の構造体にする**: 同じ内容を出力するのに型を分けると、受け取る側のパーサーが2つになる。
- **`--fail-on-findings`の意味を再定義して許可する**: 暫定結果をCIで判定する動機は確認されていない。
- **`changes compute --current-tree`で代用する**: 出力の単位がFeatureで、要求との対応と判定が無い。
