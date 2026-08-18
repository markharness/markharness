# Knowledge From Code (standalone)

> このファイルは単体で完結しています — このファイル1つだけを任意のプロジェクトにコピーする(あるいは本文をそのまま AI コーディングアシスタントとのチャットに貼り付ける)だけで動作し、`markharness` テンプレートリポジトリの他のファイルを一切必要としません。唯一の外部依存は `markharness` CLI 自体です(Phase 0 を参照)。
>
> ユーザーがチャットで使っている言語で応答してください。

既存のソースコードを読み、そこから `markharness` のテスト知識を導出し、`markharness generate` で `TestCase` データを生成します。これはテスト知識(`knowledge/**/{feature,condition,expected/*}.yaml`)の AI 支援による作成であり、人間によるレビューの代替ではありません。導出した Condition/ExpectedResult はすべて、コード中の具体的な箇所に遡れる必要があります。

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
- すべてのステップが完了したら `## Summary` セクションを追加する(Phase 5 参照)。

## データモデル(参考 — 他のファイルは不要)

```text
knowledge/
└── <requirement>/
    ├── requirement.yml
    └── <feature>/
        ├── feature.yml       # requirement: <requirement>
        └── <behavior>/
            ├── behavior.yml
            └── <condition>/
                ├── condition.yml
                └── expected/
                    └── 001.yml, 002.yml, ...

axes/                  # 横断的な axis レジストリ、axis 1つにつき axes/<id>.yml 1ファイル
generated/testcases/   # <requirement>/<feature>/<behavior>/<condition>.yml、`markharness generate` により決定的に再生成される — 手編集禁止
```

## 手順

### Phase 0 — ツールが使えることを確認する

1. `markharness --version`(または `--help`)を実行する。コマンドが見つからない場合は処理を止め、ユーザーに次を伝える: まず `markharness` をビルド/インストールする必要がある(そのリポジトリから `cargo install --path .`、またはビルド済みバイナリを使う)、あるいは本当に別のツールを意図していないか確認する。出力を捏造したり、確認なしに先へ進んだりしないこと。
2. 対象ディレクトリに `knowledge/`・`axes/`・`schema/` が既に存在するか(=そこで `markharness init` が実行済みか)を確認する。いずれか欠けていれば、先に `markharness init --dir <target>` を実行する — 必要なサブディレクトリと、`markharness validate` が必要とするデフォルトの `schema/*.schema.json`、プロジェクトルート目印 `.markharness.toml` を作成する(既存のものには一切手を加えない)。
3. `init` 済みのプロジェクト配下(`.markharness.toml` が祖先ディレクトリにあるところ)であれば、以降の各コマンドは `--dir` を省略してもそこまで遡ってプロジェクトルートを自動検出する。複数プロジェクトを並行して扱う場合や、カレントディレクトリがプロジェクト外の場合は明示的に `--dir <target>` を指定する。
4. 全くの新規プロジェクトでは `axes/` が空なので、使う予定のある axis は*すべて*、ドラフト作成前に(Phase 2 で)新規作成する必要がある — 頼れる既存レジストリは存在しない。

### Phase 1 — スコープ確認

1. 対象コード(ユーザーが指定したファイル/モジュール/関数、不明なら質問する)を特定する。
2. このコードがどの `requirement` に属するかを確認する(既存の `knowledge/<requirement>/` id、または新規作成 — 新規プロジェクトでは常に新規となる)。
3. 上で作成したチェックリストファイルに、抽出予定の Behavior 1つにつき1行を記入する。

### Phase 2 — コードを分析する

対象コードを読み、公開関数/分岐/エラーパスごとに次を特定する:

- **Feature**: そのコードが実装しているユーザー向けの機能。
- **Behavior**: Feature が行う個別の1つのこと(例: 1つの関数、またはその中の1つの責務)。
- **Condition**: 結果を変える入力/状態の組み合わせ(分岐・エッジケース・エラーパスもすべて対象)。
- **ExpectedResult**: その Condition における観測可能な結果 — 実際のコード(戻り値・エラー・副作用)から読み取る。推測しない。

コードが示していない振る舞いを創作しないこと。意図が曖昧な場合(`TODO` や、分岐の解釈が複数あり得る場合など)は、推測せず処理を止めてユーザーに確認する。

### Phase 3 — axis を確認する

1. `markharness axes list --json` を実行し、登録済みの `axes/*.yml` エントリを確認する。
2. *新規*の requirement/feature/behavior で使う予定の `axis` 値はすべて事前に登録されている必要がある(`markharness knowledge validate` は未登録の axis を `unknown_axis` として拒否し、最も近い候補を提示する)。必要な axis が存在しない場合は、続行前に `markharness axes add <id> [--label <label>]` で登録する(`--label` 省略時は `id` がそのまま label になる。既に存在する id を指定するとエラーになる)。

### Phase 4 — ドラフト作成・検証・適用(Condition ごとに繰り返す)

Phase 2 で特定した各 Condition について:

1. `KnowledgeDraft` スキーマに合致するドラフト YAML ファイルを(例えばスクラッチパスに)書く。空の雛形は `markharness knowledge scaffold`(stdout に出力、`--out <path>` でファイル出力も可 — 既存ファイルは上書きしない)で取得できる。IDE 補完用に `docs/knowledge_draft.schema.json` という参考スキーマも用意されている(あくまで参考用 — 「既存エントリなら label/axis/description は省略可」「axis は `axes/` に登録済みである必要がある」といった状態依存のルールはプレーンな JSON Schema では表現できないため、実際のチェックは常に `knowledge validate`/`apply` が行う):

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

   condition:
     id:
       <condition-slug> # behavior id をプレフィックスとして繰り返さない
       # 同じ Behavior 内でのみ一意であればよい —
       # 詳細は下記「Condition id の一意性」参照
     label: <label>
     description: <このパスを引き起こす具体的な入力/状態 — ファイルパスと関数/メソッド名で出所を明記(行番号は書かない。理由は「原則」参照)>

   expected:
     - description: <観測可能な結果。コードから読み取る — ファイルパスと関数/メソッド名で出所を明記(行番号は書かない)>
   ```

   既に存在し変更のない階層では `label`/`axis`/`description` を省略する — 矛盾する値を渡すと `conflicting_existing_value` で検証に失敗する。

   **Condition id の一意性:** `markharness generate` は各 Condition を `generated/testcases/<requirement>/<feature>/<behavior>/<condition-id>.yml` に書き出す — `knowledge/` と全く同じ階層をそのままミラーする。そのため `condition.id` は同じ Behavior 内でのみ一意であればよく(`markharness knowledge validate`/`apply` もその範囲でのみ一意性を検証する)、異なる Behavior で同じ id を再利用しても(例: `add-todo` と `edit-todo` の両方で `valid-title` を使う等)出力が衝突することはない。id をリネームしたり衝突を避けたりする作業は不要 — 詳細は末尾の「原則」を参照。

2. 検証: `markharness knowledge validate <draft-file> --json`。報告されたエラー(`invalid_slug`、`missing_axis`、`missing_description`、`unknown_axis`、`redundant_prefix`、`conflicting_existing_value`、`parent_not_found`、`unknown_forked_from`)をすべて解消してから次に進む。
3. 適用: `markharness knowledge apply <draft-file> --json`(`--strip-redundant-prefix` は、意図的に `behavior-` プレフィックス付きの `condition.id` を剥がしたい場合のみ追加する)。
4. 対応するチェックリストのステップを完了にする。

複数の Condition をまとめて処理する場合は、ドラフトファイルを1つのディレクトリに集め、`markharness knowledge validate --batch <dir> --json`(または同じチェックを行う `markharness knowledge apply --batch <dir> --dry-run --json`)で一括検証してから `markharness knowledge apply --batch <dir> --json` を実行してもよい。`--batch <dir>` は直下の `*.yml` ファイルのみを対象にする(`.yaml` 拡張子のファイルは無視され、該当ファイルが1つもなければ exit code 2 で失敗する)。ファイルはディレクトリ内のファイル名順に適用され、後続のドラフトは同じバッチ内で先行するドラフトが作成した Requirement/Feature/Behavior を参照できる(例: `01-xxx.yml`、`02-xxx.yml` のように命名して順序を制御する)。`apply --batch`(`--dry-run` なし)が途中のファイルで失敗した場合、その回の呼び出しで書き込み済みのファイルも含めてすべてロールバックされる(バッチ全体が不可分)。

### Phase 5 — 生成

計画していたすべての Condition を適用し終えたら:

1. `markharness generate` を実行し、`knowledge/` から `generated/testcases/*.yml` を決定的に(再)生成する。
2. 生成されたファイル数が、適用した Condition の数と一致することを確認する(例: `find knowledge -name condition.yml | wc -l` と `find generated/testcases -type f -name "*.yml" | wc -l` を比較 — `generated/testcases/` は `knowledge/` と同じ階層にミラーされるため、`-maxdepth 1` は付けずに再帰的に数える)。数が一致しない場合は、同じ requirement/feature/behavior/condition の組み合わせに誤って複数回 apply していないか等、意図しない重複を確認する。
3. `markharness validate` を実行し、`knowledge/`/`axes/` が引き続き `schema/*.schema.json` に準拠し、相互参照が解決することを確認する。
4. もう一度 `markharness generate` を実行し、差分が出ないことを確認する — これは多くの CI 設定がチェックする内容なので、引き渡す前にローカルで確認しておく。

### Phase 6 — 完了処理

1. チェックリストファイルに `## Summary` セクションを追加する: どの Feature/Behavior/Condition を追加したか、それぞれがどのコード(ファイルパス・関数/メソッド名)に遡れるか。行番号は挙げない(理由は「原則」参照)。
2. どの `generated/testcases/*.yml` が新規/変更されたかをユーザーに報告し、コードの意図が曖昧でスキップせざるを得なかった箇所があればそれも伝える。

## 原則

- すべての Condition/ExpectedResult は、その出所となったコードを明記すること — 推測によるテスト知識は作らない。ただし出所の明記は「ファイルパス + 関数/メソッド/分岐名」までにとどめ、行番号のようにリファクタリングや無関係な変更で頻繁にズレる情報は description に含めない — 記載しても実装が変わるたびに陳腐化し、`knowledge/` の維持コストを増やすだけで検証可能性を高めない。
- `generated/testcases/*.yml` を手編集しないこと。これは派生出力である。`knowledge/` 配下のみを(`apply` 経由で)書き、残りは `markharness generate` に生成させる。
- 大きなドラフト1つより、小さなドラフト(Condition ごとに1つ)を複数作る方を優先する — 検証と修正が段階的に行いやすい。
- Condition の結果がコードだけでは完全に決まらない外部状態(I/O・並行性・設定)に依存する場合は、単一の決定的な結果を断定するのではなく、その旨を description に記載する。
- `condition.id` は Behavior 内で一意であればよい(`knowledge apply`/`knowledge validate` が検証する)。`generate` の出力先は `knowledge/` と同じ階層(`generated/testcases/<requirement>/<feature>/<behavior>/<condition>.yml`)にフルミラーされるため、別の Behavior で同じ `condition.id` を再利用しても衝突しない(旧版はフラットな `generated/testcases/<condition-id>.yml` に出力しており、Behavior をまたいだ再利用時に黙って上書きされる欠陥があったが、構造的に解消済み)。
