# コードからテスト知識を導出する手順書(AI 実行用)

> **この1ファイルで完結します。** 他のファイルを参照する必要はありません。外部依存は `markharness` CLI(v0.5.0 で検証)のみです。
> **対象読者: AI コーディングアシスタント。** このファイルをそのままチャットに貼るか、プロジェクトに置いて参照させてください。
> ユーザーがチャットで使っている言語で応答してください。

既存のソースコードを読み、`markharness` のテスト知識(`.markharness/knowledge/**`)を導出し、`markharness generate` で TestCase を生成します。

**この作業の3原則(全 Phase を貫く制約):**

1. **創作禁止。** 導出したすべての Scenario は、コード中の具体的な箇所に遡れること。コードが示していない振る舞いを書かない。曖昧な箇所(`TODO`、複数解釈できる分岐)は推測せず、処理を止めてユーザーに確認する。
2. **人間が実施できる操作として書く。** markharness に自動実行エンジンはない。`steps` は人間の Test Executor が上から読んで手作業で実施する手順書である。「`addTodo()` を呼ぶ」ではなく「入力欄に『牛乳を買う』と入力し、『追加』ボタンをクリックする」と書く。
3. **これはレビューの代替ではない。** 成果物は人間がレビューする前提の下書きである。

---

## 1. クイックスタート(コマンドの全系列)

新規プロジェクトで最初から最後まで通す場合の全コマンドです。**この順序に必ず従ってください。**

```bash
markharness --version                                  # 0.5.0 相当を確認。無ければ §2.1 へ
markharness init --dir .                               # .markharness/ が無い場合のみ
markharness axes add functional --label 機能            # 使う axis を *すべて* 先に登録(§4.5)
printf '<req-id>\n<axis-id>\n' | markharness knowledge add   # 新規 Requirement 作成(落とし穴②)
markharness identity migrate --dir .                   # ← 1回目。Requirement に uid を発行
# ... ドラフト YAML を書く(§3)...
markharness knowledge validate --batch drafts/ --json  # 全件検証。エラーが0になるまで繰り返す
markharness knowledge apply    --batch drafts/ --json  # 一括適用(途中失敗なら全ロールバック)
markharness identity migrate --dir .                   # ← 2回目。**これを忘れると次の validate が必ず失敗する**
markharness generate                                   # → "generated N testcase(s) into ..."
markharness validate                                   # → ".markharness/knowledge/ and .markharness/axes/ are valid"
markharness generate                                   # もう一度。N が同じで差分が出ないことを確認
```

### 落とし穴 ①: `identity migrate` は2回必要

`knowledge apply` が作成した Feature / Behavior / Scenario にも uid は付きません。apply 後に migrate を実行しないと `markharness validate` が次のエラーで落ちます。

```
project is in UID mode ([identity] mode = "uid") but this feature '<id>' has no uid;
run `markharness identity migrate` to repair
```

migrate は冪等です。迷ったら実行してください。害はありません。

### 落とし穴 ②: `markharness knowledge add` は対話専用で、非対話フラグが無い

`--dir` と `--edit`(`$EDITOR` を開く)しかありません。**stdin が EOF になると「入力が空です。」を無限に出力し続け、終了しません。** AI から実行する場合は必ず stdin にパイプしてください。

プロンプトは**2つだけ**、この順です:

1. `Requirement name (e.g. task-management):`
2. `Requirement axis (comma separated, e.g. ui, validation):`

```bash
printf 'todo-app\nfunctional\n' | markharness knowledge add --dir .
```

成功すると `Requirement '<id>' を作成しました。` と出て**その時点で対話は自動終了します**(Feature 以降は尋ねられません)。これは正常な挙動であり、失敗ではありません。

### 落とし穴 ③: Requirement の `label` は id に固定される

`knowledge add` は label を尋ねないため、`label: <入力した id>` が確定します。後からドラフト側に別の label(例: `label: TODO アプリ`)を書くと `conflicting_existing_value` で弾かれます。**id を決める時点で、それが表示名になることを前提に命名してください。**

---

