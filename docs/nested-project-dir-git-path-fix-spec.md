# 修正指示書: プロジェクトディレクトリがGitリポジトリのルートでない場合に `git show <ref>:<path>` が失敗する

**Status**: 完了(Done)
**Created**: 2026-08-13
**Updated**: 2026-08-13 — 実装完了。当初案(§2〜§5、`<rev>:<path>`に`./`を前置する方式)から、`<rev>:<path>`構文そのものを廃止し`ls-tree`/`cat-file`ベースに書き換える方式に変更して実装したため、§2〜§5を実装内容に合わせて全面的に書き換え。§7を全項目チェック済みに更新
**関連ドキュメント**: [cli-manual.md](./cli-manual.md) 1.11節(`changes compute`)・1.12節(`backfill run`)・1.14節(`execution record`)、`src/git.rs`、`src/id_cache.rs`、`src/execution.rs`
**発見経緯**: 外部プロジェクト(`todo2`。アプリ本体をリポジトリ直下、markharnessの`knowledge/`等を`docs/`配下に`markharness init --dir docs`で初期化)で`markharness execution record`を実行した際に発覚。実運用でこの配置(markharnessのプロジェクトディレクトリ ≠ gitリポジトリのルート)を試みる利用者が実在することを示す一次事例。

---

## 0. 設計判断としての位置付け(2026-08-12 追記)

本書§8で「対象外」としていた「markharnessのプロジェクトディレクトリを常にリポジトリルートに置くべきか、サブディレクトリでもよいか」という設計判断は、2026-08-12 に検討し結論が出た。以降のTDD実装(§5〜§7)はこの結論を前提に進める。

### 結論

プロジェクトディレクトリ(`knowledge/` 等の親ディレクトリ、`--dir`/cwdで指定する `root`)は、gitリポジトリのルートと一致している必要はない。**リポジトリ内の任意のサブディレクトリに置く配置を正式にサポートする**(黙認ではなく正規の運用パターンとして扱う)。

### 理由(要旨)

- `todo2` の一次事例(本書冒頭)が既にこの配置を実運用で要求している。
- `changes compute` はマイルストーン間で `knowledge/` 配下の部分木のtree SHAだけを比較する設計であり、milestone自体(`git tag`)はリポジトリ全体に対するグローバルな概念である。プロダクト本体のリリースタグをそのままmilestoneとして流用し、markharness側は関心のある部分木だけを見る、という分担は自然であり、サブディレクトリ配置と整合的である。
- 「常にリポジトリルート」を強制すると、(a) markharness専用のリポジトリを別途用意してプロダクトリポジトリのタグと同期する運用負荷が生じるか、(b) プロダクトのソースツリー直下に `knowledge/`/`generated/` 等を同居させることになり、どちらもリポジトリ構成として不自然になる。この不利益に対応する、サブディレクトリ配置側の構造的な不利益は見当たらない。
- サブディレクトリ配置に付随するリスクは、本書が対象とする `<rev>:<path>` 構文のパス解釈バグ(§2)に限定される。`rg 'git_ref\}:' src/` で該当箇所が `tree_sha`/`show_blob` の2関数のみであることを確認済み(§3)であり、他の `git -C root ...` 呼び出し(`tag`/`notes`/`merge-base`/`log`)はリポジトリ全体に対する操作でありパス非依存のため、`root` がリポジトリルートかサブディレクトリかによらず正しく動く。

### 派生する対応事項

