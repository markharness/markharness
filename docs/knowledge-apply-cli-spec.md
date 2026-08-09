# 仕様書: `markharness knowledge validate` / `apply` — 非対話ナレッジ登録コマンド

Status: Implemented(`src/knowledge_draft.rs` / `src/knowledge_apply.rs` / `src/cli.rs`)。本仕様に対する実装時の追加・変更点は各節末の「実装時の追記」を参照。
Created: 2026-08-08
関連ドキュメント: `docs/testcase-generation-design.md`, `docs/product-operation.md`, `src/interactive.rs`, `src/knowledge.rs`, `src/cli.rs`

## 1. 背景・目的

現行の `markharness knowledge add` は TTY 上での逐次プロンプト(stdin一行ずつ)を前提にしており、以下の制約を持つ。

- Requirement → Feature → Behavior → Condition → ExpectedResult の5階層を一方向に進み、確認済みの階層から**都度ファイルに書き込む**(後戻り不可、部分書き込みが残り得る)。
- 番号選択(既存流用)と新規id/ラベル入力が同一プロンプトに混在し、選択肢の存在がヘルプテキストに明示されない。
- Condition idの冗長接頭辞除去(`strip_redundant_condition_prefix`)を確認なしに実行する。
- axis(横断的観点)の値を `axes/*.yml` レジストリと照合しない。

これに加えて、本コマンドは今後 **Claude Code 等のAIエージェントによる非対話呼び出し**および**将来のGUI実装**からも利用される想定である。両者はいずれも「まとまった入力を一括で検証・確定する」操作モデルを必要とし、TTY越しの逐次プロンプトには依存できない。

本仕様は、既存の対話型 `knowledge add` はそのまま残しつつ、その内部ロジック(候補列挙・バリデーション・書き込み)を **TTY非依存の `validate` / `apply` サブコマンド**として切り出すための設計を定義する。人間の対話CLI・AIエージェント・将来のGUIバックエンドは、いずれもこの `validate` / `apply` を共通の実行エンジンとして利用する。

## 2. スコープ

対象:

- `markharness knowledge validate <draft-file>` (新規)
- `markharness knowledge apply <draft-file> [flags]` (新規)
- ドラフトYAMLのスキーマ定義
- 機械可読なエラー出力フォーマット
- axisレジストリ照合ルール
- Condition id冗長接頭辞の扱い変更

非対象(本仕様では扱わない):

- `$EDITOR` を起動する `knowledge add --edit` の実装(本仕様のAPIを呼ぶラッパーとして別途設計。本書ではインターフェース要件のみ §9.3 に記載)
- 既存 `knowledge add`(逐次プロンプト版)の削除・置換。当面は維持する。
- GUIそのものの実装。

## 3. コマンド仕様

### 3.1 `markharness knowledge validate <draft-file>`

副作用なし。ドラフトファイルを読み込み、スキーマ・整合性を検証し、結果のみを返す。

```
markharness knowledge validate <draft-file> [--json] [-d, --dir <path>]
```

| 引数/フラグ | 必須 | 説明 |
|---|---|---|
| `<draft-file>` | ○ | ドラフトYAMLファイルのパス |
| `-d, --dir <path>` | - | 対象プロジェクトルート(`knowledge/`の親)。省略時はカレントディレクトリ |
| `--json` | - | エラー・結果をJSON1行で出力(§6参照)。省略時は人間可読テキスト |

終了コード: §3.4 の表に従う。ファイルへの書き込みは一切行わない。

### 3.2 `markharness knowledge apply <draft-file> [flags]`

ドラフトを検証し、問題がなければ `knowledge/` 配下にファイルを**アトミックに**書き込む。

```
markharness knowledge apply <draft-file> [--json] [-d, --dir <path>] [--strip-redundant-prefix] [--dry-run]
```

| フラグ | 説明 |
|---|---|
| `--json` | §3.1と同様 |
| `--dir` | §3.1と同様 |
| `--strip-redundant-prefix` | Condition idがBehavior idと重複接頭辞を持つ場合、確認なしで除去したidを採用する。指定なしの場合はバリデーションエラーとして停止し、除去後の候補idをエラー内に提示する(§7)。 |
| `--dry-run` | `validate` と同義(バリデーションのみ行い書き込まない)。CI等での用途を想定した別名。 |

**アトミック性の要件**: 5階層(Requirement〜ExpectedResult)のうち一部だけを新規作成する場合でも、バリデーションが全て通過した後にまとめて書き込む。書き込み中にI/Oエラーが発生した場合、可能な限り一時ファイル+リネームで各ファイルを書き、失敗時は成功済みファイルも含めてロールバック(削除)する。少なくとも「バリデーションエラーで一部だけ書き込まれる」状態は発生させない。