## 2. 準備とスコープ確認

### 2.1 ツールの確認

1. `markharness --version` を実行する。コマンドが見つからない場合は**処理を止め**、ユーザーに次を伝える: `markharness` のビルド/インストールが必要(そのリポジトリで `cargo install --path .`、またはビルド済みバイナリ)。出力を捏造したり、確認なしに先へ進んだりしない。
2. `.markharness/config.toml` が存在するか確認する。無ければ `markharness init --dir <target>` を実行する(`.markharness/{knowledge,axes,generated,executions,changes,schema}` とデフォルトスキーマ、プロジェクトルート目印を作成。既存物には手を加えない)。
3. `.markharness/config.toml` が祖先ディレクトリにあれば、以降 `--dir` は省略可(自動でルートを検出)。複数プロジェクトを並行して扱う場合のみ明示する。

### 2.2 既存知識の確認(**再現性のために必須**)

**ドラフトを書き始める前に、必ず既存の階層を確認してください。**

```bash
markharness axes list
ls .markharness/knowledge/requirements/
ls .markharness/knowledge/features/
find .markharness/knowledge/features -name behavior.yml    # PowerShell: Get-ChildItem -Recurse -Filter behavior.yml
grep -rn "tc-" tests/ 2>/dev/null | head -40               # 既存テストが参照する case_id
```

**既存の Feature / Behavior がある場合は、その分割をそのまま再利用してください。** 新しく切り直してはいけません。

理由: 生成される TestCase の `case_id` は `tc-<feature>-<behavior>-<scenario>` の連結です。Feature の切り方が変わると**既存のテストコードが参照している case_id がすべて壊れます。** 実測例では、同じコードから2回導出した結果、Feature 分割が `todo-management`/`todo-filtering` → `todo-management`/`todo-view`/`todo-persistence` と割れ、**既存テスト14件中12件が参照先を失いました。**

既存の分割が明らかに不適切で作り直す必要があると判断した場合は、**自分で決めず、影響範囲(壊れる case_id の一覧)を示してユーザーに確認してください。**

### 2.3 スコープ確定

1. 対象コード(ファイル/モジュール/関数)を特定する。不明ならユーザーに質問する。
2. 所属する Requirement を決める(既存 id の再利用 / 新規作成)。
3. 進捗チェックリスト `checklist-knowledge-from-code.md` をプロジェクトルートに作成する。

```markdown
# Task: Derive knowledge/ from <target code>

Created: <YYYY-MM-DD>

## Steps

- [ ] <抽出予定の Behavior 1つにつき1行>

## Notes

<背景、決定事項、ブロッカー>
```

- 完了したら即座に `- [x]` にする(後でまとめてではなく)。
- 不要と判明したステップは削除せず `- [~] Skipped: <理由>` とする。
- **このファイルは作業用の一時ファイルです。** 完了後に成果物として残すかはユーザーの判断に委ねること(多くのリポジトリで `.gitignore` 済み)。

---

## 3. ドラフトの書き方

### 3.1 データモデル

```text
.markharness/
├── knowledge/
│   ├── requirements/<requirement>/requirement.yml
│   └── features/                         # Feature は Requirement 配下ではなくトップレベル
│       └── <feature>/
│           ├── feature.yml               # requirement_uids: [...] で Requirement に関連付く(複数可)
│           └── <behavior>/
│               ├── behavior.yml          # procedures: 名前付き共通手順(任意)
│               └── <scenario>/scenario.yml   # phases: 操作と結果の順序付き配列
├── axes/<axis-id>.yml                    # 横断的 axis レジストリ
├── schema/                               # markharness init が生成
└── generated/testcases/<feature>/<behavior>/<scenario>.yml   # 派生出力。手編集禁止
```

- **1 Scenario = 1 TestCase。** Condition / ExpectedResult という区分はありません。
- `scenario.phases` の**配列の並び順が実行順序の契約**です(表示上の整列ではない)。並び替えると意味が変わります。
- `scenario.id` は**同じ Behavior 内でのみ一意**であればよく、別 Behavior での再利用は衝突しません。id のリネームや衝突回避作業は不要です。
- 生成される TestCase の `axis` は **Feature と Behavior の axis の和集合**(重複除去・ソート済み)です。Scenario は axis を持ちません。Requirement の axis は継承されません。

