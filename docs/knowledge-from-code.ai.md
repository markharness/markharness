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
markharness knowledge reconcile --print-template > intent.yml   # Knowledge Intent の雛形
# ... intent.yml を書く(§3)...
markharness knowledge reconcile intent.yml --check --json   # 検証のみ。エラーが0になるまで繰り返す
markharness knowledge reconcile intent.yml --json           # 反映(単一トランザクション)
markharness generate                                   # → "generated N testcase(s) into ..."
markharness validate                                   # → ".markharness/knowledge/ and .markharness/axes/ are valid"
markharness generate                                   # もう一度。N が同じで差分が出ないことを確認
```

**Knowledge の書込み口は `knowledge reconcile` だけです。** Intent(望ましい状態を書いた1ファイル)を渡すと、現在状態との差分から作成・更新・renameが決まり、UID の発行とファイル書込みが単一トランザクションで行われます。`identity migrate` を挟む必要はありません。

### 落とし穴 ①: 既存要素を書き直すときは内容を完全に一致させる

Intent は**望ましい状態**の記述なので、既存要素をもう一度書いても構いません。ただし判定は次のとおりです(表示IDでの照合)。

| Intent の書き方 | 現在状態 | 結果 |
|---|---|---|
| `uid` なし、同じ scope に同じ id が無い | — | 新規作成 |
| `uid` なし、同じ id があり内容も一致 | 既存 | `unchanged`(何も書かれない) |
| `uid` なし、同じ id があるが内容が違う | 既存 | **`ambiguous_identity` で停止**。UID の明示を要求 |
| `uid` あり | 既存 | 内容を比較して更新。`id` を変えれば rename |

**つまり「既存要素の内容を変えたい」ときだけ `uid` が必要です。** UID は反映成功時の出力か `--json` の結果から取得します。

### 落とし穴 ②: axis は Intent を書く前に全部登録する

未登録 axis は `unknown_axis` で反映前に全件弾かれます。Intent を全部書き終えてから気づくと手戻りになります(§4.5・§5.1)。

### 落とし穴 ③: `--check` の結果は書込みの許可証ではない

`--check` は解析・照合・検証・計画までを本番と同じ実装で行い、書込みだけをしません。ただし通常実行はコミット直前に現在状態を読み直し、その間に状態が変わっていれば `stale_plan` で停止します。`--check` が通ったからといって、次の実行が必ず成功するとは限りません。

---

## 2. 準備とスコープ確認

### 2.1 ツールの確認

1. `markharness --version` を実行する。コマンドが見つからない場合は**処理を止め**、ユーザーに次を伝える: `markharness` のビルド/インストールが必要(そのリポジトリで `cargo install --path .`、またはビルド済みバイナリ)。出力を捏造したり、確認なしに先へ進んだりしない。
2. `.markharness/config.toml` が存在するか確認する。無ければ `markharness init --dir <target>` を実行する(`.markharness/{knowledge,axes,generated,executions,changes,schema}` とデフォルトスキーマ、プロジェクトルート目印を作成。既存物には手を加えない)。
3. `.markharness/config.toml` が祖先ディレクトリにあれば、以降 `--dir` は省略可(自動でルートを検出)。複数プロジェクトを並行して扱う場合のみ明示する。

### 2.2 既存知識の確認(**再現性のために必須**)

**Intent を書き始める前に、必ず既存の階層を確認してください。**

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

## 3. Knowledge Intent の書き方

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

**Intent はこの保存形式とは別のスキーマです。** Intent は authoring 専用の入力であり、保存される YAML そのものではありません。新規要素は Intent 内ローカルな `key` で相互参照し(`key` は保存されません)、既存要素は `uid` で選択します。

### 3.2 Intent スキーマ

**この YAML がスキーマの正本です。** `markharness knowledge reconcile --print-template` は空欄の雛形のみを出力し、`procedures` や `phases` の中身の書式は示しません。

```yaml
format: markharness/knowledge-intent/v1
mode: merge                    # 初期版は merge のみ(既存要素を削除しない)

