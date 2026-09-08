# Knowledge From Code (standalone)

> このファイルは単体で完結しています — このファイル1つだけを任意のプロジェクトにコピーする(あるいは本文をそのまま AI コーディングアシスタントとのチャットに貼り付ける)だけで動作し、`markharness` テンプレートリポジトリの他のファイルを一切必要としません。唯一の外部依存は `markharness` CLI 自体です(Phase 0 を参照)。
>
> ユーザーがチャットで使っている言語で応答してください。

既存のソースコードを読み、そこから `markharness` のテスト知識を導出し、`markharness generate` で TestCase データを生成します。これはテスト知識(`.markharness/knowledge/**/{requirement,feature,behavior,scenario}.yml`)の AI 支援による作成であり、人間によるレビューの代替ではありません。導出した Scenario/Phase はすべて、コード中の具体的な箇所に遡れる必要があります。

## 進捗管理(チェックリストルールを内蔵)

作業開始前に、プロジェクトルートに `checklist-knowledge-from-code.md` を作成してください。

```markdown
# Task: Derive knowledge/ from <target code>

Created: <date>

## Steps

- [ ] <抽出予定の Behavior 1つにつき1行>

## Notes

<背景、決定事項、ブロッカー>
```

- 各ステップは完了したら即座に `- [x]` にする(後でまとめてではなく)。
- あるステップが不要と判明した場合は削除せず `- [~] Skipped: <理由>` とする。
- すべてのステップが完了したら `## Summary` セクションを追加する(Phase 7 参照)。

## データモデル(参考 — 他のファイルは不要)

Feature は Requirement 配下のサブディレクトリではなく、`features/` 直下に自身の id で置かれる**トップレベルの**ディレクトリです。Requirement との関連は Feature 側が持つ `requirement_uids`(複数可)で表現され、1つの Feature が複数の Requirement に対等に関連付くことができます。詳細は `docs/ja/decisions/0017-scenario-case-revision-and-execution-evidence.md` を参照してください。

```text
.markharness/
├── knowledge/
│   ├── requirements/
│   │   └── <requirement>/
│   │       └── requirement.yml
│   └── features/
│       └── <feature>/           # Requirement 配下ではなくトップレベル
│           ├── feature.yml      # requirement_uids: [<requirement の uid>, ...]
│           └── <behavior>/
│               ├── behavior.yml # procedures: 名前付き共通手順(任意)
│               └── <scenario>/
│                   └── scenario.yml  # phases: 操作(action/use)と確認結果の配列
├── axes/                # 横断的な axis レジストリ、axis 1つにつき axes/<id>.yml 1ファイル
├── schema/              # markharness init が生成するデフォルトスキーマ
└── generated/
    └── testcases/       # <feature>/<behavior>/<scenario>.yml、`markharness generate` により決定的に再生成される — 手編集禁止
```

Condition/ExpectedResult という区分はありません。Scenario が持つ順序付き `phases` 配列(各 phase が `steps` と `results` を持つ)がそれに代わるもので、**1 Scenario = 1 TestCase** です。実行順序の正本は配列の並び順そのもので、ファイル名順ではありません。

## 手順

### Phase 0 — ツールが使えることを確認する

1. `markharness --version`(または `--help`)を実行する。コマンドが見つからない場合は処理を止め、ユーザーに次を伝える: まず `markharness` をビルド/インストールする必要がある(そのリポジトリから `cargo install --path .`、またはビルド済みバイナリを使う)、あるいは本当に別のツールを意図していないか確認する。出力を捏造したり、確認なしに先へ進んだりしないこと。
2. 対象ディレクトリに `.markharness/`(`knowledge/`・`axes/`・`schema/` 等を含む)が既に存在するか(=そこで `markharness init` が実行済みか)を確認する。存在しなければ、先に `markharness init --dir <target>` を実行する — `.markharness/{knowledge,axes,generated,executions,changes,schema}` と、`markharness validate` が必要とするデフォルトの `.markharness/schema/*.schema.json`、プロジェクトルート目印 `.markharness/config.toml` を作成する(既存のものには一切手を加えない)。
3. `init` 済みのプロジェクト配下(`.markharness/config.toml` が祖先ディレクトリにあるところ)であれば、以降の各コマンドは `--dir` を省略してもそこまで遡ってプロジェクトルートを自動検出する。複数プロジェクトを並行して扱う場合や、カレントディレクトリがプロジェクト外の場合は明示的に `--dir <target>` を指定する。
4. 全くの新規プロジェクトでは `axes/` が空なので、使う予定のある axis は*すべて*、ドラフト作成前に(Phase 3 で)新規作成する必要がある — 頼れる既存レジストリは存在しない。