### 3.2 ドラフトスキーマ(`KnowledgeDraft`)

**この YAML がスキーマの正本です。** `markharness knowledge scaffold` は空欄のテンプレートのみを出力し、`procedures` や `phases` の中身の書式は示しません。

```yaml
requirement:
  id: <existing-or-new-requirement-slug>
  label: <label>            # 既存かつ変更なしなら省略
  axis: [<axis-id>, ...]    # 既存かつ変更なしなら省略
  description: <text or null>

feature:
  id: <feature-slug>
  label: <label>
  axis: [<axis-id>, ...]
  description: <text>

behavior:
  id: <behavior-slug>
  label: <label>
  axis: [<axis-id>, ...]
  description: <この Behavior が行うこと。コード自身の言葉で>
  procedures:                 # 省略可。既存 Behavior を変更なしで再利用する場合も省略可
    - name: <procedure-slug>  # scenario.phases から `use: <name>` で参照
      steps:
        - <素の文字列。最低1件必須>      # ← 書式に注意(§3.3a)

scenario:
  id: <scenario-slug>         # behavior id をプレフィックスとして繰り返さない
  label: <label>
  description: <このパスを引き起こす具体的な入力/状態 + 出所(ファイルパス#関数名)>
  phases:                     # 常に完全指定。省略による再利用は不可。最低1件必須
    - steps:
        - action: <人間が手作業で行える操作>    # ← マッピング。`- <文字列>` は不可(§3.3a)
        # - use: <procedure-slug>              # Behavior の procedure を参照する場合
      results:
        - <この phase の操作後に観測できる結果。1要素=1観測。最低1件必須>
  implementation_note: <実装根拠メモ。省略可。生成には使わない>
```

**既存要素の省略ルール:**

- 既存の Requirement / Feature / Behavior は `label` / `axis` / `description` / `procedures` を**省略する**。矛盾する値を渡すと `conflicting_existing_value` で失敗します。これにより2件目以降のドラフトは実質10行で済みます。
- **Scenario にはこの省略はありません。** `description` / `phases` は常に完全指定。既存 `scenario.id` の再利用時は内容の完全一致がチェックされます。

### 3.3 書式の落とし穴(**必読 — ここで確実に一度は詰まります**)

#### (a) `steps` は2箇所にあり、書式が異なる

| 場所 | 書式 | 例 |
|---|---|---|
| `behavior.procedures[].steps` | **素の文字列** | `- ページを開く` |
| `scenario.phases[].steps` | **マッピング** `action:` または `use:` | `- action: ページを開く` |

同じ「steps」という名前ですが非対称です。`scenario.phases[].steps` で `action:` を書き忘れると、次の Rust 内部型名がそのまま出ます:

```
scenario.phases[0].steps: data did not match any variant of untagged enum StepItem
```

**このエラーを見たら `- action:` の付け忘れです。**

#### (b) YAML プレーンスカラー内のコロン

**すべてのプレーンスカラー**(`label`、`description`、`steps` の文字列、`results` の文字列)で、**コロン直後にスペースが続くとマッピングの区切りと誤認されパースエラーになります。**

```
failed to parse draft: mapping values are not allowed in this context at line 23 column 57
```

このエラーメッセージは原因を一切説明しません。対処は2つ:

- 区切り記号を変える: `app.js: addTodo` → `app.js#addTodo`、`残数: 1 件` → `残数 = 1 件`
- 値全体をダブルクォートで囲む: `description: "追加される (app.js: addTodo)"`

**迷ったらダブルクォートで囲んでください。** コロンを含む可能性のある値(日本語の説明文、ファイルパス+関数名、`残数: N`)ではこれが最も安全です。

#### (c) 行番号を書かない

出所の明記は「ファイルパス + 関数/メソッド/分岐名」までに留めます。行番号はリファクタリングや無関係な変更で陳腐化し、維持コストを増やすだけで検証可能性を高めません。

