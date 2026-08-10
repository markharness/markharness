# Task: changes lineage generalization

Created: 2026-08-11

## Steps

- [x] 既存の `changes compute` と lineage 判定の挙動を確認する
- [x] 失敗するテストを追加する
- [x] `changes compute` でマイルストーン区間内のマージを検出して `from_tree_shas` を記録するように実装する
- [x] 追加したテストと既存テストを実行して確認する
- [x] 必要に応じてドキュメントを更新する

## Notes

- 既存実装は `to_milestone` が直接マージコミットを指す場合のみ対応している。
- ここではマイルストーン区間内の任意の位置で発生したマージを対象にする。
- **2026-08-11 追記(レビューで発覚)**：上記Stepsは全て `[x]` だが、実装は区間内で最初に見つかった2親マージ1件しか見ておらず、`git rev-list --parents --ancestry-path`はデフォルトで新しい順に出力するため実質「区間内で一番新しいマージのみ」しか記録されない。同一Featureが区間内で複数回真の分岐を起こすケースを見落とす。また `docs/テスト知識管理のGit-nativeモデル_統合版.md` §3.2「部分統合」・§3.6・§7 は旧来の制約(`to_milestone`が直接マージコミットの場合のみ)を記述したままで、実装と食い違っている。`docs/improvement-prompts.md` 項目2の「完了」表記も時期尚早だった。上記Stepsの `[x]` は取り消さず、この追記と下の Follow-up で実態を補う(checklist-workflowのルール5「ステップを消さない」に従い、誤りは追記で残す)。

## Summary

`changes compute` がマイルストーン区間内で見つかった 2 親マージを検出し、真の分岐なら `from_tree_shas` に両親の tree SHA を記録するように実装した。**ただし単一マージのみ対応の暫定実装であり、Follow-up で完成させる。**

---

## Follow-up: 複数マージ対応とドキュメント整合(2026-08-11)

Q5の検討結果、データモデルは (a) 最後のマージのみ記録・(b) フラット `Vec<String>` のいずれも却下し、(c) を採用する。ただし生の `Vec<[String; 2]>` ではなく、マージコミットSHAを添えた名前付き構造体にする。理由：(a)は他の真の分岐情報を静かに破棄し版履歴の正確性というRQ1の核心と矛盾する。(b)は「隣り合う2要素が1組」という規約を型で表現できず、既存コードベース(JSON Schemaの`additionalProperties: false`、`generate.rs`の決定的ソート等)の「暗黙の前提を作らない」方針と矛盾する。(c)は`merge_commit`を持たせることで`markharness changes lineage --commit <sha>`との突き合わせ(監査)が可能になり、コストもごく小さい。`CLAUDE.md`の設計ルール(後方互換性を前提にしない)により、既存`changes/*.yaml`との互換性は選定理由から除外した。

### 確定した設計

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrueDivergence {
    /// このFeatureが真の分岐と判定された、区間内のマージコミットSHA。
    /// `git show <merge_commit>` で監査できる。
    pub merge_commit: String,
    /// `[tree(P1), tree(P2)]`
    pub parent_tree_shas: [String; 2],
}
```

`ChangeEvent.from_tree_shas: Vec<String>` を `true_divergences: Vec<TrueDivergence>` に改名する(後方互換を前提にしないため、単一マージ時代の名前を実態に合わせて改める)。

### Steps

- [x] 失敗するテストを追加する: 同一区間内に2つの真の分岐マージがあるケースで、両方が `true_divergences` に記録されることを検証する(`tests/changes_cli.rs`)
- [x] `ChangeEvent.from_tree_shas: Vec<String>` を `true_divergences: Vec<TrueDivergence>` に変更する(`src/changes.rs`)。`TrueDivergence` 構造体を追加する
- [x] `find_merge_commit_in_interval` を「最初の1件を返す」から「区間内の全2親コミットを収集して返す」(`Vec<String>` を返す)に変更する(`find_merge_commits_in_interval`に改名)
- [x] `git rev-list --parents --ancestry-path` の出力は新しい順なので、`--reverse` を付けて**古い順**(発生順)で返すようにする(決定性・可読性のため。`generate.rs`のソート方針に合わせる)
- [x] `compute_changes` を、収集した各マージコミットについて `true_divergence_parent_tree_shas` を呼び、Featureごとに複数の `TrueDivergence` を集約するループに変更する
- [x] 既存のシングルマージ用テスト(`changes_compute_records_both_parent_tree_shas_when_to_milestone_is_a_true_divergence` 等)を新フィールド名・新構造体に合わせて更新する。`src/changes.rs`内のユニットテスト(`sample_event`等のヘルパー含む)も同様に更新
- [x] `docs/テスト知識管理のGit-nativeモデル_統合版.md` §3.2「部分統合(2026-08追記)」を、区間内の全マージに対応した旨に書き換える(`from_tree_shas`→`true_divergences`のフィールド名変更も反映)
- [x] 同資料 §3.6 実装状況表の「設計から簡略化」欄から、本項目に該当する記述を「実装済み」欄へ移す(表下の解説文も合わせて更新)
- [x] 同資料 §6 Threats to Validity・§7 Future Work から、本項目に該当する記述を削除する
- [x] `docs/improvement-prompts.md` 項目2の状況注記を、複数マージ対応完了後の実態に合わせて更新する
- [x] `docs/cli-manual.md` 1.11節(`changes compute`)の説明・出力例を新フィールド名・区間全体走査の挙動に合わせて更新する(レビューで追加発見: Follow-upの対象外だったが`from_tree_shas`を参照していた)
- [x] `docs/gap-analysis-mh-sample-test-case.md` §8 は過去の実行結果の記録のため書き換えず、フィールド名改名の注記のみ追記する(レビューで追加発見。§8.4の「未検証」記述にも追記で対応)
- [x] `cargo test` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` を通す