### Phase 1 — スコープ確認

1. 対象コード(ユーザーが指定したファイル/モジュール/関数、不明なら質問する)を特定する。
2. このコードがどの `requirement` に属するかを確認する(既存の `.markharness/knowledge/requirements/<requirement>/` id、または新規作成 — 新規プロジェクトでは常に新規となる)。
3. 上で作成したチェックリストファイルに、抽出予定の Behavior 1つにつき1行を記入する。

### Phase 2 — コードを分析する

対象コードを読み、公開関数/分岐/エラーパスごとに次を特定する:

- **Feature**: そのコードが実装しているユーザー向けの機能。1つ以上の Requirement に `requirement_uids` で関連付く(Phase 4 の「Requirement を先に migrate する」参照)。
- **Behavior**: Feature が行う個別の1つのこと(例: 1つの関数、またはその中の1つの責務)。複数の Scenario が共通して使う手順があれば、`procedures`(名前付き手順)としてここに定義する。
- **Scenario**: 結果を変える入力/状態の組み合わせ(分岐・エッジケース・エラーパスもすべて対象)ごとに1つ作る。順序付き `phases` を持ち、各 phase は操作(`steps`)と、その操作後に観測できる結果(`results`)の組。**1 Scenario = 1 TestCase**。

コードが示していない振る舞いを創作しないこと。意図が曖昧な場合(`TODO` や、分岐の解釈が複数あり得る場合など)は、推測せず処理を止めてユーザーに確認する。

`phases[].steps` は、人間の Test Executor が手順書として読んで手作業で実施するものであることを常に念頭に置く(markharness に自動実行エンジンはない)。コードを読んで裏付けを取るのは構わないが、書き出す操作は関数呼び出しや内部処理ではなく、その関数/分岐を実際に引き起こすユーザー操作(画面のクリック・入力・API リクエスト送信など、人間または人間相当の操作主体が行える操作)として記述する。「`addTodo()` を呼び出す」ではなく「入力欄に "牛乳を買う" と入力し、「追加」ボタンをクリックする」のように書く。各 steps 要素は `action: <操作>` または `use: <Behavior が定義した procedure 名>` のいずれか。

同じ操作の後に確認する独立した複数の観測結果は、1つの phase の `results` に複数行として書く。新しい phase を作るのは、新しい操作(前の phase にない `steps`)を挟んでから確認する新しい局面を表現する場合のみ。

`description` に出所を「ファイルパス + 関数/メソッド名」で書く際は **YAML のプレーンスカラー内でコロン直後にスペースを続けた `":"` を使うとマッピングの区切りと誤認されパースエラーになる**(例: `description: 追加される (app.js: addTodo)` はエラー)。ファイルパスと関数名を区切るときは `:` ではなく `#`/`>`/`—` など別の記号を使う(例: `app.js#addTodo` や `app.js > addTodo`)か、値全体をダブルクォートで囲む(`description: "追加される (app.js: addTodo)"`)。

### Phase 3 — axis を確認する

1. `markharness axes list --json` を実行し、登録済みの `.markharness/axes/*.yml` エントリを確認する。
2. *新規*の requirement/feature/behavior で使う予定の `axis` 値はすべて事前に登録されている必要がある(`markharness knowledge validate` は未登録の axis を `unknown_axis` として拒否し、最も近い候補を提示する)。必要な axis が存在しない場合は、続行前に `markharness axes add <id> [--label <label>]` で登録する(`--label` 省略時は `id` がそのまま label になる。既に存在する id を指定するとエラーになる)。
3. Scenario の axis は個別に持たない — 生成される TestCase の `axis` は Feature と Behavior の axis の和集合(重複除去・ソート済み)になる。Requirement の axis は自動継承されない(Requirement を軸にした横断検索は `requirement_ids`/`requirement_uids` の関連をたどって行う設計であるため)。

