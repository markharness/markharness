# Task: canonicalization_rule_version / id_index_schema_version の改訂シナリオ検証

Created: 2026-08-10

背景: `src/id_cache.rs`の`CANONICALIZATION_RULE_VERSION`/`ID_INDEX_SCHEMA_VERSION`は"1"固定のまま一度も改訂されておらず、バージョン変更時にキャッシュが正しく破棄・再計算される挙動が(`tool_version`経由でしか)明示的に検証されていなかった(improvement-prompts.md項目5)。

既存の`stale_cache_key_is_silently_recomputed_instead_of_trusted`テストは`tool_version`のみを不一致にして検証しており、コメントで「4つのキー構成要素すべての代表として」としているが、`canonicalization_rule_version`・`id_index_schema_version`個別の不一致は直接テストされていなかった。

## Steps
- [x] Step 1: `canonicalization_rule_version`が不一致の場合に既存キャッシュが破棄・再計算されることを検証するテストを追加する(`stale_canonicalization_rule_version_is_silently_recomputed_instead_of_trusted`)
- [x] Step 2: `id_index_schema_version`が不一致の場合に既存キャッシュが破棄・再計算されることを検証するテストを追加する(`stale_id_index_schema_version_is_silently_recomputed_instead_of_trusted`)
- [x] Step 3: バージョン不一致時の挙動(再計算)が設計意図(§3.3: 読み込み時に不一致なら静かに再計算)と一致しているか確認する。一致しているため実装変更は不要と結論した
- [x] Step 4: `cargo test`(229ユニットテスト全件)/ `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check`を通す
- [x] Step 5: 検証結果をchecklist-paper-gaps.mdに記録する

## Notes
- `resolve_feature_versions`のキャッシュ有効性判定は`CacheKey`構造体全体の等値比較(`&cache_file.key == current_key`)であり、`tree_sha`/`canonicalization_rule_version`/`id_index_schema_version`/`tool_version`のいずれか1つでも不一致ならキャッシュ全体を破棄して再計算する設計になっている。したがって「`tool_version`だけ実装されていて他の2フィールドは未検証」という状態ではなく、実装は既に4フィールド全てに同一のロジックで対応していた。今回追加したテストは、既存の`stale_cache_key_is_silently_recomputed_instead_of_trusted`(tool_versionのみ検証、コメント上は「4つの代表」)が実際に他の2フィールドでも同様に機能することを明示的に固定するテストであり、実装修正は不要だった。
- `canonicalization_rule_version`/`id_index_schema_version`の値自体("1"固定)を実際に改訂する運用(正規化ルールやid-indexフォーマットの実際の変更)は、本テストでは検証していない(意図的に不一致の値を注入したのみ)。実際の改訂運用そのものの検証はFuture Workのまま(論文§3.3・第7章に既存の記述通り)。

## Summary
`canonicalization_rule_version`・`id_index_schema_version`それぞれについて、キャッシュキー不一致時に既存キャッシュ(意図的に汚染したダミーエントリ)が信頼されず再計算される、という設計意図通りの挙動をTDDで固定するテストを追加した。実装は既に汎用的な等値比較で4フィールド全てに対応しており、コードの修正は不要だった(検証のみで完了)。
