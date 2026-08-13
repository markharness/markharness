# 0002: `changes compute`のimpacted_testcasesはhistoricalモードをデフォルトにする

## ステータス

Accepted

## 背景

`markharness changes compute`は`from_milestone`/`to_milestone`という過去の2タグを比較するが、`impacted_testcases`(影響を受けるTestCase集合)は常に**現在の作業ツリー**から生成されていた(`impacted_testcases_by_feature`、`src/changes.rs`)。そのため、同じ過去の区間を後日再計算すると、現在のテスト構造の変化に応じて結果が変わり得た(`docs/テスト知識管理のGit-nativeモデル_評価レビュー.md`のP1指摘)。

これは「過去のある区間で実際に何が影響を受けたか」という問い(履歴再現)と、「今この時点で再確認すべきテストは何か」という問い(現在候補抽出)が、明示的に分離されずに1つの実装に混在していたことが原因である。

## 決定内容

`markharness changes compute`の`impacted_testcases`計算に2つのモードを設け、デフォルトを**historicalモード**(`to_milestone`タグが指すGitツリーから生成)にする。

- **historicalモード(デフォルト)**：`to_milestone`タグのGitツリーから`knowledge/`を一時的な`git worktree`に展開し、そこから`TestCase`を生成する(`historical_testcases_by_feature`)。同じ区間を後日再計算しても常に同じ結果になる。
- **`--current-tree`(オプトイン)**：現在の作業ツリーの`knowledge/`から生成する従来動作(`impacted_testcases_by_feature`)。作業ツリーが変化し続ける限り、同じ区間の再計算結果も変わりうる。

`markharness backfill run`(第4章のバックフィルワーカー)も、過去のマイルストーン区間を再構成する処理であるため、同じくデフォルト(historical)を使うようにした。

## 採用理由(デフォルトをhistoricalにした理由)

- 後方互換(既存の作業ツリー参照動作を維持する)よりも、安全側(同じ問い合わせが常に同じ結果を返す)を優先した。`changes/*.yaml`はマイルストーン境界の**不変の事実記録**として設計されている([change-event-verification-tracking-spec.md](../design/change-event-verification-tracking-spec.md) §2.3)ため、その一部である`impacted_testcases`が再計算のたびに変わりうる実装は、この設計思想と整合しない。
- 「今この時点で再確認すべきテストは何か」という現在候補抽出のユースケース自体は引き続き必要なため、廃止はせず`--current-tree`として明示的なオプトインに変更した。

## 影響・将来の再検討条件

- `markharness backfill run`のデフォルト挙動が変わる(既存動作は`--current-tree`相当だったが、historicalに変更)。バックフィル対象は過去のマイルストーン区間の再構成が目的であるため、この変更はユースケースとより整合する。
- `docs/テスト知識管理のGit-nativeモデル_統合版.md`§3.5、`docs/design/change-event-verification-tracking-spec.md`§2.4に両モードの説明を追記済み。
- 将来、`--current-tree`モードの利用実績が乏しいことが分かった場合は、オプション自体の削除を再検討してよい。逆に、CI等での既定運用として`--current-tree`の方が有用だと分かった場合は、デフォルトの再逆転をこの決定の上書きとして記録すること。