### Phase 4 — Requirement を identity 上 migrate する(新規 Requirement の場合のみ)

Feature は表示 id ではなく Requirement の不変 uid で関連付けられる。**新規作成された Requirement は `markharness identity migrate` を実行するまで uid を持たず**、その間は新規 Feature からの参照が `requirement_not_migrated` エラーで拒否される。

1. 既存の Requirement を再利用するだけ(新規作成しない)なら、この Phase は不要 — 次の Phase 5 に進む。
2. 新規 Requirement を作る場合は `markharness knowledge add`(対話モード)を実行し、Requirement 名(と axis)を入力する。新規 Requirement は `identity migrate` 実行前は常に uid を持てないため、対話はこの時点で Requirement を書き込んだところで自動的に終了し、続けて Feature 等を尋ねることはない。**`markharness knowledge apply`(ドラフトファイル)には Requirement 単体を書き込む手段がない** — ドラフトは常に requirement/feature/behavior/scenario の4セクション全部を必須とし、新規 Requirement と新規 Feature を同じドラフトで同時に作ろうとしても検証時点で `requirement_not_migrated` により必ず拒否され、何も書き込まれない(この組み合わせは構造的に不可能な設計になっている)。
3. Requirement 作成後、`markharness identity migrate --dir <target>` を実行する。これは「uid を持たないすべての Knowledge 要素(Requirement/Feature/Behavior/Scenario)に uid を割り当てる」冪等な操作で、既に uid を持つ要素には何もしない。`--dry-run` で実際に書き込まず割り当て予定だけを確認でき、`--json` で機械可読出力にできる。
4. Feature を新規作成するドラフトを `apply` する前に、参照する Requirement が migrate 済みであることを確認する。

### Phase 5 — ドラフト作成・検証・適用(Scenario ごとに繰り返す)

Phase 2 で特定した各 Scenario について:

1. `KnowledgeDraft` スキーマに合致するドラフト YAML ファイルを(例えばスクラッチパスに)書く。空の雛形は `markharness knowledge scaffold`(stdout に出力、`--out <path>` でファイル出力も可 — 既存ファイルは上書きしない)で取得できる:

   ```yaml
   requirement:
     id: <existing-or-new-requirement-slug>
     label: <label> # requirement が既存かつ変更なしなら省略可
     axis: [<axis-id>, ...]
     description: <text or null>

   feature:
     id: <feature-slug>
     label: <label>
     axis: [<axis-id>, ...]
     description: <text>
     # forked_from: <existing-feature-id>   # 他の Feature の真の派生である場合のみ

   behavior:
     id: <behavior-slug>
     label: <label>
     axis: [<axis-id>, ...]
     description: <この Behavior が行うこと。コード自身の言葉で>
     procedures: # 省略可(省略時は「procedures なし」と同義)。既存 Behavior を変更なしで再利用する場合も省略可
       - name: <procedure-slug> # 同じ Behavior 内の scenario.phases から `use: <name>` で参照する
         steps:
           - <この procedure の操作手順。人間が手作業で行える操作として書く。最低1件必須>

   scenario:
     id:
       <scenario-slug> # behavior id をプレフィックスとして繰り返さない
       # 同じ Behavior 内でのみ一意であればよい —
       # 詳細は下記「Scenario id の一意性」参照
     label: <label>
     description: <このパスを引き起こす具体的な入力/状態 — ファイルパスと関数/メソッド名で出所を明記(行番号は書かない。理由は「原則」参照)>
     phases: # 常に完全に指定する必要がある(既存 Behavior のような「変更なしなら省略可」は Scenario にはない) — 最低1件必須
       - steps:
           - action: <人間が手作業で行える操作> # または `- use: <procedure-slug>`
         results:
           - <この phase の操作後に観測できる結果。コードから読み取る。1要素=1つの観測結果。最低1件必須>
     implementation_note: <実装根拠メモ。省略可。生成には使わない>
   ```

   既に存在し変更のない Requirement/Feature/Behavior では `label`/`axis`/`description`/`procedures` を省略する — 矛盾する値を渡すと `conflicting_existing_value` で検証に失敗する。Scenario にはこの「省略による再利用」はない — `description`/`phases` は常に完全に指定し、既存の `scenario.id` を再利用する場合は内容が完全一致しているかがチェックされる(不一致は `conflicting_existing_value`)。

   同じ Scenario の下で複数の局面(例:「追加した直後の確認」「再読み込み後の永続化確認」)を検証したい場合は、`phases` 配列に複数の要素を書く。新しい phase を作るのは新しい操作(前の phase にない `steps`)を挟む場合のみ — 同じ操作の結果を複数行に分けたいだけなら、新しい phase を作らず既存の `results` に行を足すこと。

   **Scenario id の一意性:** `markharness generate` は各 Scenario を `.markharness/generated/testcases/<feature>/<behavior>/<scenario-id>.yml` に書き出す — `.markharness/knowledge/features/` と全く同じ階層(Feature がトップレベル)をそのままミラーする。そのため `scenario.id` は同じ Behavior 内でのみ一意であればよく(`markharness knowledge validate`/`apply` もその範囲でのみ一意性を検証する)、異なる Behavior で同じ id を再利用しても(例: `add-todo` と `edit-todo` の両方で `valid-title` を使う等)出力が衝突することはない。id をリネームしたり衝突を避けたりする作業は不要 — 詳細は末尾の「原則」を参照。

