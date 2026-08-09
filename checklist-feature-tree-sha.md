# Task: Feature変更検知をfeature.yml単体blobからディレクトリ全体のtree SHAに変更
Created: 2026-08-09

背景：`changes compute`はFeatureの変更を`feature.yml`ファイル自身のblob SHAでしか検知しておらず、
Condition/Behavior/ExpectedResultの追加・変更（feature.yml自体は不変）を見逃す。
Featureディレクトリ全体のtree SHAで比較するよう変更する。命名も実態に合わせて刷新する
（blob_sha→tree_sha、FeatureBlob→FeatureVersion等）。

合意事項（/grillingセッションより）：
- 命名は実態に合わせて刷新する（blob→tree系の呼称に統一）
- キャッシュ無効化はcache rebuild/--no-cacheへの手動委任のみ（スキーマバージョニングは別タスク）
- `ChangeEvent連動_実行状態追跡仕様.md`§7の誤記述も同タスクで修正する
- git呼び出しは`ls-tree -r -t`1回のまま（Feature単位の個別プロセス起動はしない）
- `.markharness-cache/`は非コミットのため後方互換問題なし。このリポジトリに実データのchanges/executionsも存在しないためバックフィル不要

## Steps
- [x] Step 1: `src/git.rs`の`ls_tree_recursive`を拡張し、`-t`フラグでtreeエントリも含めて返せるようにする（`ObjectKind`追加、`blob_sha`→`sha`）
- [x] Step 2: `src/id_cache.rs`の`FeatureBlob`→`FeatureVersion`、`resolve_feature_blobs`→`resolve_feature_versions`に変更し、feature.yml単体のblobではなくFeatureディレクトリのtree SHAを返すようにする。Condition追加だけでtree SHAが変わることを検証する新規テストを追加する
- [x] Step 3: `src/changes.rs`を`resolve_feature_versions`/`tree_sha`に追従させ、`ChangeEvent.from_blob`/`to_blob`を`from_tree_sha`/`to_tree_sha`にリネームする
- [x] Step 4: `src/execution.rs`を追従させ、`ExecutionEntry.verified_feature_blobs`を`verified_feature_tree_shas`にリネームする
- [x] Step 5: `src/verify.rs`（trace/pending）を追従させる
- [x] Step 6: `src/cli.rs`・`tests/*.rs`のリネーム箇所を追従させる
- [x] Step 7: `docs/ChangeEvent連動_実行状態追跡仕様.md`のフィールド名・§7の記述を修正する（`設計書との相違点_調査資料.md`にも追記）
- [x] Step 8: `cargo clippy --all-targets -- -D warnings` / `cargo fmt` / `cargo audit` / 全テストを通す

## Notes
- リネームの過程で`src/git.rs`・`src/changes.rs`の一部テスト名に残っていた`blob`表記も刷新した（`blob_sha_changes_when_file_content_changes_across_tags`→`blob_entry_sha_changes_when_file_content_changes_across_tags`等）。

## Summary

`id_cache::resolve_feature_blobs`（feature.yml単体のblob SHA比較）を`resolve_feature_versions`（Featureディレクトリ全体のtree SHA比較）に置き換え、Condition・Behavior・ExpectedResultの追加/変更がfeature.yml自体を変更しなくても検知されるようにした。命名も実態に合わせて刷新（`FeatureBlob`→`FeatureVersion`、`blob_sha`→`tree_sha`、`ChangeEvent.from_blob/to_blob`→`from_tree_sha/to_tree_sha`、`ExecutionEntry.verified_feature_blobs`→`verified_feature_tree_shas`）。git呼び出しは`ls-tree -r -t`1回のまま（`ObjectKind`でtree/blobを判別）。`ChangeEvent連動_実行状態追跡仕様.md`§7の誤った記述（tree SHA比較を前提にしていたが実装は追いついていなかった）も修正した。テストは191件のユニットテスト＋19件のCLI統合テストが全パス、clippy/fmt/audit済み。
