# 0036: StrictDocのJSONエクスポートからKnowledge Intentを生成する`knowledge intent-from-strictdoc`

## ステータス

Accepted(2026-09-29決定、実装済み)。

## 背景

[0023](0023-requirement-native-and-external-source.md)は`source: external`のRequirementを定義し、[0030](0030-external-requirement-source-key.md)は外部側の識別子`source_key`(StrictDocのMIDを推奨)を追加した。しかし、既存のStrictDocプロジェクトの要求をこれらのRequirementとして登録する手段はCLIに無く、`knowledge reconcile`のIntentを要求1件ずつ人手で書くしかない(issue #88。実プロジェクトでは358件)。`import`の入力は`native`・`junit`のみで、knowledgeには書き込まず、常にCanonicalSnapshotを出力する。

`strictdoc export --formats=json`は、プロジェクト全体を`out/json/index.json`の1ファイルに出力する(strictdoc 0.30.1で確認)。`DOCUMENTS[]`の下に`SECTION`と要求が`NODES`で入れ子になり、要求ノードは`_NODE_TYPE: "REQUIREMENT"`とMIDを持つ。次の点が設計に効く。

- **どのノードにも所属するソースファイルのパスは含まれない。**
- StrictDocは`.sdoc`とMarkdown形式(`.md`、`.markdown`)の両方をドキュメントの第一級の形式とする。Markdown形式には**ドキュメント単位のMIDが存在しない**(MIDは要求などのノードごとにだけ書ける)。JSON上もドキュメントのMIDは無い。
- MIDが出力されるのは、ソースにMIDが永続化されているか、`.sdoc`のドキュメントが`ENABLE_MID: True`のときだけである。後者でソースにMIDが書かれていない場合、exportの実行ごとに別のMIDが生成される。
- MIDは常に32桁とは限らない(StrictDoc自身のドキュメントに31桁のMIDがある)。
- 実プロジェクト(StrictDoc自身の358要求)では、`spec/`の2つのMarkdownドキュメントが51要求を持つ。また、`tests/`配下にフィクスチャとしてMIDを含むMarkdownが複製されている。

## 決定

### 1. `knowledge intent-from-strictdoc`はIntent YAMLをstdoutに出力するだけで、何も書き込まない

```
markharness knowledge intent-from-strictdoc --input out/json/index.json \
  [--sdoc-root <dir>]... [-d <dir>] | markharness knowledge reconcile -
```

knowledgeへの書き込みは既存の`knowledge reconcile`に任せる。これによりatomicな書き込み・`--check`・`id`基準の冪等性が再実装なしで得られる。`import`は「常にCanonicalSnapshotを出力する」性質を保つため、`import --source strictdoc`にはしない。

### 2. 入力はStrictDocのJSONエクスポートファイルのみ

markharnessは`strictdoc`を実行せず、要求の本文もパースしない(設計書P5: Coreは外部形式を知らない)。利用者が`strictdoc export`を実行し、出力ファイルを`--input`で渡す。

### 3. 取り込み対象と生成するフィールド

`DOCUMENTS[].NODES`を再帰的にたどり、`_NODE_TYPE == "REQUIREMENT"`のノードだけを対象とする(`SECTION`は入れ子をたどるだけ。カスタムgrammarの型は対象外)。各要求から次のRequirementを生成する。

| フィールド | 値 |
|---|---|
| `id` | `sd-<MID全体>`(MIDは小文字hexで、有効なslug) |
| `source` | `external` |
| `source_key` | MID(0030の推奨どおり) |
| `source_locator` | §4 |
| `source_revision` | `current` |
| `axis` | `[]` |

MIDは小文字hexで1桁以上でなければならない(桁数は検証しない)。Intentは文字列整形で組み立てるため、これ以外の値は`id`(slug)にもYAMLにも到達させない。

### 4. `source_locator`は要求MIDの索引で決める

JSONにソースファイルのパスが無く、Markdown形式にはドキュメントMIDも無いため、**要求自身のMID**から所属ファイルを引く。`--sdoc-root`(複数回指定可。省略時はプロジェクトルート)以下の`*.sdoc`・`*.md`・`*.markdown`を走査し、MIDを宣言する行を索引にする。

- `.sdoc`は**行頭の**`MID: <値>`。
- Markdownは**行頭の**`**MID**: <値>`(行末の` \`は許す)。
- インデントされた行は無視する。StrictDoc自身のユーザーガイドが、`[REQUIREMENT]`ブロックをテキストノード内のコード例として引用しているため。
- MIDの宣言が意味を持たない、明確なリテラル領域は走査から除く。`.sdoc`の複数行文字列(`FIELD: >>>`から`<<<`まで。インデントされず、要求ブロックの引用が行頭に来る)と、Markdownのコードフェンス(3つ以上のバッククォートまたはチルダで囲まれた領域。閉じられなければファイル末尾まで)である。
- MIDとして読むのは小文字hexの値だけで、それ以外の値の行は索引に入れない。

要求のMIDに一致する行がちょうど1ファイルにあれば、そのパスを`source_locator`とする。パスはプロジェクトルート基準の相対パスで、区切りは`/`(`knowledge reconcile`が`git hash-object`をプロジェクトルートで実行するのと同じ基準)。`--sdoc-root`はプロジェクトルート配下でなければならない。

この走査はStrictDocを解釈しない。要求の構文やノード構造は読まず、MIDを宣言する行と、上記のリテラル領域の境界だけを判定する。

### 5. 不整合は全体を拒否する

次のいずれかがあれば、診断を出力して非0で終了し、Intentは何も出力しない。

- 対象の要求ノードにMIDが無い、または小文字hexでない。
- 要求のMIDが、走査範囲のどのファイルにも宣言されていない。
- 要求のMIDが、複数のファイルに宣言されている(エラーは該当ファイルを列挙し、`--sdoc-root`での絞り込みを案内する)。
- `--sdoc-root`がプロジェクトルートの外にある。

### 6. 利用者が満たすべき前提

- ソースにMIDが永続化されていること。`ENABLE_MID: True`だけでMIDがexportのたびに生成される状態では、`id`・`source_key`が実行ごとに変わり、再実行のたびに別のRequirementが作られる。索引に一致する行が無いため、この状態は§5の「どのファイルにも宣言されていない」で検出される。
- MIDを含む複製(テスト用フィクスチャなど)がある場合は、`--sdoc-root`でドキュメントのディレクトリだけを指定する(StrictDocの`include_doc_paths`に相当)。
- `source_revision: current`の解決に必要な、ソースファイルのコミット(未コミットなら`knowledge reconcile`が失敗する。ADR 0029)。

## 不変条件

- このコマンドはknowledgeにもファイルシステムにも書き込まない。
- 出力されるIntentの`id`は入力のMIDだけから決まり、実行順序や件数に依存しない。
- 1件でも不整合があれば、部分的なIntentは出力されない。

## 影響

- `src/knowledge_strictdoc.rs`(新規): JSON+MID索引→Intentの変換。
- `src/cli.rs`: `KnowledgeCommand::IntentFromStrictdoc`。
- `tests/knowledge_intent_from_strictdoc_cli.rs`(新規)。
- [0023](0023-requirement-native-and-external-source.md)の`source_locator`をStrictDocソースファイル(`.sdoc`/`.md`/`.markdown`)へ一般化し、`validate`・reconcile・traceabilityの該当メッセージを合わせる。

## 検討し、採用しなかった選択肢

- **`import --source strictdoc`**: `import`の出力(CanonicalSnapshot)と`--bind`・`--format`の意味が崩れるため不採用。
- **`knowledge`配下で直接書き込む専用コマンド**: `knowledge reconcile`の書き込み経路と重複するため不採用。
- **`strictdoc`をサブプロセスとして実行する**: 外部ツール実行と環境差(Windows等)を持ち込むため不採用。
- **自前の`.sdoc`/Markdownパーサ**: MIDを宣言する行以外は不要なため不採用。
- **ドキュメントMIDと`.sdoc`ヘッダの照合**: 当初案。Markdown形式にドキュメントMIDが無く、Markdownドキュメントを扱えないため不採用。
- **`TITLE`照合・全件同一`--source-locator`・利用者による対応表**: TITLEは重複しやすく、複数ファイルのプロジェクトでは不正確になる(0023の変更検出はソースファイル単位のblob差分)ため不採用。
- **`--exclude`による除外指定**: 想定外の複製を拾わない安全側として、走査範囲を明示する`--sdoc-root`(複数指定可)を採る。
- **`source_key`による重複検出・更新・削除検出**: 0030 §3のスコープ外を維持する(YAGNI)。`id`がMIDから決定的なので、`id`基準の冪等性で再実行は成立する。
- **`intent --source <name>`のような汎用化**: 入力形式が1つのうちは不要(設計書§9.2)。2つ目の形式が来た時点で改名を検討する。