2. 検証: `markharness knowledge validate <draft-file> --json`。報告されたエラー(`invalid_slug`、`missing_axis`、`missing_description`、`missing_steps`、`unknown_axis`、`redundant_prefix`、`conflicting_existing_value`、`parent_not_found`、`unknown_forked_from`、`multiline_label`、`requirement_not_migrated`)をすべて解消してから次に進む。`requirement_not_migrated` は Phase 4 の migrate 未実施が原因なので、先に `markharness identity migrate` を実行する。`missing_steps` は `behavior.procedures[].steps`/`scenario.phases`/`scenario.phases[].steps`/`scenario.phases[].results` のいずれかが欠落・空、または空文字列の要素を含む場合に出る。`behavior.procedures[].steps` は(新規 procedure 作成時)空配列も拒否されるが、`scenario.phases` とその中の `steps`/`results` は常に(既存 Scenario の再利用時も含め)非空が必須で、旧版にあった「追加前提・追加手順は空配列 `[]` を許容する」という例外は存在しない。
3. 適用: `markharness knowledge apply <draft-file> --json`(`--strip-redundant-prefix` は、意図的に `behavior-` プレフィックス付きの `scenario.id` を剥がしたい場合のみ追加する)。
4. 対応するチェックリストのステップを完了にする。

複数の Scenario をまとめて処理する場合は、ドラフトファイルを1つのディレクトリに集め、`markharness knowledge validate --batch <dir> --json`(または同じチェックを行う `markharness knowledge apply --batch <dir> --dry-run --json`)で一括検証してから `markharness knowledge apply --batch <dir> --json` を実行してもよい。`--batch <dir>` は直下の `*.yml` ファイルのみを対象にする(`.yaml` 拡張子のファイルは無視され、該当ファイルが1つもなければ exit code 2 で失敗する)。ファイルはディレクトリ内のファイル名順に適用され、後続のドラフトは同じバッチ内で先行するドラフトが作成した Requirement/Feature/Behavior を参照できる(例: `01-xxx.yml`、`02-xxx.yml` のように命名して順序を制御する)。`apply --batch`(`--dry-run` なし)が途中のファイルで失敗した場合、その回の呼び出しで書き込み済みのファイルも含めてすべてロールバックされる(バッチ全体が不可分)。

### Phase 6 — 生成

計画していたすべての Scenario を適用し終えたら:

