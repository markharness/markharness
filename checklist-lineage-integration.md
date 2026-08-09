# Task: lineage判定結果をchanges computeの主系譜に統合する

Created: 2026-08-10

背景: `src/changes.rs`の`compute_changes`(マイルストーン境界の線形比較)と`src/lineage.rs`の`compute_lineage`(`git merge-base`による2親分岐判定、監査用の独立コマンド)が連携しておらず、`changes compute`を実行してもtrue divergence(真の分岐)がChangeEventのderived_from相当のフィールドに記録されない(improvement-prompts.md項目2)。

ユーザー確認済みの方針: `ChangeEvent`に新規フィールド`from_tree_shas: Vec<String>`(`#[serde(default)]`)を追加する加算的な変更とする。真の分岐が検出された場合のみ`[tree(P1), tree(P2)]`を格納し、それ以外は空のまま(既存の`from_tree_sha`単一値はそのまま残し後方互換を保つ)。

統合範囲: `to_milestone`のタグが指すコミット自体が2親のマージコミットである場合にのみ、そのFeatureについて`git merge-base(P1, P2)`を基準にlineage分類を行い、`TrueDivergence`のFeatureだけ`from_tree_shas`を埋める。`to_milestone`とマージコミットの間に他のコミットが挟まる一般ケースは対象外(既知の限界としてThreats to Validityに明記する)。

## Steps
- [x] Step 1: 現状の挙動を再現する失敗するテストを追加する(2親分岐があるケースで`changes compute`を実行しても`from_tree_shas`が空のまま、またはフィールド自体が存在しないことを示す)
- [x] Step 2: `src/lineage.rs`の`classify`関数を`pub(crate)`にして`changes.rs`から再利用可能にする
- [x] Step 3: `ChangeEvent`に`from_tree_shas: Vec<String>`(`#[serde(default)]`)を追加し、`compute_changes`内で`to_milestone`が2親マージコミットの場合のみ、対象Featureのlineageを分類し`TrueDivergence`なら`[p1_tree_sha, p2_tree_sha]`を設定する
- [x] Step 4: 既存の全テスト(シリアライズ・annotate等)を通す。新規のCLI統合テストをtests/changes_cli.rsに追加する
- [x] Step 5: `cargo test`(227ユニットテスト+統合テスト全件)/ `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check`を通す
- [x] Step 6: 論文§3.2・§3.6・第6章Threats to Validity・第7章Future Workの「非連携」記述を、今回の統合範囲(to_milestoneが直接マージコミットの場合のみ)に合わせて更新する
- [x] Step 7: checklist-paper-gaps.mdの既存Note(lineage非連携)を解消済みとして追記する

## Notes
- 実装: `src/changes.rs`に`true_divergence_parent_tree_shas`関数を追加。`git::parents(to_milestone)`の結果が2親の場合のみ`git::merge_base`とlineage分類を行い、`TrueDivergence`のFeatureだけ`from_tree_shas`を埋める。2親でない場合(通常のケース)は空のBTreeMapを返し、既存の`compute_changes`の挙動に一切影響しない。
- `ChangeEvent`への新規フィールド追加は加算的な変更(`#[serde(default)]`)であり、既存の`changes/*.yaml`ファイルはマイグレーション不要でそのまま読み込める。
- スコープの限定: `to_milestone`タグが直接マージコミットを指す場合のみ検出する。マイルストーン区間の途中でマージが発生し、その後さらにコミットが積まれてからタグが打たれるような一般的なケースは対象外。これは論文側にも明記した(§3.2・§3.6・第6章・第7章)。

## Summary
`ChangeEvent`に`from_tree_shas: Vec<String>`フィールドを追加し、`to_milestone`タグが直接2親のマージコミットを指す場合に限り、`lineage::classify`のロジックを`changes compute`内部から呼び出して`TrueDivergence`と判定されたFeatureの両親tree SHAを記録するようにした。既存スキーマとの後方互換を保つ加算的な変更としてTDDで実装し、cargo test/clippy/fmtを全て通した。論文の該当箇所(§3.2・§3.6・第6章・第7章)も統合範囲の限界を含めて更新した。

## Steps (2026-08-10 追記: unwrapパニックのバグ修正)
- [x] Step 8: ユーザーからの指摘(`true_divergence_parent_tree_shas`内で`p1_sha.unwrap()`/`p2_sha.unwrap()`を呼んでおり、`lineage::classify`が「一方のブランチでFeatureを削除、もう一方で変更」というケースでも`TrueDivergence`を返しうるため、`p1_sha`が`None`の場合にパニックする)を受けて回帰テストを追加した(`does_not_panic_when_true_divergence_involves_a_feature_deleted_on_one_branch`)。modify/delete衝突を`git merge`で発生させ、featureブランチ側の変更を残す形で手動解決してマージコミットを作るテストケース。
- [x] Step 9: テストがパニックで失敗すること(Red)を確認した上で、`p1_sha`/`p2_sha`が両方`Some`の場合のみ`from_tree_shas`を記録するよう修正した(片方が`None`の場合は空のまま=通常の`from_tree_sha`/`to_tree_sha`表現にフォールバック)。
- [x] Step 10: `cargo test`(230件)/ `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check`を再度通した。

## Notes(バグ修正、2026-08-10)
- 根本原因: `lineage::classify(base, p1, p2)`は`p1 == p2`でもなく`p1 == base`でも`p2 == base`でもない場合に`TrueDivergence`を返す。`p1=None`・`base=Some`・`p2=Some(x≠base)`のとき、`None == Some(x)`は偽、`None == base`も偽、`p2 == base`も偽なので`TrueDivergence`になる。「両方の親が存在し値が異なる」という前提は成り立たない。
- 修正方針は「削除を伴う真の分岐は`from_tree_shas`(2親情報)としては記録せず、既存の`from_tree_sha`(単一値・線形表現)にフォールバックする」を選んだ。2親のうち一方が存在しない場合、`[tree(P1), tree(P2)]`という配列自体が意味を持たないため。