- `docs/cli-manual.md` は `--dir` を「対象プロジェクトディレクトリ(gitリポジトリのルート)」と説明しており(`milestone init` オプション表、1.11節付近)、この結論と矛盾する。「gitリポジトリ内の任意のディレクトリ(リポジトリ自体のルートである必要はない)」という趣旨に修正する(§7の受け入れ基準に反映)。
- コード上は「プロジェクトディレクトリ」と「gitリポジトリルート」を単一の `root: PathBuf` として暗黙に同一視しており、これが本バグの温床になっている。今回の修正(§4)はこの2概念を明示的に区別する実装変更までは含まないが、将来的な再発防止のため、両概念が異なりうることをコードコメント等で明記することが望ましい(実装の要否は別途判断する)。
- `init.rs::ensure_gitignore`(`.gitignore` への `.markharness-cache/` 追記)は検討の結果、**変更不要と判断した**。同関数はgit呼び出しを一切行わずプロジェクトディレクトリ直下の `.gitignore` に対する純粋なファイル操作であり、`.markharness-cache/` もプロジェクトディレクトリ直下に生成されるため、ネストした `.gitignore` として意味的に正しく機能する(gitの `.gitignore` はネストしたディレクトリに置いても、そのディレクトリ以下にのみ適用されるため無視対象とズレが生じない)。リポジトリルートの `.gitignore` に書く方式に変更すると、リポジトリルート検出への新規git依存が発生し、1リポジトリ内に複数のmarkharnessプロジェクトが存在するケースでエントリの所属が曖昧になるなど、変更する側にデメリットがある。よって本修正の対象に含めない。
- `git rev-parse --show-toplevel` 相当のリポジトリルート自動検出機能の追加(`--dir` 未指定時にcwdから上位探索する等)は、本設計判断を運用面で完成させる自然な次の課題だが、本書のスコープ外のまま据え置く(§8参照)。

---

## 1. 再現手順と症状

```console
$ ls  # リポジトリルート(todo2/)
index.html  style.css  app.js  docs/  .git/

$ cd todo2
$ markharness milestone init 2026-08-13 --dir docs   # 成功する
initialized executions/2026-08-13/milestone.yml

$ markharness execution record tc-valid-title-001 \
    --milestone 2026-08-13 --result pass --executor yamada --dir docs
error: filesystem error: git show 2026-08-13:knowledge/todo-management/add-todo/feature.yml failed: fatal: path 'docs/knowledge/todo-management/add-todo/feature.yml' exists, but not 'knowledge/todo-management/add-todo/feature.yml'
hint: Did you mean '2026-08-13:docs/knowledge/todo-management/add-todo/feature.yml' aka '2026-08-13:./knowledge/todo-management/add-todo/feature.yml'?
```

`--dir docs` を外して `cd docs && markharness execution record ...`(cwdベース)にしても同じエラーになる。`validate` / `generate` / `verify` / `knowledge apply` / `milestone init` は同じ配置で問題なく動作する。

## 2. 原因

`src/git.rs` の2関数が、Gitの `<rev>:<path>` blob/treeオブジェクト指定構文を使っていた(修正前のコード)。

```rust
// 修正前: src/git.rs:83-98
pub fn tree_sha(root: &Path, git_ref: &str, path_in_repo: &str) -> io::Result<Option<String>> {
    let rev = format!("{git_ref}:{path_in_repo}");
    // ...  git -C root rev-parse --verify --quiet <rev>

// 修正前: src/git.rs:105-107
pub fn show_blob(root: &Path, git_ref: &str, path_in_repo: &str) -> io::Result<String> {
    run_git(root, &["show", &format!("{git_ref}:{path_in_repo}")])
}
```

**`<rev>:<path>` 構文のpathは、`./`または`../`で始めない限り常にリポジトリルートからの相対パスとして解釈される。** `git -C <root>` でカレントディレクトリを変えても、この解釈規則には影響しない。

一方、呼び出し元の `id_cache::resolve_feature_versions`(`src/id_cache.rs:98-`)は `git::ls_tree_recursive`(`git ls-tree -r -t <ref> -- <path>` のpathspec構文)で得た `entry.path` を、そのまま `tree_sha`/`show_blob` に渡していた。`ls-tree`のpathspec引数は通常のgitサブコマンドと同様に**カレントディレクトリ相対**で解釈され、出力されるpathも実行時のcwd(`-C root`)相対になる。