### Notes

- `TrueDivergence`のフィールド名・`true_divergences`という改名は設計判断のため実装前にユーザーへ最終確認し、承認を得た(2026-08-11)。
- レビューの過程で、Follow-up作成時に列挙していなかった `docs/cli-manual.md`・`docs/gap-analysis-mh-sample-test-case.md` にも旧フィールド名 `from_tree_shas` への参照が見つかった。前者はCLIの現在の振る舞いを説明する文書のため更新し、後者は過去の実行結果の記録のため本文は変更せず注記のみ追加する方針とした(ユーザー確認済み)。

## Follow-up完了

`cargo test`(230+件、全パス)・`cargo clippy --all-targets -- -D warnings`(警告0件)・`cargo fmt --check`を確認済み。複数マージ対応・ドキュメント整合ともに完了。

## 第三者レビュー(2026-08-11、コードレビューのみ・Rustツールチェイン未導入のサンドボックスのため`cargo`は未実行)

`src/changes.rs`・`tests/changes_cli.rs`・`schema/*.json`・`src/knowledge.rs`・論文本文を通読して確認した結果:

- `TrueDivergence`構造体・`find_merge_commits_in_interval`(`--reverse`で古い順)・`compute_changes`の複数マージ集約ループはFollow-upの確定設計通りに実装されている。2マージが同一区間にあるケースを再現する専用テスト(`changes_compute_records_a_true_divergence_entry_for_each_merge_when_the_interval_contains_two_merges`)もあり、`merge_commit:`が2件出力されることを確認している。
- `related_events`(項目7)・`Requirement.source`/`related_issues`(項目8)・`ExpectedResult.generated_by`/`verified_by`(項目9)は、いずれも縮小版として合意した設計(priority/status・confidence_score・model・prompt_versionを含めない)通りに実装されている。`generated_by`省略=「不明」という意味論もdocコメント・JSON Schemaの`description`双方に明記されている。
- 論文§8 Conclusionに、今回の一般化より前の制約(「`lineage`の判定結果が主系譜に自動反映されない」「主系譜統合が実装課題として残っている」)を指したままの記述が2箇所残っていた。§3.2・§3.6は更新済みだったが§8がFollow-upの更新対象リストに入っていなかったための漏れ。実態に合わせて修正し、Changelogに追記した(本チェックリスト作成者ではなく第三者レビューでの発見)。
- 副次的に、§3.7の「`changes/<from>-<to>.yaml`」という誤ったファイル名例(§3.5の実際の命名規則`changes/<to_milestone>.yaml`と食い違う、今回の変更以前からの既存の誤り)も発見したため合わせて修正した。
- 軽微な指摘(未修正、対応不要と判断すれば見送りで良い)：`markharness changes annotate`の`--type`が必須引数のため、`related_events`だけを追記したい場合でも`--type`の再指定を強制される。`related_events`は`change_type`と独立した加算的フィールドという設計意図(コード内docコメント)と、CLIの必須引数制約がやや噛み合っていない。

このサンドボックスにRustツールチェインが無く`cargo test`等を再実行できなかったため、上記はコードリーディングによる検証である点に留意。

