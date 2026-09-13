# 0029: `stale_plan`の検出範囲をmarkharnessが所有する状態に限る

## ステータス

Accepted（2026-09-14決定）。[0027](0027-declarative-knowledge-reconciliation.md)§6の「入力状態」の範囲を確定する。

## 背景

[0027](0027-declarative-knowledge-reconciliation.md)§6は、通常実行がコミット直前に現在状態を再確認し、入力状態が変化していれば`stale_plan`で停止することを求める。実装はmutation plan構築の開始時にfingerprintを撮り、コミット直前に撮り直して比較する。

「入力状態」に何が含まれるかは§6が定義していない。`knowledge reconcile`が読む状態には、性質の異なる三種類がある。

1. `.markharness/knowledge`：markharnessが所有し、reconcile自身が書き換える正規Knowledge。
2. `.markharness/axes`：markharnessが所有し、`unknown_axis`の判定対象となるAxis registry。`axes`コマンドが書き換える。
3. `source_locator`が指す`.sdoc`：外部正本。markharnessは所有せず、書き込まない。[0023](0023-requirement-native-and-external-source.md)によりexternal Requirementの内容はこのファイルが持ち、`source_revision`がその時点のblob OIDを固定する。

1と2はidentity lockで保護しうる状態である。3は保護しえない。`.sdoc`はmarkharness外の編集者・エディタ・ツールが任意の時点で変更するファイルであり、reconcileがこれをlockする権限も根拠も持たない。

`source_revision: current`は、Reconciliation Moduleが`source_locator`の現在のblob OIDを解決して保存する指示である（[0027](0027-declarative-knowledge-reconciliation.md)§5）。この解決とコミットの間に`.sdoc`が編集されると、保存されるOIDは編集前の内容を指す。

## 決定

### 1. `stale_plan`の検出範囲をmarkharnessが所有する状態に限る

fingerprintの対象は`.markharness/knowledge`と`.markharness/axes`とする。`source_locator`が指す`.sdoc`の内容およびそのblob OIDは対象に含めない。

Axis registryを含めるのは、`unknown_axis`が登録済みAxisだけを参照できるという[0027](0027-declarative-knowledge-reconciliation.md)§4の規則の判定対象であり、かつ`axes`コマンドがidentity lockを取らずに書き換えるためである。登録済みと判定したAxisが判定後に削除されると、存在しないAxisを参照するKnowledgeを保存し、`markharness validate`が拒否する状態になる。これは検出しなければならない。

### 2. 対象外とする条件を明示する

次のすべてを満たす変更は`stale_plan`の対象外とする。

- 変更対象が`source_locator`の指す`.sdoc`ファイルであること。
- 変更が、`source_revision: current`のblob OID解決からコミット完了までの間に発生したこと。
- 変更によって生じる差異が、保存された`source_revision`と`.sdoc`の現在内容との不一致に限られること。

`.sdoc`の変更が正規Knowledgeの妥当性そのものを損なう場合（例：`source_locator`が指すファイルが削除された場合）は対象外規定に含めない。この場合は解決時点で`invalid_source_revision`によりfail-closedで停止する。

### 3. 並行編集時に古いblob OIDを保存し得ることを受容する

上記の窓で`.sdoc`が編集された場合、`knowledge reconcile`は編集前の内容のblob OIDを`source_revision`として保存し、成功する。この結果を受容する。

受容する根拠は次の三点である。

- **窓は閉じられない。** `.sdoc`はlockできないため、コミット直前に再解決しても窓は縮むだけで消えない。コミット完了の直後に編集されれば、保存されたOIDは同じように現在内容と一致しなくなる。「保存されたOIDが常に`.sdoc`の現在内容と一致する」という不変条件は、外部正本に対して原理的に成立しない。
- **生じる状態は不正ではない。** 固定参照が現在内容と一致しない状態は、`markharness validate`が受理する正当な状態である。これは[0023](0023-requirement-native-and-external-source.md)が定めた固定参照モデルの通常の状態であり、`.sdoc`が更新されればreconcileの関与なしに日常的に発生する。
- **検出機構が既にある。** 次節の通り、この状態はstale pinとして検出・報告される。

`.markharness/knowledge`および`.markharness/axes`の変更については、この受容は適用しない。これらは§1の通り検出対象である。

### 4. 既存の検証・運用上の緩和策

- **検出**：`markharness impact`が、external Requirementの`source_revision`とhead時点のblob OIDを突き合わせ、不一致をstale pinとして出力する（[markharness-v2-design.md](../design/markharness-v2-design.md)§6.1手順3、AC10c・AC19）。これは仕様変更の検知とは独立した項目であり、固定参照が古いことは黙って失われない。
- **訂正**：該当Requirementへ`source_revision: current`を指定して`knowledge reconcile`を再実行すれば、固定参照は現在のblob OIDへ進む。訂正のために特別な復旧コマンドを必要としない。
- **回避**：`.sdoc`を編集中のreconcile実行を避ける。単一利用者のローカルCLIとしての通常の利用では、この窓に編集が重なる状況自体が生じにくい。

### 5. 将来再検討するトリガー

次のいずれかが成立した時点で本決定を再検討する。

- `.sdoc`を書き換える主体がmarkharness自身になった場合（例：StrictDoc Adapterが`.sdoc`を生成・更新するようになった場合）。所有関係が変わるため、lockと検出の前提が変わる。
- `knowledge reconcile`が単一利用者の対話的CLIを超えて、CIジョブや常駐プロセスなど並行実行される実行環境で使われるようになった場合。窓に編集が重なる確率が実運用上無視できなくなる。
- stale pinの発生源として、`.sdoc`の通常の更新ではなくreconcileの実行窓が原因であるものが実際に報告された場合。
- `source_revision`の意味を「解決時点の内容を指すpin」から「コミット時点の内容と一致することを保証するpin」へ変更する決定が行われた場合。この変更は[0027](0027-declarative-knowledge-reconciliation.md)§5の改訂を伴う。

## 帰結

- `stale_plan`が保護する不変条件が「markharnessが所有する状態は、planを構築した時点から変化していない」と明確になる。
- 外部正本の変更検知は`impact`のstale pinが担い、`reconcile`の責務ではないことが確定する。
- fingerprintへIntentが参照するlocator集合を引き回す配管が不要になり、`Plan`の構造が読んだ状態の所有関係と一致したままになる。
- reconcile実行中に無関係な`.sdoc`を編集しても中断しない。`.sdoc`をfingerprintへ含めた場合に生じる偽陽性の中断は起きない。
- 並行編集時に古いblob OIDが保存され得ることが、レビューのたびに再判断される暗黙の前提ではなく、記録された決定になる。

## 検討したが採用しない選択肢

- **コミット直前に`source_revision: current`を再解決する**：窓は数ミリ秒から数マイクロ秒へ縮むが消えない。「常に一致する」という保証は得られず、成立しない不変条件のために解決処理を二重化する。
- **Intentが参照する`.sdoc`の内容をfingerprintへ含める**：検出はできるが、reconcile実行中の`.sdoc`編集を`stale_plan`で中断する。生じる状態が正当であり検出機構も既にあるため、中断は利用者にとって偽陽性である。
- **`.sdoc`をidentity lockの保護対象に含める**：markharnessが所有しないファイルを外部の編集者に対してlockすることはできない。markharness以外のツールがそのlockを尊重する保証がない。
- **stale pinの検出を`reconcile`側にも持たせる**：`impact`が既に持つ判定を二箇所へ複製する。`reconcile`はdesired-stateの適用を担い、base/head間の分析は`impact`が担うという分離を崩す。