requirements:
  - key: <intent-local-name>   # 新規要素の Intent 内参照名。保存されない
    # uid: <ULID>              # 既存 Requirement を変更する場合のみ。key と排他
    id: <requirement-slug>
    source: native             # native | external
    label: <label>             # source: native では必須。source: external では書けない
    # source: external では source_locator と source_revision: current が必須(§6.3)
    axis: [<axis-id>, ...]
    description: <text or null>          # 省略可
    related_issues: []                   # 省略可

features:
  - key: <intent-local-name>
    # uid: <ULID>              # 既存 Feature を変更する場合のみ
    id: <feature-slug>
    contributes_to: [<requirement key または uid>, ...]   # 全置換
    label: <label>
    axis: [<axis-id>, ...]
    description: <text>                  # 省略可
    forked_from: <feature-id or null>    # 省略可。概念的な派生元(§6.3)
    behaviors:
      - id: <behavior-slug>
        # uid: <ULID>          # 既存 Behavior を変更する場合のみ
        label: <label>
        axis: [<axis-id>, ...]
        description: <この Behavior が行うこと。コード自身の言葉で>   # 新規作成時は必須
        procedures:                 # 省略可。全置換
          - name: <procedure-slug>  # scenario.phases から `use: <name>` で参照
            steps:
              - <素の文字列。最低1件必須>      # ← 書式に注意(§3.3a)
        scenarios:
          - id: <scenario-slug>     # behavior id をプレフィックスとして繰り返さない
            # uid: <ULID>           # 既存 Scenario を変更/reparent する場合のみ
            label: <label>
            description: <このパスを引き起こす具体的な入力/状態 + 出所(ファイルパス#関数名)>
            phases:                 # 新規作成時は必須。最低1件
              - steps:
                  - action: <人間が手作業で行える操作>    # ← マッピング。`- <文字列>` は不可(§3.3a)
                  # - use: <procedure-slug>              # Behavior の procedure を参照する場合
                results:
                  - <この phase の操作後に観測できる結果。1要素=1観測。最低1件必須>
            implementation_note: <実装根拠メモ。省略可。生成には使わない>
```

**値のcollection(`axis` / `contributes_to` / `procedures`)は全置換です。** 記述すればその内容で置き換わり、省略すれば現在値を保ち、空配列を明示すれば空になります。

**Requirement と Feature を同じ Intent で新規作成できます。** Feature の `contributes_to` に Requirement の `key` を書けば、同じ反映の中で発行された UID へ解決されます。

### 3.3 書式の落とし穴(**必読 — ここで確実に一度は詰まります**)

#### (a) `steps` は2箇所にあり、書式が異なる

| 場所 | 書式 | 例 |
|---|---|---|
| `behaviors[].procedures[].steps` | **素の文字列** | `- ページを開く` |
| `scenarios[].phases[].steps` | **マッピング** `action:` または `use:` | `- action: ページを開く` |

同じ「steps」という名前ですが非対称です。`phases[].steps` で `action:` を書き忘れると、次の Rust 内部型名がそのまま出ます:

```
data did not match any variant of untagged enum StepItem
```

**このエラーを見たら `- action:` の付け忘れです。**

#### (b) YAML プレーンスカラー内のコロン

**すべてのプレーンスカラー**(`label`、`description`、`steps` の文字列、`results` の文字列)で、**コロン直後にスペースが続くとマッピングの区切りと誤認されパースエラーになります。**

```
error[invalid_format]: mapping values are not allowed in this context at line 23 column 57 (<document>)
```

このエラーメッセージは原因を一切説明しません。対処は2つ:

- 区切り記号を変える: `app.js: addTodo` → `app.js#addTodo`、`残数: 1 件` → `残数 = 1 件`
- 値全体をダブルクォートで囲む: `description: "追加される (app.js: addTodo)"`

**迷ったらダブルクォートで囲んでください。** コロンを含む可能性のある値(日本語の説明文、ファイルパス+関数名、`残数: N`)ではこれが最も安全です。

#### (c) 行番号を書かない

出所の明記は「ファイルパス + 関数/メソッド/分岐名」までに留めます。行番号はリファクタリングや無関係な変更で陳腐化し、維持コストを増やすだけで検証可能性を高めません。

### 3.4 Intent の分割粒度

**1つの Intent に複数の Requirement / Feature / Behavior / Scenario をまとめて書いて構いません。** 反映は全体で1トランザクションであり、どれか1つでも検証に失敗すれば何も書き込まれません。

診断は `features[0].behaviors[1].scenarios[0].phases` のような `location` 付きで返るため、1ファイルにまとめてもどこが悪いかは特定できます。`--check --json` は**検出できた診断をまとめて返す**ので、1件直しては再実行する必要はありません。

導出対象が大きい場合は、**Feature 単位で Intent ファイルを分ける**のが実用的です(`intent-todo-management.yml` 等)。1つの Intent が巨大になると、YAML のインデント階層が深くなり書式ミスを起こしやすくなります。

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

**axis は Intent を書く前にすべて登録が必要です**(§5.1)。新規プロジェクトでは `axes/` は空(`markharness axes list --json` が `[]` を返す)で、頼れる既存レジストリはありません。**先に設計してから登録してください。**

---

## 5. 実行手順

### 5.1 axis の登録

```bash
markharness axes list --json                          # 登録済みを確認
markharness axes add functional --label 機能           # 必要な分だけ繰り返す
```

- `--label` 省略時は `id` がそのまま label になります
- **`axes add` は冪等ではありません。** 既存 id を指定するとエラーになります。先に `list` で確認してください

### 5.2 Intent の作成

```bash
markharness knowledge reconcile --print-template > intent.yml
```

雛形は Requirement 1件 → Feature 1件 → Behavior 1件 → Scenario 1件の最小構成です。§3.2 を正本として書き足してください。

### 5.3 検証と反映

```bash
markharness knowledge reconcile intent.yml --check --json   # エラーが0になるまで修正 → 再実行
markharness knowledge reconcile intent.yml --json           # 反映
```

- 反映は**単一トランザクション**です。検証に失敗すれば何も書き込まれず、途中で中断しても中途半端な状態は後続コマンドへ公開されません
- UID の発行も同じトランザクション内で行われます。**`identity migrate` を別途実行する必要はありません**
- 終了コード: `0` 成功 / `1` 検証エラー / `3` 他の identity 操作が進行中または回復保留 / `4` `--check` で変更が生じる状態
- 反映が終わった Scenario に対応するチェックリストのステップを `- [x]` にします

### 5.4 生成と検証

```bash
markharness generate     # → generated N testcase(s) into .markharness/generated/testcases/
markharness validate     # → .markharness/knowledge/ and .markharness/axes/ are valid
markharness generate     # 2回目。N が同じで差分が出ないことを確認(CI が見る内容)
```

**件数の検算(OS 非依存):** `markharness generate` の出力行 `generated N testcase(s)` の **N が、反映した Scenario の総数と一致すること**を確認します。

生成される各 TestCase は `generated_from`(出所と `requirement_ids`/`requirement_uids`)、`phases`(`use:` 参照は procedure の steps に展開済み)、`axis`(Feature ∪ Behavior)から構成されます。`case_uid` / `case_revision` は決定的で、再生成しても変わりません。

**`.markharness/generated/testcases/*.yml` を手編集しないこと。** 派生出力です。書いてよいのは `.markharness/knowledge/` 配下のみ、それも `knowledge reconcile` 経由だけです。

### 5.5 完了処理

1. チェックリストに `## Summary` を追加する: どの Feature/Behavior/Scenario を追加したか、それぞれどのコード(ファイルパス・関数名。行番号は書かない)に遡れるか。
2. ユーザーに報告する:
   - 新規/変更された `.markharness/generated/testcases/*.yml`
   - コードの意図が曖昧でスキップした箇所
   - **既存のテストコードが参照する case_id を壊した場合は、その一覧**(§2.2)

---

## 6. リファレンス

### 6.1 エラー別の対処

診断は人間可読モードでは `error[<code>]: <message> (<location>)`、`--json` では `{"ok":false,...}` の形で返ります。

| コード | 意味 / 対処 |
|---|---|
| `invalid_format` | YAML がパースできない、または `format` が `markharness/knowledge-intent/v1` でない。プレーンスカラー内の `": "` が原因のことが多い(§3.3b) |
| `unknown_axis` | axis 未登録。`markharness axes add` で先に登録(§5.1) |
| `unknown_local_reference` | `contributes_to` が、同じ Intent に存在しない `key` を指している |
| `unknown_uid` | 指定した `uid` を持つ要素が存在しない |
| `duplicate_key` / `duplicate_uid` | 同じ Intent 内で `key` / `uid` を重複して使っている |
| `ambiguous_identity` | `uid` なしで書いた要素が、同じ id の既存要素と内容不一致。変更したいなら `uid` を明示する(落とし穴①) |
| `conflicting_scope` | Behavior を別の Feature へ移そうとした。Behavior の移動は非対応(移動できるのは Scenario のみ) |
| `conflicting_existing_value` | 書込み先のパスが別のファイルに占有されている。id を変えるか、既存ファイルを整理する |
| `invalid_procedure_reference` | `use:` が、その Behavior の `procedures` に無い名前を指している |
| `invalid_source_revision` | `source_revision` の値が不正(`current` 以外を書いた、`source: native` に対して `current` を書いた等) |
| `missing_required_field` | 必須フィールドの欠落。新規作成時の `id` / `description` / `phases`、`source` など |
| `invalid_slug` | id に使えない文字。小文字英数字とハイフンにする |
| `redundant_prefix` | `scenario.id` が behavior id をプレフィックスとして繰り返している。id を短くする |
| `multiline_label` | `label` に改行が含まれる。`label` は単一行のみ |
| `invariant_violation` | 既存要素が `uid` を持たない等、前提が壊れている。`markharness identity migrate` での修復が必要 |
| `stale_plan` | 計画を立ててから反映するまでの間に現在状態が変わった。もう一度実行する |

### 6.2 2回目以降の更新

既存の知識に追記・修正する場合:

1. **§2.2 を必ず実施し、既存の Feature/Behavior 分割を再利用する。**
2. **追加だけなら `uid` は不要です。** 既存要素を同じ内容で書き直せば `unchanged` となり、追加した要素だけが `created` になります。
3. **既存要素の内容を変えるなら `uid` が必要です。** `uid` なしで内容だけ変えると `ambiguous_identity` で停止します。UID は直前の反映結果か `--json` の出力から取得します。
4. rename は `uid` で対象を選び、`id` に新しい値を書きます。uid と版履歴は維持されます。
5. Scenario を別の Behavior へ移す(reparent)場合は、その Scenario を `uid` で指定し、移動先 Behavior の下に書きます。
6. Feature の分割を変更する場合、既存テストが参照する case_id が壊れます。**自分で決めず、影響一覧を出してユーザーに確認してください。**

### 6.3 このドキュメントで扱っていないオプション

初回導出では不要です。必要になったら `--help` を参照してください。

- `features[].forked_from` — 他の Feature の真の派生である場合のみ
- `scenarios[].implementation_note` — 実装根拠メモ。生成には使われない
- `requirements[].source: external` — 外部ドキュメント(`.sdoc` 等)を出典とする Requirement。`native` とは field の集合が排他で、`label` は書けず、`source_locator`(リポジトリ内のパス)と `source_revision: current`(実行時に現在の blob OID へ解決される)が必須
- `markharness axes prune` — 未参照 axis の報告
- `markharness identity sync` / `audit` / `resolve` / `migrate` — identity 履歴の修復・監査

### 6.4 外部状態への依存

Scenario の結果がコードだけでは決まらない外部状態(I/O・並行性・設定・時刻)に依存する場合は、単一の決定的な結果を断定せず、その旨を `description` に記載してください。