### 3.4 ドラフトの分割粒度

**Scenario ごとに1ファイル。** 大きなドラフト1つにまとめないこと。

理由(実測):

- パースエラーが複数同時に出ても、原因が1種類だと即座に分かる
- 1箇所の書式ミスの修正が1ファイルで済む
- `validate --batch` がどのファイルが悪いかをファイル名付きで返す

ファイル名は `01-xxx.yml`、`02-xxx.yml` のように順序を制御できる形にします。**バッチはファイル名順に適用され、後続のドラフトは先行するドラフトが作成した Requirement/Feature/Behavior を参照できます。**

**`--batch <dir>` は直下の `*.yml` のみを対象にします。`.yaml` 拡張子は無視されます**(該当ファイルが1つも無ければ exit code 2)。拡張子を `.yml` に統一してください。

---

## 4. 設計判断(**成果物の品質を決めるのはここ**)

コマンドは決まりきっていますが、階層の切り方は AI が判断する部分であり、実行者によってブレます。以下を基準にしてください。

### 4.1 Feature の切り方

**Feature = ユーザーから見た1つの機能領域。** 実装の内部構造(クラス、モジュール、ファイル)ではありません。

推奨の切り口を**1つ選び、プロジェクト内で統一する**こと:

| 切り口 | 適する対象 | 例 |
|---|---|---|
| 画面 / ビュー単位 | UI アプリ | `todo-list-screen`、`settings-screen` |
| ユースケース単位 | 業務アプリ | `todo-management`、`todo-filtering` |
| API エンドポイント群単位 | バックエンド | `todos-api`、`auth-api` |

**判断に迷ったら、切り口を混在させないことを最優先してください。** `todo-management`(ユースケース)と `todo-view`(画面)を並べると、新しい振る舞いをどちらに置くかが毎回曖昧になります。

**Feature の数は少なめに寄せる。** 1 Feature に Behavior が10個あるのは問題ありませんが、Feature が10個あって各1 Behavior という分割は、ほぼ確実に切りすぎです。

### 4.2 Behavior の切り方

**Behavior = Feature が行う個別の1つのこと。** 目安は「公開関数1つ」または「その中の1つの責務」です。

- `addTodo` / `deleteTodo` / `toggleTodo` → それぞれ1 Behavior
- 複数の Scenario が共通して踏む前準備(「TODO を3件登録した状態にする」等)があれば、`procedures` として Behavior に定義し、Scenario 側から `use:` で参照する

### 4.3 Scenario の切り方

**Scenario = 結果を変える入力/状態の組み合わせ1つ。** 分岐・エッジケース・エラーパスはすべて対象です。

- 正常系1つで終わらせない。`if`/`else`、早期 return、バリデーション失敗、空入力、境界値をそれぞれ Scenario にする
- **画面から観測できないものは Scenario にしない。** 例: `crypto.randomUUID()` による内部 id の採番は、UI に表示されない限り `results` に書けないため Scenario 化しない。書けるのは人間が観測できる結果だけです

### 4.4 phase の分割基準

- **新しい phase を作るのは、新しい操作(前の phase にない `steps`)を挟んでから確認する場合のみ。**
- **同じ操作の後に確認する複数の独立した観測は、1つの phase の `results` に複数行として書く。**

適用例:

| Scenario | phase 数 | 理由 |
|---|---|---|
| 「TODO を追加する」 | 1 | 操作は1回。「一覧に出る」「残数が増える」は同一 phase の results 2行 |
| 「完了にして、戻す」 | 2 | チェック操作 → 再度チェック操作、と操作が2回ある |
| 「追加後に再読み込みしても残る」 | 2 | 追加操作 → 再読み込み操作 |

### 4.5 axis の設計

axis は **TestCase を横断的に絞り込むためのタグ**です(生成される TestCase の `axis` = Feature の axis ∪ Behavior の axis)。

**1つの分類体系を選び、混在させないでください。** 実用的な既定は**テストの性質による分類**です:

