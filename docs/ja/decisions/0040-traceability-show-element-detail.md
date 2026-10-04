# 0040: `traceability show`で、選んだ1要素の内容を読めるようにする

## ステータス

Accepted(2026-10-05決定)。

## 背景

GUIは、要求を根にした1本の木と、選んだ要素の詳細ペインで表示を構成する。木は軽い`traceability`で作る。詳細ペインには、選んだ要素の内容を出す。

`traceability`の各Nodeは、識別子と`label`しか持たない。軸・説明文・`procedures`・Scenarioの`phases`・生成されたTestCaseの手順は、どの読み取り出力にも含まれない。一方、[設計書](../design/cli-read-model-design.md)§11は、viewがKnowledgeや`.markharness/`を直接読まず、CLIの出力だけを入力とすると定めている。そのためGUIは、詳細ペインの内容を取る手段を持たない(Issue #95)。

設計書§12は、TestCaseの詳細表示専用モデルを「viewで具体的な必要性が確認されてから追加する」ものとし、「生成ファイルを直接読めば当面は代替できる」と書いていた。詳細ペインという具体的な必要が出たので、この記述を見直す必要がある。また、直接読む代替案は、view側が`.markharness/`を直接読まないという境界と矛盾する。

## 決定

### 1. 新コマンド`traceability show`を追加する

```text
markharness traceability show --uid <UID> [--at <git-ref>] [-d, --dir <path>]
```

選んだ1要素の内容を、標準出力へJSONで1件だけ返す。`traceability`は軽いままにし、内容は詳細だけが持つ。`--at`の意味は`traceability`と同じで、省略時は作業ツリー、指定時はそのGit refを読む([0033](./0033-traceability-defaults-to-working-tree.md))。木と詳細で読む時点がずれると、GUIの表示が食い違うためである。

`traceability`に引数なしで`--at`などを渡す従来の使い方は変えない。`show`はそのサブコマンドとして置く。

### 2. 要素ごとの項目

| 要素 | 項目 |
|---|---|
| Requirement | `requirement_id`、`axis`、`description` |
| Feature | `feature_id`、`axis`、`description` |
| Behavior | `behavior_id`、`axis`、`description`、`procedures` |
| Scenario | `scenario_id`、`description`、`phases`(各`steps`の`action`と`use`、`results`)、`implementation_note` |
| TestCase | `case_id`、`case_revision`、`phases`(`use:`を展開した、実際に実行される手順) |

全要素に`schema_version`・`record_kind`・`at`・`kind`・`uid`を付ける。

- `kind`は`requirement`・`feature`・`behavior`・`scenario`・`test_case`のいずれか。呼び出し側は`uid`だけで引けるので、要素の種類を事前に知らなくてよい。
- `record_kind`は1つ(`traceability_detail`)にする。種類ごとの項目の違いは、JSON Schemaの`oneOf`で表す。
- 存在しない項目は、`null`にせずキーごと省略する。`traceability`の`label`の扱いと、設計書§11の「未知の任意フィールドを追加しても読める」方針に揃える。
- `source: external`のRequirementは`description`を出さない。外部文書が内容を持つためで、`label`を出さないのと同じ理由である([0023](./0023-requirement-native-and-external-source.md))。
- Scenarioの`phases`は、Knowledgeに書かれたまま返し、`use`は展開しない。TestCaseの`phases`は展開した結果を返す。前者は編集の現在値、後者は「実際に実行される内容」の確認用で、目的が違うため形も分ける。

### 3. TestCaseは`case_uid`で引き、現在の版を返す

TestCaseの`uid`は、`traceability`の`test_cases[].case_uid`である。`case_revision`は指定させない。GUIが木に使った`--at`と同じ値を渡せば、木と一致する版が返る。

### 4. 存在しない`uid`は失敗にする

`--uid`が、その時点のどの要素にも一致しない場合は、終了コード2で終了し、標準出力には何も出さない。エラー文には、`uid`と読んだ時点(`working-tree`またはGit ref)を含める。GUIは、失敗したら木を再取得すればよい。

### 5. `schema_version`は1とする

[0039](./0039-read-output-schema-version-frozen-in-prototype.md)に従い、0.xの間は上げない。

### 6. 範囲

次の項目は、表示に使わないので、いまは含めない。編集の範囲が決まってから、必要なら別に決める。

- Requirementの`source_revision`・`related_issues`
- Featureの`forked_from`

設計書§12の「TestCaseの詳細表示専用モデル」は、この決定で追加済みの扱いにする。

## 理由

- 詳細ペインという具体的な必要が、設計書§12の追加条件を満たす。
- viewが`.markharness/`を直接読まない境界を保つなら、内容はCLIから出すしかない。
- 内容を木に含めると、全件の本文を毎回読むことになり、木が重くなる。選んだ1件だけを返せば、木は軽いままである。
- `use:`の展開結果は生成ロジックの出力である。GUIが自前で再現すると、生成ロジックと食い違いうる。

## 検討した代替案

- **`traceability`に`--detail`のような引数を足し、全件の内容を一括で出す。** 木が重くなり、「木は軽く、詳細は1件ぶんだけ」という前提と食い違う。採用しない。
- **種類ごとに`record_kind`を分ける(5種類)。** 呼び出し側が事前に要素の種類を知っている必要があり、`uid`だけでは引けない。採用しない。
- **TestCaseは`case_uid`と`case_revision`の両方で指定させる。** GUIが木と同じ`--at`を渡せば版は一致するので、版の指定は要らない(YAGNI)。採用しない。
- **GUIが生成ファイルを直接読む例外を、設計書で認める。** viewの境界が`traceability`の一部だけ崩れ、`use:`の展開の再現も要る。採用しない。
- **存在しない`uid`を、`found: false`を持つ正常なJSONで返す。** 成功と失敗の区別が出力の中に移り、呼び出し側が毎回その項目を見る必要がある。`impact`・`coverage`の失敗の扱いとも揃わない。採用しない。

## 帰結

- `docs/*/design/cli-read-model-design.md`に§5.5を追加し、§9・§12・§14.1を更新する。CLIマニュアルにも追加する。
- JSON Schema(`schema/traceability-detail-read-model.schema.json`)と、種類ごとのfixture(`tests/fixtures/read-models/traceability_detail/v1/`)を追加する。
- `knowledge reconcile`とIntentの形式は変えない。詳細の出力は、GUIが編集Intentを作るときの現在値を渡すだけである。