### 3.3 `markharness knowledge add --edit`(参考・別紙で詳細化)

人間向けの薄いラッパー。テンプレートドラフトを一時ファイルに生成 → `$EDITOR` 起動 → 保存後に内部で `apply` 相当の処理を呼ぶ。バリデーションエラー時はエディタを再度開いて修正させる。本仕様のAPI(§9.2の`apply_draft`関数)を呼び出す前提でインターフェースを設計し、実装は別チケットとする。

### 3.4 終了コード

| コード | 意味 |
|---|---|
| 0 | 成功(validate: エラーなし、apply: 書き込み成功) |
| 1 | バリデーションエラーあり(§6のエラーリストをstderrに出力) |
| 2 | 使用方法エラー(ファイル不在、YAMLパース不能、フラグ不正) |
| 3 | ファイルシステムエラー(書き込み失敗など。apply専用) |

## 4. ドラフトファイルのYAMLスキーマ

`knowledge.rs` の既存構造体(`Requirement`/`Feature`/`Behavior`/`Condition`/`ExpectedResult`)と1対1対応する木構造。1回の `apply` で1本のチェーン(Requirement→Feature→Behavior→Condition→複数ExpectedResult)を登録する。複数チェーンの一括登録は非スコープ(§10「非対応」参照)。

```yaml
requirement:
  id: controls              # 必須。ASCII slug ([a-z0-9-]+)
  label: controls           # 省略可。省略時は id を label として使う
  axis: [gameplay]          # 新規作成時は必須。既存reuse時は省略可(下記「既存id再利用」参照)
  description: null         # 省略可

feature:
  id: player-jump
  label: player-jump
  axis: [gameplay, animation]

behavior:
  id: jump
  label: jump
  axis: [gameplay]
  description: Player presses jump.   # 必須(Behaviorのみdescriptionが必須項目、既存スキーマ通り)

condition:
  id: ground
  label: ground
  description: Jump from the ground and land

expected:
  - description: lands safely
  - description: takes fall damage if height > 3m
```

フィールド仕様は `src/knowledge.rs` の各構造体定義に準拠する(`id`/`label`/`axis`/`description`の型・必須有無は現行struct通り)。`expected` のみ配列(1回のapplyで複数のExpectedResultを追加可能。既存の連番採番ロジック `{condition_id}-{seq:03}` を踏襲)。

## 5. バリデーションルール一覧

各ルールは §6 のエラーコードに対応する。

| # | 対象 | ルール | エラーコード |
|---|---|---|---|
| 1 | 全id | `is_valid_slug` を満たすこと(小文字英数字とハイフンのみ) | `invalid_slug` |
| 2 | requirement/feature/behavior | 新規作成時は `axis` が1件以上必要 | `missing_axis` |
| 3 | behavior | `description` が空でないこと | `missing_description` |
| 4 | condition | `description` が空でないこと | `missing_description` |
| 5 | expected[] | `description` が空でないこと(各要素) | `missing_description` |
| 6 | axis全般 | 値が `axes/*.yml` に登録済みのidと一致すること(§8) | `unknown_axis` |
| 7 | condition.id | Behavior idとの重複接頭辞(`{behavior_id}-`)を持つ場合、`--strip-redundant-prefix` 未指定なら停止(§7) | `redundant_prefix` |
| 8 | 既存id再利用時 | 提供された `axis`/`description`/`label` が既存ファイルの値と一致しない場合(§10.2) | `conflicting_existing_value` |
| 9 | requirement/feature/behavior/condition | 親参照(例: feature.requirement)が実在すること。ドラフト内で新規作成する場合はドラフト自身の値と整合していること | `parent_not_found` |
| 10 | feature.forked_from | 値が指定されている場合、`knowledge/`配下のいずれかのFeatureの`id`と一致すること(実装時追加。論文§3.1の`forked_from`、本仕様の初版では未記載) | `unknown_forked_from` |

**実装時の追記**：ルール#10(`unknown_forked_from`)は本仕様の初版になかったが、`feature.forked_from`フィールド(`knowledge.rs`)の実装にあわせて`knowledge_draft.rs::feature_id_exists`で追加された。Feature idは`requirement`配下にネストしていてもリポジトリ全体で一意である前提のため、`knowledge/`配下を`requirement`階層をまたいで全探索する。

## 6. エラー出力フォーマット

