# 0037: 近縁ツールSphinx-Needsとの位置づけと、現時点では連携しない方針

## ステータス

Accepted(2026-09-30決定)。実装は伴わない。関連研究の位置づけと、連携しない理由、連携する場合に何が必要かを記録する。再検討の条件は末尾に示す。

## 背景

Sphinx-Needsは、markharnessと思想がよく似たツールとして名前が挙がる。似ている点と違う点を毎回ゼロから導き直さずに済むよう、比較と判断をここに残す。

Sphinx-Needsの記述は公式ドキュメント(sphinx-needs.readthedocs.io)を読んで確認したものである。ソースコードは読んでおらず、ドキュメントに載っていない挙動は未確認として扱う。

## 似ている点

- **Docs-as-Code。** 要求やテスト知識をテキストで書き、Gitで管理する。専用のDBやGUIを正本にしない。
- **ID付きの要素とリンク。** Sphinx-Needsのneed(型・ID・リンク・拡張フィールド)は、markharnessのRequirement / Feature / Behavior / Scenarioに相当する。
- **トレーサビリティと検証カバレッジの可視化。** Sphinx-Needsはフィルタ・表・フロー図で、markharnessは`traceability`とRelease Coverageで示す。
- **整合性の機械検査。** Sphinx-Needsは警告とスキーマ検証で、markharnessは`validate`で検出する。

## 違う点

| 観点 | Sphinx-Needs | markharness |
|---|---|---|
| 主な対象 | 文書に埋め込んだ要求・仕様・テストの関係 | テスト知識(Feature / Behavior / Scenario)と、その変更影響 |
| 正本の取得 | Sphinxのbuildで文書からneedを収集し、グラフを構築する | Git上のYAMLを直接読む(外部要求はStrictDocのJSONエクスポートを経由する。0036) |
| 時間軸 | 現在の文書のスナップショットが基本。`needs.json`に版ごとのneedを並べて保持でき、`needimport`の`:version:`で版を選べる | マイルストーンタグ間のGit tree SHAを比較して`ChangeEvent`を自動導出する |
| テストの扱い | needの一種として書く。実行結果は別の拡張で取り込む | Scenarioから`TestCase`を決定的に生成し、`binding`で検証手段を宣言する |
| 検証ルール | `needs_schema_definitions`などによる宣言的なスキーマ検証 | `validate`に組み込まれた検査 |
| 外部の要求 | `needs_external_needs`で他プロジェクトの`needs.json`を参照する | `source: external`でStrictDocの要求を固定参照する(0023) |

要点は次の2つである。Sphinx-Needsは「今の関係を描く」ことに強く、markharnessは「版の間で何が変わり、何を再確認すべきか」に答える。この差が補完関係を成立させうる場所であり、同時に両者が競合しない理由でもある。

## 決定

**現時点ではSphinx-Needsとの連携を実装しない。** `source: external`は引き続きStrictDocだけを指す(0030 §1)。理由は次のとおり。

1. **想定ユーザーと重ならない。** markharnessは開発者と、開発に近いQAチームを想定する。Sphinx-Needsが強い文書中心・規制産業の組織は視野に入れていない。
2. **需要の実証がない。** Sphinx-Needsを使う利用者からの要望も、自分たちで使う予定もない。YAGNIにより、実際に次の要求が来た時点でその要求に合わせて拡張する。
3. **外部正本を2種類持つ保守コストが大きい。** `source_key`の意味、`source_locator`の粒度、stale pin、`validate`・traceabilityの分岐が、外部正本の種類ごとに増える。
4. **連携に必要な情報の一部が、公開ドキュメントでは確認できない。** 下記「連携する場合に必要になること」を参照。

なお、`needs.json`のようなビルド成果物に依存すること自体は、連携しない理由にならない。`knowledge intent-from-strictdoc`(0036)も、利用者が`strictdoc export`で出力したJSONを入力にしており、同じ構造だからである。当初この点を「Git-nativeと緊張する」と整理したが、0036の設計と矛盾するため、理由から外した。

### StrictDocを選んだ理由との関係

StrictDocを外部正本に選んだ理由は、要求をGit上のテキスト(`.sdoc`、Markdown)として直接管理でき、Git-nativeな設計と合うためである。StrictDocの公式FAQは、StrictDocをDoorstopの「close successor」と位置づけており、最初のバージョンはDoorstopのフォークとして始まり、その後ゼロから作り直したと説明している。両者はコードを共有しないが、設計原則(要求をテキストで管理し、コードと同じ場所に置く)は共通する。Doorstopは要求ごとに1つのYAMLファイルを持ち、StrictDocは1文書を1つの`.sdoc`ファイルとして複数の要求を含める点が違う。この選定理由は、Sphinx-Needsのbuild中心のモデルには当てはまらない。

