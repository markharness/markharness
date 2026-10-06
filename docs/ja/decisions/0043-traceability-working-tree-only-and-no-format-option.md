# 0043: `traceability`を作業ツリー専用にし、出力形式を選ぶ`--format`を廃止する

## ステータス

Accepted(2026-10-06決定)。[0033](./0033-traceability-defaults-to-working-tree.md)の「`--at`を省略可とし、省略時に作業ツリーを読む」を置き換える(作業ツリーを読むという決定そのものは有効である)。

## 背景

[0033](./0033-traceability-defaults-to-working-tree.md)は、`traceability`に`--at <ref>`を残したまま、省略時に作業ツリーを読むようにした。その後、`--at <ref>`を使う利用者は現れなかった。GUIは作業ツリーだけを読み、コミット済みの特定時点は`coverage --at`で読む。設計書に残る`--at`の使い道は「コミット済み状態を確認したい場合」の1行だけだった。

`--at`を残すと、次のコストが続く。

- 同じ要求とケースの紐づきを、Git treeから読む経路と作業ツリーから読む経路の2つで保つ必要がある。
- Git treeから読む経路は、blobごとにgitを起動し、Windowsで遅い(Issue #112)。

また、`coverage`・`impact`・`traceability`・`import`・`release scope show`には`--format`があるが、値は`json`の1つだけで、選ぶ意味がない。出力が常にJSONであることを示すだけで、人間可読出力の予定もない(`generate`・`verify`の`--json`は、人間向けテキストとの切り替えで、意味が違う)。

## 決定

### 1. `traceability`と`traceability show`から`--at`を削除する

両コマンドは、作業ツリーだけを読む。コミット済みの特定時点を読みたい場合は、`coverage --at`を使う。

出力の`at`も削除する。読み取り元が作業ツリーの1つだけになり、値が常に`"working-tree"`で、情報を持たないためである([0033](./0033-traceability-defaults-to-working-tree.md)決定2の、読み取り元を区別する目的は不要になる)。`traceability show --uid`が見つからない場合のエラー文も、`uid`だけを示す。

`impact`・`coverage`は、2点比較・リリース監査という目的のため、`--base`/`--head`・`--at`を持つ([0033](./0033-traceability-defaults-to-working-tree.md)決定4のとおり)。

### 2. `--format`を、`coverage`・`impact`・`traceability`・`import`・`release scope show`から削除する

出力は常にJSONである。人間可読出力が必要になった時点で、必要な形のオプションを足す。

## 影響

- `traceability --at`・`traceability show --at`・上記5コマンドの`--format`は、指定するとclapの未知の引数のエラーになる。GUIが`--format json`を渡している場合は、外す。
- `traceability`の読み取り経路から、Git treeの読み取りがなくなる。
- `traceability`・`traceability show`の出力から`at`がなくなる。JSON Schemaも同じく更新する。GUIが`at`を読んでいる場合は、外す。

## 検討したが採用しない選択肢

- **`--at`を残す**: 使う人がいないまま、2つの読み取り経路と、遅いGit tree経路を保つことになる。必要になった時点で、その時点の要求に合わせて足す。
- **`at`を固定値`"working-tree"`で残す**: 値が変わらず、情報を持たない。後方互換のためだけに残すことになる。
- **`--format`を残す**: 選べる値が1つだけで、将来の拡張のためだけに残すことになる(YAGNI)。