**人間可読(デフォルト)**: `stderr` に1行1エラー。

```
error: unknown_axis: axis "validdation" is not registered (path=behavior.axis[0])
error: redundant_prefix: condition.id "jump-ground" starts with behavior.id "jump-" (suggested="ground", path=condition.id)
```

**機械可読(`--json`)**: `stdout` にエラーオブジェクトの配列をJSON1行で出力(パイプ・エージェント解析用)。

```json
{
  "ok": false,
  "errors": [
    {
      "code": "unknown_axis",
      "path": "behavior.axis[0]",
      "value": "validdation",
      "message": "axis \"validdation\" is not registered",
      "suggestion": "validation"
    },
    {
      "code": "redundant_prefix",
      "path": "condition.id",
      "value": "jump-ground",
      "message": "condition.id starts with behavior.id prefix",
      "suggestion": "ground"
    }
  ]
}
```

`suggestion` フィールドは可能な場合のみ設定(近似候補や補正案)。エージェントはこれを使って自動リトライを組める。成功時は `{"ok": true, "written": ["knowledge/controls/player-jump/jump/ground/expected/002.yml", ...]}` を出力する(applyのみ)。

## 7. Condition id冗長接頭辞の扱い

現行の対話CLI(`interactive.rs`)は確認なしに接頭辞を除去して書き込む。`apply` ではこれをデフォルト無効化する。

- `--strip-redundant-prefix` 未指定: `redundant_prefix` エラーで停止(§6の形式で `suggestion` に除去後idを提示)。呼び出し側(人間/エージェント)が `condition.id` を修正するか、フラグを付けて再実行する。
- `--strip-redundant-prefix` 指定: 現行ロジック(`strip_redundant_condition_prefix`)をそのまま適用し、除去後のidで書き込む。警告メッセージは出力するが停止はしない。
- 既存ディレクトリが冗長接頭辞付きの名前(レガシーデータ)で既に存在する場合は、現行の対話CLIと同じく「既存優先で除去しない」(`legacy_condition_dir_with_redundant_prefix_is_reused_without_stripping` テストの挙動を踏襲)。

## 8. axisレジストリ照合

`axes/<id>.yml`(`id`/`label`/`description`)を起動時に読み込み、`requirement.axis`/`feature.axis`/`behavior.axis` の各値が登録済みidと完全一致することを要求する。

- 未登録axisは `unknown_axis` エラーで停止する(警告に留めない。エージェント駆動では「気づかず通過」の方が「axisレジストリを更新すべき」と気づける方より害が大きいため)。
- `markharness axes list [--json]` コマンドを実装済み(`docs/cli-manual.md` 1.7節)。エージェントはこれで事前にaxis一覧を取得できる。

## 9. 内部アーキテクチャ

### 9.1 モジュール構成案

```
src/
├── knowledge.rs        # 既存。構造体・parse/serialize・slug関連ユーティリティ(変更なし)
├── knowledge_draft.rs  # 新規。KnowledgeDraft構造体、YAMLパース、validate()
├── knowledge_apply.rs  # 新規。apply_draft()(検証+アトミック書き込み)
├── interactive.rs      # 既存。将来的にknowledge_draft::KnowledgeDraftを組み立ててknowledge_apply::apply_draft()を呼ぶ形にリファクタ(本仕様の直接スコープ外、§10参照)
└── cli.rs              # knowledge validate / knowledge apply サブコマンドを追加
```

### 9.2 中核関数シグネチャ(案)

```rust
// knowledge_draft.rs
pub struct KnowledgeDraft {
    pub requirement: RequirementDraft,
    pub feature: FeatureDraft,
    pub behavior: BehaviorDraft,
    pub condition: ConditionDraft,
    pub expected: Vec<ExpectedDraft>,
}

pub struct ValidationError {
    pub code: ValidationErrorCode,
    pub path: String,
    pub value: Option<String>,
    pub message: String,
    pub suggestion: Option<String>,
}

pub fn parse_draft(yaml: &str) -> Result<KnowledgeDraft, DraftParseError>;

pub fn validate_draft(
    root: &Path,
    draft: &KnowledgeDraft,
    options: &ValidateOptions, // strip_redundant_prefix: bool
) -> Vec<ValidationError>;

// knowledge_apply.rs
pub struct ApplyResult {
    pub written_paths: Vec<PathBuf>,
}

pub fn apply_draft(
    root: &Path,
    draft: &KnowledgeDraft,
    options: &ApplyOptions, // strip_redundant_prefix: bool
) -> Result<ApplyResult, ApplyError>; // 内部でvalidate_draftを呼び、エラーがあれば書き込み前に中断
```

