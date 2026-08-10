# 改善提案レビュー：データモデル分析レポート(5.1〜5.5)の妥当性検証

**位置づけ**：本資料は本リポジトリ外で作成された「markharness データモデル・YAML実装 分析レポート」(以下「元レポート」)第5章の改善案5件を、実装(`src/`・`schema/`)および[テスト知識管理のGit-nativeモデル_統合版.md](./テスト知識管理のGit-nativeモデル_統合版.md)(以下「論文」)と照合し、妥当性を評価したものである。元レポート自体はリポジトリに含まれないため、ここには評価の結論と根拠のみを記録する。

**評価方針**：`CLAUDE.md`の設計ルール(後方互換性を前提にしない、工数はデメリットに含めない、有用性/無用性で判断する)に従い、実装コストではなく設計としての整合性・既存の設計判断との矛盾の有無を基準に評価した。

---

## 結論サマリー

| 案 | 妥当性 | 理由(要約) |
|---|---|---|
| 5.1 test_variations(Conditionの多重化) | 却下推奨 | `generate.rs`/`testcase-generation-design.md`§3.2が「ExpectedResultごとのTestCase分割」を意図的に撤回した経緯と正面衝突する |
| 5.2 Axis階層化 | 要再設計 | 問題認識は妥当だが`children`フィールドは不要な冗長設計。命名規約(`ui.input`)だけで同じ効果が出せる |
| 5.3 複合ChangeEvent | 却下推奨(代替案あり) | ChangeEventはFeature単位の自動計算が核心的価値。人手グルーピングを混ぜると決定性が崩れる |
| 5.4 Requirement属性拡張(priority/status/source/related_issues) | 大部分不要 | `priority`/`status`は消費者(検証・生成ロジック)が存在せず死んだメタデータ化する。`related_issues`のみ検討価値あり |
| 5.5 AI生成メタデータ(generated_by/verified_by) | 縮小して採用 | 論文付録A.1のLLM路線却下と方向性は矛盾しないが、`confidence_score`等は「検証済み知識」というスキーマの前提と相性が悪い |

---

## 5.1 test_variations（却下推奨）

`docs/testcase-generation-design.md`§3.2・§6によれば、実装は当初案の「ExpectedResultごとにTestCaseを分割する」設計を**意図的に撤回**し、「1 Condition = 1 TestCase、全ExpectedResultを1件に集約」に変更している(理由：決定性の証明・実装の単純化、同資料§4)。`src/generate.rs`の`case_id: format!("tc-{}-001", condition.id)`は、この設計が実装として固定されていることを裏付ける(連番`001`は将来拡張の予約桁であり、現状増える余地がない)。

`test_variations`はこれと同じ「1つの単位から複数TestCaseを生成する」設計を、ExpectedResultではなくCondition側で再導入する提案であり、撤回済みの設計に逆戻りする。加えて、`src/changes.rs::impacted_testcases_by_feature`は`case_id`を素朴なキーとして扱っており、連番が実際に動き出すと「マイルストーンをまたいで同じConditionから生成されたTestCase集合の何が同一で何が新規か」を追跡する仕組みが別途必要になる。

元レポート自身が書いている通り、現状のワークアラウンド(Conditionを分割する)で技術的には代替可能であり、CTM的な「分類軸の組み合わせ」を輸入する動機は理解できるが、`testcase-generation-design.md`§6はこのツールが意図的にテスト設計技法(組み合わせテスト生成等)には踏み込まない立場を明言している。必要性が実証されるまでは見送るのが妥当。

## 5.2 Axis階層化（要再設計）

`src/axes.rs`・`src/validate.rs::check_axis_tags`によれば、Axisは`id`/`label`のみの平坦なレジストリで(`schema/axis.schema.json`も同様、`additionalProperties: false`)、タグの検証は「`axes/*.yml`に存在するか」のみで、文字列の中身(ドット区切りかどうか)には一切意味を持たせていない。

提案の`children:`フィールドは、親エントリに子一覧を持たせる方式だが、これは「親と子で二重に関係を記録する」設計であり、Axisレジストリ自体が抱える「値が実運用に追随できない」課題(元レポート4.1節)を、別の同期対象として増やしているだけである。既存のシンプルな`id`+`label`構造を維持したまま、`id: ui.input`のようにドット区切りIDを許容するだけで、同じ階層グルーピング(prefix match)は実現できる。`children`フィールドは不要な冗長性を生むため採用しない方がよい。

なお、階層化の主な効用として挙げられている「横断ビュー生成時の集約」機能自体が、現状のCLIには存在しない(Web UIはFuture Work、論文§7)。消費者の存在しない構造を先に作る優先度は低い。

## 5.3 複合ChangeEvent（却下推奨、代替案あり）

`src/changes.rs::compute_changes`によれば、ChangeEventは「Featureごとにマイルストーン境界のtree SHA差分から自動計算」される。人間が手を加えるのは`change_type`のアノテーションのみ(`markharness changes annotate`、論文§3.5)で、どのFeatureが変更されたかの判定自体は自動・決定的である。この自動性が論文§1.3の核心的貢献にあたる。

