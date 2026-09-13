# todo-minimal

English version: [README.md](./README.md)

リポジトリ直下の [README.ja.md](../../README.ja.md) の最小チュートリアルで使う、コピーしてそのまま動かせる最小構成のサンプルです。

- `axes/` — Knowledge Intentが参照する axis レジストリ(`workflow` / `ui` / `validation`)。未登録axisは `unknown_axis` エラーになる。
- `intent-v1.yml` — Requirement(`todo-management`) → Feature(`add-todo`) → Behavior(`add-task`) → Scenario(`empty-title`)からなる最小のKnowledge Intent(形式の詳細は[docs/ja/cli-manual.md](../../docs/ja/cli-manual.md) 1.2節)。
- `intent-v2.yml` — 同じ Behavior に2件目のScenario(`max-length`)を追加したIntent。望ましい状態を全部書き直しても、内容が変わらない要素は `unchanged` となり追加分だけが作成される — この宣言的な再実行と、マイルストーン間の `ChangeEvent` を実演するために使う。

単独では動かず、`markharness init` 済みのプロジェクトディレクトリに `axes/` をコピーし、`markharness knowledge reconcile` にIntentファイルを渡す形で使います。手順はリポジトリ直下の README.ja.md を参照してください。