つまり:
- `root` がリポジトリルートと一致する場合 → cwd相対パス = リポジトリルート相対パスなので、たまたま一致して動く。
- `root` がリポジトリのサブディレクトリ(例: `docs/`)の場合 → `ls-tree`はcwd(`docs/`)相対の `"knowledge/..."` を返すが、`show`/`rev-parse --verify`の`<rev>:<path>`はリポジトリルート相対解釈のため、本来必要な `"docs/knowledge/..."` ではなく `"knowledge/..."` を探しにいって失敗する。

### 実験による検証

```console
$ git -C sub show t1:knowledge/req/feat/feature.yml
fatal: path 'sub/knowledge/req/feat/feature.yml' exists, but not 'knowledge/req/feat/feature.yml'
hint: Did you mean 't1:sub/knowledge/req/feat/feature.yml' aka 't1:./knowledge/req/feat/feature.yml'?

$ git -C sub show "t1:./knowledge/req/feat/feature.yml"
id: feat   # 成功
```

`./` を先頭に付けると、gitはcwd相対として解釈するようになる(`man gitrevisions` の `<rev>:<path>` 節に明記された挙動)。この実験は当初「`./`前置」案(§4参照)の根拠として行ったが、最終的には後述の通り `<rev>:<path>` 構文自体を使わない方式を採用した。

## 3. 影響範囲

`<rev>:<path>` 構文を使っていたのは `src/git.rs` の2関数のみ(`rg 'git_ref\}:' src/` で確認済み)。

| 関数(修正前) | 呼び出し元 | 影響コマンド |
|---|---|---|
| `tree_sha` (git.rs:83) | `id_cache::compute_cache_key` | `execution record`、`changes compute`、`backfill run`(いずれも`.markharness-cache/`のキー計算経由) |
| `show_blob` (git.rs:105) | `id_cache::resolve_feature_versions` | 上記3コマンド全て(Feature idを`feature.yml`から読むため) |

`ls_tree_recursive`(pathspec構文、git.rs:46-76)は対象外。`validate`/`generate`/`verify`/`knowledge apply`/`milestone init`はこの経路を通らないため影響なし。

採用した修正(§4)では、`show_blob` の呼び出し元である `id_cache::resolve_feature_versions`(`src/id_cache.rs`)自体も変更対象に含まれる(§4参照)。

## 4. 修正方針(採用案)

当初案(`path_in_repo` に `./` を前置して `<rev>:<path>` 構文を維持したまま `-C root` 相対に解釈させる方式)を検討したが、実装時により根本的な代替案を採用した: **`<rev>:<path>` 構文そのものを使わない。**

`id_cache::resolve_feature_versions` が `ls_tree_recursive` から得る `TreeEntry` には、そもそもGitのpathspec解決(`-C root` 相対で正しく動く)を経て取得した `sha`(blob/treeのcontent-addressed SHA)が既に含まれている。この `sha` を使えば、パス文字列をもう一度gitに解釈させる必要がなくなり、「`<rev>:<path>` がリポジトリルート相対にしか解釈されない」というGitの仕様そのものを回避できる。

- `tree_sha(root, git_ref, path_in_repo)`: シグネチャは維持。内部実装を `git rev-parse --verify <rev>:<path>` から `git ls-tree <ref> -- <path>` に差し替えた。`ls-tree` のpathspec引数は他のgitサブコマンド引数と同様に `-C root` 相対で解釈されるため、`root` がリポジトリのサブディレクトリであっても正しく解決する。`compute_cache_key` はentryを経由しない直接呼び出し(`git::tree_sha(root, git_ref, "knowledge")`)であり、pathから解決する必要が残るため、この関数自体は残した。
- `show_blob(root, git_ref, path_in_repo)`: **削除**。呼び出し元がid_cache.rs一箇所のみで、かつ呼び出し元は既に対象blobの `sha` を保持していたため、パスベースの関数自体が不要と判断した。
- `show_blob_by_sha(root, sha)`(新設): `git cat-file -p <sha>` でblob内容を読む。SHAは内容アドレスであり、パス解釈・cwd相対性の問題が原理的に発生しない。
- `id_cache::resolve_feature_versions`: `git::show_blob(root, git_ref, &entry.path)` の呼び出しを `git::show_blob_by_sha(root, &entry.sha)` に変更(`entry.sha` は `ls_tree_recursive` の戻り値に既に含まれていたものを使うだけで、git呼び出し回数は変わらない)。