提案の複合ChangeEvent(`related_features`配列)を成立させるには、「複数Featureの変更が同じ論理変更である」という判断を人間が行う工程が必要になるが、これは現行のCLIワークフローに存在しない。また「同じマイルストーン区間で変わった」という条件だけでは無関係な同時変更まで一緒くたにしてしまい、論文がブランチ戦略非依存を掲げていること(論文§3.4)とも相性が悪い。

代替案として、ChangeEventの原子性(Feature単位・自動計算)は維持したまま、`change_type`アノテーション時に任意で`related_events: [event_id, ...]`を人間が追記できるようにする方が、既存の「自動生成＋人手注釈」パターン(論文§3.5・§3.7の設計思想)と整合する。

## 5.4 Requirement属性拡張（大部分不要）

`PROJECT.md`が明記する通り、本ツールの差別化点は`derived_from`(版履歴)と`ChangeEvent`の影響伝播であり、要件管理(優先度・ステータス管理)はスコープに含まれない。`priority`/`status`を追加しても、それを読む検証・生成ロジック・CLIコマンドが一つも存在せず、Axisレジストリと同様に実運用から乖離して腐るリスクが高い。外部issueトラッカー側のステータスと二重管理になれば、元レポート4.1節が指摘する「Axisの値が実運用に追随できない」問題を別フィールドで再生産することになる。

`related_issues`のみ例外的に検討価値がある。元レポート§6.1が指摘する「Bidirectionality(逆参照)の欠如」は実装上も事実であり(`Feature`から外部要件への参照フィールドが存在しない)、これは実装ロジックに影響を与えない純粋な参照情報のため、腐るリスクが相対的に低い。追加するなら`priority`/`status`は削り、`source`/`related_issues`のみに絞るのが妥当。

## 5.5 AI生成メタデータ（縮小して採用）

前提として訂正が必要な点がある。元レポート1.2節のExpectedResult例には`note`/`added_in_milestone`が挙げられているが、`schema/expected_result.schema.json`は`additionalProperties: false`で`id`/`condition`/`description`の3つしか許可しておらず、**この2フィールドは現状未実装**である。つまり5.5はここにさらに`generated_by`/`verified_by`を積む提案であり、実際のスキーマ変更量は元レポートの想定より大きい。

論文付録A.1は「LLM専用知識グラフへの全面ピボット」を新規性・評価可能性の観点で却下しているが、5.5はそれとは別の話(生成物への出所タグ付け)であり、直接矛盾はしない。ただしフィールドごとに温度差がある。

- `generated_by.method`(manual/llm/auto-combination)のような**離散的な事実情報**は追加コストが低く、「LLM生成テストだけレビュー必須にする」といったCIゲートへの応用が現実的に見込める。
- `confidence_score`(0-1の連続値)は問題が大きい。本スキーマ群は`additionalProperties: false`＋JSON Schemaでの厳密検証を徹底しており、「`knowledge/`配下にあるものは検証済みの確定知識である」という前提で`generate`/`verify`が動作する。主観的な信頼度スコアを混ぜると、「このYAMLは確定知識か暫定か」の境界が曖昧になり、既存の決定性重視の設計思想と衝突する。要るなら`verified_by.human_review`のような真偽値のゲートに留め、`prompt_version`のような揮発性の高い生成ツール側のテレメトリはversioned knowledgeとは別のログに出す方が設計として一貫する。

---

## 論文(`テスト知識管理のGit-nativeモデル_統合版.md`)との照合結果

本レビューにあたり、上記5案および元レポート本文全体を論文と突き合わせたが、**論文側に修正が必要な誤りは見つからなかった**。論文は§3.6実装状況表・各節末尾の「実装状況」注記により実装との相違を継続的に自己反映しており、今回照合した範囲(Requirement/Feature/Behavior/Condition/Axis各schemaのフィールド構成、ExpectedResultの1 Condition = 1 TestCase集約仕様、ChangeEventのFeature単位自動生成、`changes/`のファイル命名規則)はすべて実装(`src/`・`schema/`)と一致していた。

誤りが含まれていたのは論文ではなく元レポート側であり、参考として以下に記録する。

| 元レポートの記載 | 実装での実際 | 根拠 |
|---|---|---|
| ExpectedResultに`note`/`added_in_milestone`フィールドがある | `id`/`condition`/`description`のみ(`additionalProperties: false`) | `schema/expected_result.schema.json` |
| `changes/<from_milestone>-<to_milestone>.yaml`という命名 | `changes/<to_milestone>.yaml`(1マイルストーン区間=1ファイル、from-toのペア表記ではない) | `src/changes.rs`(`changes_dir.join(format!("{to}.yaml"))`)、論文§3.5のディレクトリ構造例 |
| ChangeEventのFeature参照フィールドを`feature`と表記 | 実装のフィールド名は`feature_id` | `src/changes.rs`の`ChangeEvent`構造体 |
| `axes/ui.yml`の例に`description`フィールドがある | 実装のAxisは`id`/`label`のみ | `schema/axis.schema.json` |

したがって、本レビューを理由とする論文の修正は行っていない。
