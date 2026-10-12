# 0047: `impact --worktree`は未追跡のファイルも含め、コミットした時と同じ変更を見る

## ステータス

Accepted(2026-10-12決定)。[0046](0046-impact-worktree.md)の決定2(未追跡のファイルを対象外とする)と、決定3の一時インデックスの作り方(`git add -u`)を置き換える。0046のそれ以外の決定は変わらない。

## 背景

[0046](0046-impact-worktree.md)の`impact --worktree`は、追跡されているファイルだけを対象にした。そのため、`knowledge/`に新しい要求やFeatureを作った直後は、`git add`しないと影響を確かめられなかった(Issue #131)。作業者が確かめたいのは「今コミットしたら、どう見えるか」であり、新規ファイルだけが`git add`の有無で見えたり見えなかったりするのは、その期待とずれる。0046は「未追跡のファイルも含める」を、`.gitignore`との整合を決める必要があるとして初期実装の範囲外にしていたが、`git add -A`がその整合を自動で処理するので、決める必要はなくなる。

## 決定

### 1. 未追跡のファイルも対象にする

対象は、作業者が`git add -A`してコミットした場合に得られるツリーとする。ステージ済み・未ステージ・未追跡を区別せず、`.gitignore`で無視されないすべてのファイルの作業ツリー上の現在の内容を対象にし、削除も反映する。無視すべきファイルの判定は`git add -A`に任せ、`markharness`は持たない。無視したいファイルは`.gitignore`に書く。

### 2. 範囲はリポジトリ全体とする

`.markharness/knowledge`に限らず、リポジトリ全体に`git add -A`をかける。外部要求の`source_locator`が指す`.sdoc`のように`knowledge/`の外にあるファイルも、コミットした時と同じ見え方にするためである。`knowledge/`だけに限ると、それらの未コミットの変更だけが食い違って見える。

### 3. 一時インデックスは実インデックスのコピーから作る

実インデックスのコピーを`GIT_INDEX_FILE`に指定し、`git add -A`、`git write-tree`の順に実行する。空のインデックスから作らないのは、追跡中なのに`.gitignore`に該当するファイルを、実際のコミットと同じくツリーに残すためである。0046の決定3のうち、`git add -u`を`git add -A`に変えるだけで、それ以外(ツリーOIDを既存のref向けの処理にそのまま渡すこと、実インデックスとHEADを変更しないこと)は変わらない。

### 4. オブジェクトDBへの書き込みを抑えるガードは設けない

未追跡のファイルのblobもオブジェクトDBに書き込まれる。`.gitignore`に入れていない大きなファイル(ビルド成果物や動画など)があると、実行のたびにそのハッシュ計算と書き込みが発生する。サイズ上限、警告、`--include-untracked`のようなopt-inは設けない。`git add -A`も同じ挙動であり、現時点でそのようなファイルが問題になった事例はない(YAGNI)。代わりに、見積もりと「無視すべきファイルは`.gitignore`に書く」ことをcli-manualに明記する。書き込まれるblobのうち参照されないものは`git gc`が回収する。

## 影響範囲

- `src/git.rs`: `write_tree_of_tracked_worktree`を`write_tree_of_worktree`に改名し、`git add -u`を`git add -A`に変更
- `src/impact.rs`: 呼び出しと`compute_worktree`のdocコメントの更新
- `src/cli.rs`: `--worktree`のhelpから「未追跡は対象外」を除く
- `tests/impact_cli.rs`: 既存の未追跡のテストを反転し、`.gitignore`・新規Feature・`knowledge/`の外の`.sdoc`・実インデックス不変のテストを追加
- `docs/ja/cli-manual.md`・`docs/en/cli-manual.md`(§1.13)、`docs/ja/design/cli-read-model-design.md`・`docs/en/design/cli-read-model-design.md`、`docs/ja/design/markharness-v2-design.md`・`docs/en/design/markharness-v2-design.md`(§9.2.2)

## 検討したが採用しない選択肢

- **現状維持(`git add`してから実行する)**: 新規作成のたびに`git add`が要り、コミットした時と同じ見え方にならない。
- **`.markharness/knowledge`だけに`-A`をかける**: 範囲は小さいが、`knowledge/`の外にある外部要求の参照先の変更が、コミットした時と違う見え方になる。
- **ファイルシステムから直接読む**: `id_cache`のtree SHA比較、`changed_between`、`blob_sha_at`を作業ツリー版として書き直す必要があり、変更が大きい。
- **空のインデックスから`git add -A`で作る**: 追跡中なのに`.gitignore`に該当するファイルがツリーから消え、実際のコミットと食い違う。
- **未追跡のファイルを含めるかを切り替えるオプションを設ける**: 含めない動機が確認されておらず、結果の意味が実行ごとに変わる(YAGNI)。