1. `markharness generate` を実行し、`.markharness/knowledge/` から `.markharness/generated/testcases/*.yml` を決定的に(再)生成する。1 Scenario = 1 TestCase の粒度で、各ファイルは `generated_from`(feature/behavior/scenario の出所と、Requirement への関連 `requirement_ids`/`requirement_uids`)、`phases`(scenario.yml の `phases` をそのまま、`use:` 参照は所属 Behavior の procedure の steps に展開済み)、`axis`(Feature と Behavior の axis の和集合)から構成される。
2. 生成されたファイル数が、適用した Scenario の数と一致することを確認する(例: `find .markharness/knowledge/features -name scenario.yml | wc -l` と `find .markharness/generated/testcases -type f -name "*.yml" | wc -l` を比較 — `.markharness/generated/testcases/` は `.markharness/knowledge/features/` と同じ階層にミラーされるため、`-maxdepth 1` は付けずに再帰的に数える)。数が一致しない場合は、同じ feature/behavior/scenario の組み合わせに誤って複数回 apply していないか等、意図しない重複を確認する。
3. `markharness validate` を実行し、`.markharness/knowledge/`/`.markharness/axes/` が引き続き `.markharness/schema/*.schema.json` に準拠し、相互参照が解決することを確認する。
4. もう一度 `markharness generate` を実行し、差分が出ないことを確認する — これは多くの CI 設定がチェックする内容なので、引き渡す前にローカルで確認しておく。

### Phase 7 — 完了処理

1. チェックリストファイルに `## Summary` セクションを追加する: どの Feature/Behavior/Scenario を追加したか、それぞれがどのコード(ファイルパス・関数/メソッド名)に遡れるか。行番号は挙げない(理由は「原則」参照)。
2. どの `.markharness/generated/testcases/*.yml` が新規/変更されたかをユーザーに報告し、コードの意図が曖昧でスキップせざるを得なかった箇所があればそれも伝える。

## 原則

- すべての Scenario/Phase は、その出所となったコードを明記すること — 推測によるテスト知識は作らない。ただし出所の明記は「ファイルパス + 関数/メソッド/分岐名」までにとどめ、行番号のようにリファクタリングや無関係な変更で頻繁にズレる情報は description に含めない — 記載しても実装が変わるたびに陳腐化し、`knowledge/` の維持コストを増やすだけで検証可能性を高めない。
- `phases[].steps`(`action:`/`use:` のいずれか)はコードを読んで裏付けを取った上で、常に人間の Test Executor が手作業で実施できる操作(画面操作・入力・API リクエスト送信など)として書く。markharness に自動実行エンジンはなく、これらは人間が上から順に読んで実施する手順書であるため、関数名やコード内部の処理をそのまま操作として書かない。
- `.markharness/generated/testcases/*.yml` を手編集しないこと。これは派生出力である。`.markharness/knowledge/` 配下のみを(`apply` 経由で)書き、残りは `markharness generate` に生成させる。
- 大きなドラフト1つより、小さなドラフト(Scenario ごとに1つ)を複数作る方を優先する — 検証と修正が段階的に行いやすい。
- Scenario の結果がコードだけでは完全に決まらない外部状態(I/O・並行性・設定)に依存する場合は、単一の決定的な結果を断定するのではなく、その旨を description に記載する。
- `scenario.phases` の配列の並び順は表示上の整列ではなく、生成される TestCase の `phases` の実行順序そのものを決める契約(旧版はファイル名順だったが、現在は配列順が正本) — 並び替えるとテストの意味が変わる。同じ操作後の複数の観測結果は1つの phase の `results` にまとめ、新しい phase は新しい操作を伴う局面にのみ作る。
- `scenario.id` は Behavior 内で一意であればよい(`knowledge apply`/`knowledge validate` が検証する)。`generate` の出力先は `.markharness/knowledge/features/` と同じ階層(`.markharness/generated/testcases/<feature>/<behavior>/<scenario>.yml`、Feature がトップレベル)にフルミラーされるため、別の Behavior で同じ `scenario.id` を再利用しても衝突しない。
- 新規 Requirement を作成した直後は Feature からまだ参照できない — `markharness identity migrate` を実行して uid を発行するまでの間、`requirement_not_migrated` で拒否される(Phase 4 参照)。この migrate は冪等なので、繰り返し実行しても害はない。