```
functional      機能        正常系の主要機能
ui              UI表示      画面表示・レイアウト・表示状態の確認
persistence     永続化      保存・復元・再読み込み
error-handling  異常系      バリデーション・エラーパス・境界値
```

避けるべき設計:

- **優先度(`high`/`low`)を axis にしない。** 優先度は時期で変わり、知識の属性ではありません
- **Feature 名と1対1になる axis を作らない**(`todo-management` という axis)。Feature 自体で絞り込めるため無意味です
- **axis 数を増やしすぎない。** 4〜8個程度で、どの Behavior にも最低1つ付く粒度が実用的です

**axis はドラフト作成前にすべて登録が必要です**(§5.1)。新規プロジェクトでは `axes/` は空(`markharness axes list --json` が `[]` を返す)で、頼れる既存レジストリはありません。**先に設計してから登録してください。** 登録漏れがあると、ドラフトを全件書き終えてから全件 `unknown_axis` で弾かれます。

---

## 5. 実行手順

### 5.1 axis の登録

```bash
markharness axes list --json                          # 登録済みを確認
markharness axes add functional --label 機能           # 必要な分だけ繰り返す
```

- `--label` 省略時は `id` がそのまま label になります
- **`axes add` は冪等ではありません。** 既存 id を指定するとエラーになります。先に `list` で確認してください

### 5.2 Requirement の作成(新規の場合のみ)

既存 Requirement を再利用するだけなら、この節は飛ばして §5.3 へ。

```bash
printf '<req-id>\n<axis-id>\n' | markharness knowledge add --dir .
markharness identity migrate --dir .
```

- **新規 Requirement は `identity migrate` するまで uid を持ちません。** その間、Feature からの参照は `requirement_not_migrated` で拒否されます
- **`knowledge apply` には Requirement 単体を書き込む手段がありません。** ドラフトは requirement/feature/behavior/scenario の4セクション全部を必須とし、新規 Requirement と新規 Feature を同じドラフトで作ることは構造的に不可能です(必ず `requirement_not_migrated` で拒否され、何も書き込まれません)
- `identity migrate` は `--dry-run`(書き込まず予定のみ表示)と `--json` に対応します

### 5.3 ドラフトの検証と適用

```bash
markharness knowledge validate --batch drafts/ --json   # エラーが0になるまで修正 → 再実行
markharness knowledge apply    --batch drafts/ --json
markharness identity migrate --dir .                    # ← 必須。忘れると §5.4 の validate が落ちる
```

単一ファイルの場合は `--batch drafts/` を `<draft-file>` に置き換えます。

- `apply --batch` が途中のファイルで失敗した場合、**その回に書き込み済みのファイルも含めて全ロールバックされます**(バッチ全体が不可分)。安心して一括適用できます
- 適用が終わったドラフトに対応するチェックリストのステップを `- [x]` にします

### 5.4 生成と検証

```bash
markharness generate     # → generated N testcase(s) into .markharness/generated/testcases/
markharness validate     # → .markharness/knowledge/ and .markharness/axes/ are valid
markharness generate     # 2回目。N が同じで差分が出ないことを確認(CI が見る内容)
```

**件数の検算(OS 非依存):** `markharness generate` の出力行 `generated N testcase(s)` の **N が、適用した Scenario の数と一致すること**を確認します。一致しない場合は、同じ feature/behavior/scenario の組み合わせを誤って複数回 apply していないか確認してください。

生成される各 TestCase は `generated_from`(出所と `requirement_ids`/`requirement_uids`)、`phases`(`use:` 参照は procedure の steps に展開済み)、`axis`(Feature ∪ Behavior)から構成されます。`case_uid` / `case_revision` は決定的で、再生成しても変わりません。

**`.markharness/generated/testcases/*.yml` を手編集しないこと。** 派生出力です。書いてよいのは `.markharness/knowledge/` 配下のみ、それも `apply` 経由だけです。

### 5.5 完了処理