`validate_draft` はTTY・stdin/stdoutに一切依存しない純粋関数とする(現状の `interactive.rs` にある `prompt_*` 関数群とは完全に分離する)。これにより `validate` サブコマンド・`apply` サブコマンド・将来の `add --edit` ラッパー・将来のGUIバックエンドが同一関数を共有できる。

### 9.3 `add --edit` との関係(参考)

`add --edit` は以下の擬似コードで実装される想定(別チケット)。

```
draft = generate_template_draft(root)  // 既存候補一覧をコメント付きで埋め込んだYAML文字列
tmp_file = write_temp(draft)
loop:
    open_editor(tmp_file)
    parsed = parse_draft(read(tmp_file))
    errors = validate_draft(root, parsed, options)
    if errors.is_empty():
        apply_draft(root, parsed, options)
        break
    else:
        print_errors_as_comments_in(tmp_file, errors)  // 再編集を促す
```

## 10. 既存コードとの対応・移行方針

| 既存要素 | 対応方針 |
|---|---|
| `interactive.rs::run_add` | 変更しない(本仕様は追加のみ)。将来的なリファクタで `knowledge_draft`/`knowledge_apply` を内部利用する形に置き換える別チケットを起票する。 |
| `strip_redundant_condition_prefix` (`knowledge.rs`) | シグネチャ変更なし。`apply`/`add --edit` から呼び出す形に共有する。 |
| `is_valid_slug` / `normalize_slug_candidate` / `romanize_label` | 共有。ドラフトのid自動提案(§9.3のテンプレート生成時)にも再利用する。 |
| `list_candidate_ids` (`interactive.rs`) | `knowledge_draft.rs` に移設し、`validate_draft` 内の既存id探索・§9「親参照実在チェック」に使う。 |

**非対応(明示的にスコープ外)**:

- 複数チェーンの一括登録(1ファイルで複数Feature/Behaviorを同時登録)は本仕様では扱わない。必要になった場合は別仕様として `expected` と同様に配列化を検討する。
- 既存ファイルの更新(labelやaxisの変更)は非対応。既存id再利用時は「一致するか検証するのみ」であり、書き換えは行わない(§5 ルール#8)。更新が必要な場合は別コマンド(`knowledge edit` 等)を別途設計する。

## 11. テスト計画

`interactive.rs` の既存テスト(FULL_INPUTベースのシナリオ群、`interactive.rs:290-846`)と対応させ、`knowledge_draft.rs`/`knowledge_apply.rs` に以下を実装する。

- 正常系: 新規5階層一括作成(既存 `creates_new_requirement_feature_behavior_condition_and_expected_from_scratch` 相当)
- 既存id再利用: 一致する値なら成功、不一致なら `conflicting_existing_value` エラー(新規テスト、既存対話CLIには相当機能なし)
- axis未登録: `unknown_axis` エラーが返ること(新規)
- 冗長接頭辞: フラグなしでエラー、フラグありで除去(既存 `auto_dedup_strips_redundant_condition_prefix_and_notifies` を非対話版として書き直し)
- レガシーディレクトリ優先(既存 `legacy_condition_dir_with_redundant_prefix_is_reused_without_stripping` 相当)
- アトミック性: バリデーション失敗時にファイルが一切書き込まれないこと(新規、`apply`のみ)
- `--json` 出力のスキーマ検証(新規)
- CLI統合テスト: `markharness knowledge validate`/`apply` の終了コードとstdout/stderrの検証(`cli.rs`に追加)

## 12. オープン事項(実装前に決定が必要) — 実装により解決済み

1. `expected` を配列にする現仕様と、既存の対話CLIが1回の実行で1件しかExpectedResultを作らない点との差異を許容するか。→ **解決**：許容する前提のまま実装(`knowledge_apply.rs`、`existing_expected_count`から連番を継続採番)。
2. `axes list` コマンドの追加を本チケットに含めるか、別チケットにするか。→ **解決**：`markharness axes list`として実装済み(§8参照)。
3. `conflicting_existing_value` の比較粒度(label/axis/descriptionの完全一致を要求するか、一部フィールド省略時は「未指定」として無視するか)。→ **解決**：仮採用方針の通り実装。`knowledge_draft.rs`の`push_conflicting_value`/`push_conflicting_axis`は`draft`側の該当フィールドが`Some`の場合のみ既存値と突合し、`None`(省略)の場合は比較自体を行わない。