## Sphinx-Needsの優れた解決方法

Sphinx-Needsの仕組みのうち、markharnessに取り入れる価値があるかを検討した結果を示す。

| Sphinx-Needsの仕組み | markharnessでの扱い |
|---|---|
| **宣言的なスキーマ検証。** 型・パターン・列挙値に加え、リンク数の下限・上限(`minContains` / `maxContains`)、最大4段のリンクをたどる検査、条件付きルールを、JSON Schemaで宣言する | **新しい発想であり、価値がある。** markharnessは、利用者が検査ルールを宣言的に追加する仕組みを持たない。ただし、必要になる具体的な検査が現れていないため、今は導入しない。「Requirementは1件以上のFeatureに関連づく」のような検査が利用者ごとに変わる状況が生じた時に検討する |
| **`id_prefix`による外部needのID衝突回避** | 同じ解決を既に採っている。`intent-from-strictdoc`は`id`に`sd-`接頭辞を付ける(0036 §3) |
| **`needextend`。** 元のneedを編集せず、別の場所から値を上書き・追記・削除する | 同じ発想を既に採っている。externalのRequirementは`label`/`description`を持てないが、`axis`だけはmarkharness側に保持できる(0023 §5) |
| **`needs_reproducible_json`。** タイムスタンプを除いて出力を再現可能にする | 同じ発想を既に採っている。Intentの`id`は入力のMIDだけから決まり、実行順序に依存しない(0036 不変条件) |
| **`needs.json`に複数の版を並べて保持する** | 採用しない。版の履歴はGitが既に持っており、markharnessはタグ間のtree SHA比較で導出する。これが差別化点そのものである |

以上のうち、取り入れる余地があるのは宣言的なスキーマ検証だけである。他は、既に同じ解決を採っているか、Gitがその役割を担っている。

## 連携する場合に必要になること

将来、連携が必要になった場合の論点を先に記録する。

1. **入力形式。** `needs.json`(`needs`ビルダーの出力)を入力にするのが、0036と同じ構造で自然である。markharnessはSphinxを実行しない(0036 §2と同じ理由)。
2. **ソースの位置。** 0036 §4はStrictDocのJSONにソースファイルのパスがないため、MIDの索引で`source_locator`を決めた。`needs.json`の公開ドキュメントには、`docname`・`lineno`などの位置情報の記載がなく、存在するかは未確認である。連携する時は、実物の出力で確かめる。
3. **変更検出の粒度。** 0023の変更検出は`source_locator`が指すファイルのblob差分である。Sphinx-Needsは1つの`.rst`やMarkdownに多数のneedを書くため、ファイル単位では粗い。need単位の識別(`source_key`に相当するもの)を、blob差分とは別に持つ必要がある。
4. **IDの安定性。** `needs_id_required`が既定で`False`であり、IDは自動生成されうる。自動生成IDが編集の前後で安定するかは、ドキュメントでは確認できなかった。0036 §6が「MIDがソースに永続化されていること」を前提にしたのと同様に、明示的なIDを必須にする(`needs_id_required = True`)前提を置くことになる。
5. **命名。** 2つ目の入力形式が来た時点で、`intent-from-strictdoc`の名前と、`source: external`が常にStrictDocを指す前提(0030 §1)を見直す。0036は、汎用化を「2つ目の形式が来た時点」に回している。

## 検討したが採用しない選択肢

- **今すぐSphinx-Needs連携を実装する。** 需要の実証がなく、上記の未確認事項が多いため採用しない。
- **`.rst`やMarkdown(MyST)を直接パースしてneedを読む。** Sphinx-Needsの構文は拡張やbuild設定に依存する。0036が自前の`.sdoc`パーサを採らなかったのと同じ理由で採用しない。
- **`needs.json`の版履歴機能で、markharnessの`ChangeEvent`を置き換える。** 版の履歴はGitが持っており、build成果物に重複して持たせる理由がない。

## 将来再検討するトリガー

次のいずれかが成立した時点で、連携しない本決定を再検討する。

- 開発者やQAチームの利用者から、Sphinx-Needsで管理する要求をexternal Requirementとして参照したいという要望が、実際に出た場合。
- 利用者ごとに異なる検査ルールを宣言的に追加したいという要望が、実際に出た場合(スキーマ検証の導入検討)。

## 出典

- Sphinx-Needs: https://sphinx-needs.readthedocs.io/en/latest/(概要、Builders、Configuration、Schema validation、`needextend`、`needimport`)。2026-09-30に確認。
- StrictDoc F.A.Q.: https://strictdoc.readthedocs.io/en/latest/latest/docs/strictdoc_03_faq.html(Doorstopとの関係)。2026-09-30に確認。