1. チェックリストに `## Summary` を追加する: どの Feature/Behavior/Scenario を追加したか、それぞれどのコード(ファイルパス・関数名。行番号は書かない)に遡れるか。
2. ユーザーに報告する:
   - 新規/変更された `.markharness/generated/testcases/*.yml`
   - コードの意図が曖昧でスキップした箇所
   - **既存のテストコードが参照する case_id を壊した場合は、その一覧**(§2.2)

---

## 6. リファレンス

### 6.1 エラー別の対処

#### パース段階のエラー(構造化されず、生のメッセージが出る)

| メッセージ | 原因 | 対処 |
|---|---|---|
| `mapping values are not allowed in this context at line N column M` | プレーンスカラー内の `": "` | 該当行の値をダブルクォートで囲む、または `:` を `#` 等に置換(§3.3b) |
| `data did not match any variant of untagged enum StepItem` | `scenario.phases[].steps` の `- action:` 付け忘れ | `- action: <操作>` または `- use: <name>` にする(§3.3a) |

#### `knowledge validate` が返す構造化エラー

| コード | 意味 / 対処 |
|---|---|
| `unknown_axis` | axis 未登録。`markharness axes add` で先に登録(§5.1)。最も近い候補が提示される |
| `requirement_not_migrated` | 新規 Requirement に uid が無い。`markharness identity migrate` を実行(§5.2) |
| `conflicting_existing_value` | 既存要素に異なる値を渡した。既存要素では `label`/`axis`/`description` を省略する(§3.2) |
| `missing_steps` | `behavior.procedures[].steps` / `scenario.phases` / `phases[].steps` / `phases[].results` のいずれかが欠落・空・空文字列要素を含む。`procedures[].steps` は空配列も拒否。`phases` とその `steps`/`results` は常に非空必須 |
| `missing_description` | `description` が未指定 |
| `missing_axis` | 新規要素に `axis` が無い。最低1件必要 |
| `invalid_slug` | id に使えない文字。小文字英数字とハイフンにする |
| `redundant_prefix` | `scenario.id` が behavior id をプレフィックスとして繰り返している。id を短くするか、意図的なら `apply --strip-redundant-prefix` |
| `multiline_label` | `label` に改行が含まれる |
| `parent_not_found` | 参照先の親要素が存在しない。バッチ内の適用順(ファイル名順)を確認 |
| `unknown_forked_from` | `feature.forked_from` の参照先が存在しない |

#### `markharness validate`(プロジェクト全体)のエラー

```
project is in UID mode ([identity] mode = "uid") but this <kind> '<id>' has no uid
```

→ `markharness identity migrate --dir .` を実行(§5.3 の最後の migrate 漏れ)。

### 6.2 2回目以降の更新

既存の知識に追記・修正する場合:

1. **§2.2 を必ず実施し、既存の Feature/Behavior 分割を再利用する。**
2. 既存要素は `label`/`axis`/`description`/`procedures` を省略したドラフトを書く。
3. 既存 `scenario.id` を再利用する場合、`description`/`phases` は完全一致が必要。変更したい場合は内容を書き換えたドラフトを apply する。
4. Feature の分割を変更する場合、既存テストが参照する case_id が壊れます。**自分で決めず、影響一覧を出してユーザーに確認してください。**

### 6.3 このドキュメントで扱っていないオプション

初回導出では不要です。必要になったら `--help` を参照してください。

- `feature.forked_from` — 他の Feature の真の派生である場合のみ
- `apply --strip-redundant-prefix` — `redundant_prefix` を意図的に剥がす
- `scenario.implementation_note` — 実装根拠メモ。生成には使われない
- `knowledge add --edit` — `$EDITOR` を開く。**AI からは使用不可**
- `apply --batch --dry-run` — `validate --batch` と同等のチェック
- `markharness axes prune` — 未参照 axis の報告
- `markharness identity sync` / `audit` / `resolve` — identity 履歴の修復・監査

### 6.4 外部状態への依存

Scenario の結果がコードだけでは決まらない外部状態(I/O・並行性・設定・時刻)に依存する場合は、単一の決定的な結果を断定せず、その旨を `description` に記載してください。