```rust
// src/git.rs(採用後)
pub fn tree_sha(root: &Path, git_ref: &str, path_in_repo: &str) -> io::Result<Option<String>> {
    let raw = run_git(root, &["ls-tree", git_ref, "--", path_in_repo])?;
    let Some(line) = raw.lines().next() else { return Ok(None) };
    let Some((meta, _path)) = line.split_once('\t') else { return Ok(None) };
    Ok(meta.split_whitespace().nth(2).map(|sha| sha.to_string()))
}

pub fn show_blob_by_sha(root: &Path, sha: &str) -> io::Result<String> {
    run_git(root, &["cat-file", "-p", sha])
}
```

```rust
// src/id_cache.rs(採用後、resolve_feature_versions内)
let content = git::show_blob_by_sha(root, &entry.sha)?;
```

## 5. TDD実装手順(`.github/instructions/tdd-workflow.instructions.md` に準拠)

`src/git.rs` の `#[cfg(test)] mod tests` に、プロジェクトディレクトリがリポジトリのサブディレクトリであるケースのテストを追加した。

```rust
#[test]
fn tree_sha_resolves_path_when_root_is_a_subdirectory_of_the_repo() {
    let repo = init_repo(); // git init はリポジトリのルートで行う
    fs::create_dir_all(repo.path().join("sub/knowledge/req/feat")).unwrap();
    fs::write(repo.path().join("sub/knowledge/req/feat/feature.yml"), "id: feat\n").unwrap();
    commit_all(repo.path(), "add feature");
    run_git(repo.path(), &["tag", "t1"]).unwrap();

    let sub_root = repo.path().join("sub"); // root != リポジトリルート
    let sha = tree_sha(&sub_root, "t1", "knowledge").unwrap();

    assert!(sha.is_some());
}

#[test]
fn show_blob_by_sha_returns_content_of_the_blob() {
    let dir = init_repo();
    fs::create_dir_all(dir.path().join("knowledge/req/feat")).unwrap();
    fs::write(dir.path().join("knowledge/req/feat/feature.yml"), "id: feat\nlabel: v1\n").unwrap();
    commit_all(dir.path(), "add feature");
    run_git(dir.path(), &["tag", "m1"]).unwrap();
    let entries = ls_tree_recursive(dir.path(), "m1", "knowledge").unwrap();
    let blob = entries.iter().find(|e| e.kind == ObjectKind::Blob).unwrap();

    let content = show_blob_by_sha(dir.path(), &blob.sha).unwrap();

    assert_eq!(content, "id: feat\nlabel: v1\n");
}

// show_blob_by_shaはSHA直接指定でパス解決を経ないため、rootがサブディレクトリ
// であっても影響を受けないことを示す確認テストも追加した
// (show_blob_by_sha_works_when_root_is_a_subdirectory_of_the_repo)。
```

`show_blob`(パスベース)を使っていた既存テスト `show_blob_returns_content_of_file_at_ref` は、新しい `show_blob_by_sha` を使う形に書き換えた(`show_blob_by_sha_returns_content_of_the_blob`)。`tree_sha` の既存テスト(`tree_sha_returns_the_tree_object_sha_for_an_existing_path_at_ref`、`tree_sha_returns_none_when_path_absent_at_ref`)はシグネチャ・挙動とも変更していないため、そのまま流用した。

