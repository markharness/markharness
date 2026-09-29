# 0036: StrictDocのJSONエクスポートからKnowledge Intentを生成する`knowledge intent-from-strictdoc`

## ステータス

Accepted(2026-09-29決定、実装済み)。

## 背景

[0023](0023-requirement-native-and-external-source.md)は`source: external`のRequirementを定義し、[0030](0030-external-requirement-source-key.md)は外部側の識別子`source_key`(StrictDocのMIDを推奨)を追加した。しかし、既存のStrictDocプロジェクトの要求をこれらのRequirementとして登録する手段はCLIに無く、`knowledge reconcile`のIntentを要求1件ずつ人手で書くしかない(issue #88。実プロジェクトでは358件)。`import`の入力は`native`・`junit`のみで、knowledgeには書き込まず、常にCanonicalSnapshotを出力する。

StrictDocの`strictdoc export --formats=json`は、プロジェクト全体を`out/json/index.json`の1ファイルに出力する(strictdoc 0.30.1で確認)。`DOCUMENTS[]`の下に`SECTION`と要求が`NODES`で入れ子になり、要求ノードは`_NODE_TYPE: "REQUIREMENT"`とMIDを持つ。**どのノードにも所属する`.sdoc`のパスは含まれない**。また、MIDが出力されるのは`.sdoc`にMIDが永続化されているか、ドキュメントが`ENABLE_MID: True`のときだけで、後者で`.sdoc`にMIDが書かれていない場合、exportの実行ごとに別のMIDが生成される。

## 決定

### 1. `knowledge intent-from-strictdoc`はIntent YAMLをstdoutに出力するだけで、何も書き込まない

```
markharness knowledge intent-from-strictdoc --input out/json/index.json [--sdoc-root <dir>] [-d <dir>] \
  | markharness knowledge reconcile -
```

knowledgeへの書き込みは既存の`knowledge reconcile`に任せる。これによりatomicな書き込み・`--check`・`id`基準の冪等性が再実装なしで得られる。`import`は「常にCanonicalSnapshotを出力する」性質を保つため、`import --source strictdoc`にはしない。

### 2. 入力はStrictDocのJSONエクスポートファイルのみ

markharnessは`strictdoc`を実行せず、`.sdoc`の要求本文もパースしない(設計書P5: Coreは外部形式を知らない)。利用者が`strictdoc export`を実行し、出力ファイルを`--input`で渡す。

### 3. 取り込み対象と生成するフィールド

`DOCUMENTS[].NODES`を再帰的にたどり、`_NODE_TYPE == "REQUIREMENT"`のノードだけを対象とする(`SECTION`は入れ子をたどるだけ。カスタムgrammarの型は対象外)。各要求から次のRequirementを生成する。

| フィールド | 値 |
|---|---|
| `id` | `sd-<MID全体>`(MIDは小文字32桁hexで有効なslug) |
| `source` | `external` |
| `source_key` | MID(0030の推奨どおり) |
| `source_locator` | §4 |
| `source_revision` | `current` |
| `axis` | `[]` |

### 4. `source_locator`は`.sdoc`ヘッダのMIDとの照合で決める

JSONに`.sdoc`のパスが無いため、`--sdoc-root`(省略時はプロジェクトルート)以下の`*.sdoc`を走査し、各ファイルのヘッダの`MID:`行を読んで、ドキュメントの`MID`と一致するファイルを探す。要求の本文はパースしない。`source_locator`はプロジェクトルート基準の相対パスで、区切りは`/`とする(`knowledge reconcile`が`git hash-object`をプロジェクトルートで実行するのと同じ基準)。`--sdoc-root`はプロジェクトルート配下でなければならない。

### 5. 不整合は全体を拒否する

次のいずれかがあれば、診断を出力して非0で終了し、Intentは何も出力しない。

- 対象の要求ノードにMIDが無い。
- 要求を含むドキュメントにMIDが無い。
- ドキュメントのMIDに一致する`.sdoc`が0件、または複数件ある。
- `--sdoc-root`がプロジェクトルートの外にある。

### 6. 利用者が満たすべき前提

`.sdoc`にMIDが永続化されていること(要求ノードもドキュメントも)。`ENABLE_MID: True`だけでMIDがexportのたびに生成される状態では、`id`・`source_key`が実行ごとに変わり、再実行のたびに別のRequirementが作られる。ドキュメントのMIDが`.sdoc`のヘッダに無い場合は§4の照合で検出してエラーにするが、要求ノードのMIDが永続化されているかは検証しない。`source_revision: current`の解決に必要な、`.sdoc`のコミットも利用者が済ませておく(未コミットなら`knowledge reconcile`が失敗する。ADR 0029)。

## 不変条件

- このコマンドはknowledgeにもファイルシステムにも書き込まない。
- 出力されるIntentの`id`は入力のMIDだけから決まり、実行順序や件数に依存しない。
- 1件でも不整合があれば、部分的なIntentは出力されない。

## 影響

- `src/knowledge_strictdoc.rs`(新規): JSON+`.sdoc`ヘッダ索引→Intentの変換。
- `src/cli.rs`: `KnowledgeCommand::IntentFromStrictdoc`。
- `tests/knowledge_intent_from_strictdoc_cli.rs`(新規)。

## 検討し、採用しなかった選択肢

- **`import --source strictdoc`**: `import`の出力(CanonicalSnapshot)と`--bind`・`--format`の意味が崩れるため不採用。
- **`knowledge`配下で直接書き込む専用コマンド**: `knowledge reconcile`の書き込み経路と重複するため不採用。
- **`strictdoc`をサブプロセスとして実行する**: 外部ツール実行と環境差(Windows等)を持ち込むため不採用。
- **自前の`.sdoc`パーサ**: ヘッダの`MID:`行以外は不要なため不採用。
- **`source_key`による重複検出・更新・削除検出**: 0030 §3のスコープ外を維持する(YAGNI)。`id`がMIDから決定的なので、`id`基準の冪等性で再実行は成立する。
- **`intent --source <name>`のような汎用化**: 入力形式が1つのうちは不要(設計書§9.2)。2つ目の形式が来た時点で改名を検討する。
- **`TITLE`照合・全件同一`--source-locator`・利用者による対応表**: TITLEは重複しやすく、複数`.sdoc`のプロジェクトでは不正確になる(0023の変更検出は`.sdoc`単位のblob差分)ため不採用。