実装(`tree_sha`のls-tree化、`show_blob`の削除と`show_blob_by_sha`の新設、`id_cache.rs`の呼び出し変更)後、追加した新規テストを含む全テストがパスすることを確認した(§7参照)。作業はRed/Green/Refactorのコミットを分けず、1コミットにまとめている。

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
cargo audit
```

## 6. 統合検証(実際のバグ発生条件の再現)

修正後、`todo2`プロジェクト相当の構成(リポジトリルート直下にアプリファイル、`docs/`配下にmarkharnessプロジェクト)に対して、実際に`execution record`が通ることを確認する。`c:\Users\papa\work\todo2` を使う場合:

```bash
cd c:/Users/papa/work/todo2
markharness execution record tc-valid-title-001 \
  --milestone 2026-08-13 --result pass --executor <name> --dir docs
cat docs/executions/2026-08-13/results.yml
```

`verified_feature_tree_shas` にFeature `add-todo` のtree SHAが記録され、エラーが出ないことを確認する。あわせて `changes compute` も同条件(サブディレクトリ配置)で1回動作確認する(このバグの影響範囲に含まれるため)。

**実施結果(2026-08-13)**: `c:\Users\papa\work\todo2`(使い捨て・検証専用。作業完了後に削除予定)に対し、修正版バイナリで以下を実行し、いずれも成功を確認した。

```console
$ markharness execution record tc-valid-title-001 --milestone 2026-08-13 --result pass --executor qa-verify --dir docs
recorded pass for tc-valid-title-001 into executions/2026-08-13/results.yml
```

`docs/executions/2026-08-13/results.yml` に `verified_feature_tree_shas: {add-todo: 6f0d7b08...}` が記録され、エラーは発生しなかった。

`changes compute` の検証には比較対象となる2つ目のマイルストーン(タグ)が必要だったため、`add-todo/feature.yml` に検証用コメント行を追記した使い捨てコミット・タグ(`2026-08-13-verify`)を追加した上で実行した。

```console
$ markharness changes compute 2026-08-13 2026-08-13-verify --dir docs
computed 1 change event(s) into changes/2026-08-13-verify.yaml
```

`changes/2026-08-13-verify.yaml` に `feature_id: add-todo` の `ChangeEvent`(`from_tree_sha`/`to_tree_sha` が異なる値)が1件出力され、エラーは発生しなかった。

## 7. 受け入れ基準

- [x] 上記の新規テスト(`tree_sha_resolves_path_when_root_is_a_subdirectory_of_the_repo` 等、§5参照)が追加され、いずれも成功する
- [x] 既存の `cargo test` が全件パス(回帰なし、242件 + 追加分すべて成功)
- [x] `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` が通る(`cargo audit` はadvisory-db側の重複ID(`RUSTSEC-2026-0244`)によるパースエラーで実行不能。本修正とは無関係な既存のツール側インフラ不具合であり、対応は別課題とする)
- [x] `todo2`(サブディレクトリ配置のプロジェクト)に対して `execution record` が実際に成功する(統合検証、上記実施結果参照)
- [x] `docs/cli-manual.md` の該当コマンド節(1.11 / 1.12 / 1.14)に、この制約が解消された旨を追記した。あわせて `--dir` の説明にある「対象プロジェクトディレクトリ(gitリポジトリのルート)」という記述(`milestone init` オプション表)を、§0の結論に基づき「gitリポジトリ内の任意のディレクトリ(リポジトリ自体のルートである必要はない)」という趣旨に修正した
- [x] `init.rs::ensure_gitignore` には変更を加えていない(§0で変更不要と判断済み。範囲外の修正を防ぐための確認項目)

## 8. 対象外(Out of scope)

- markharnessのプロジェクトディレクトリを「常にリポジトリルートに置くべきか、サブディレクトリでもよいか」という設計判断は §0(2026-08-12 追記)で結論済み(サブディレクトリ配置を正式サポート)。本書はその結論を前提に、どの配置でも同じ挙動になることを保証する修正のみを行う。
- `--dir` を一切指定せずカレントディレクトリから自動的にgitリポジトリルートを検出する(`git rev-parse --show-toplevel`相当)機能の追加は、§0で言及した派生課題であり、引き続き別課題とする。
