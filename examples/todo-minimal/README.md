# todo-minimal

リポジトリ直下の [README.md](../../README.md) の最小チュートリアルで使う、コピーしてそのまま動かせる最小構成のサンプルです。

- `axes/` — `markharness knowledge apply` が要求する axis レジストリ(`workflow` / `ui` / `validation`)。
- `draft-v1.yml` — Requirement(`todo-management`) → Feature(`add-todo`) → Behavior(`add-task`) → Condition(`empty-title`) → ExpectedResult 1件からなる最小のチェーン。`markharness knowledge apply`/`validate` が読むドラフトYAML形式(詳細は[docs/cli-manual.md](../../docs/cli-manual.md) 1.3節)。
- `draft-v2.yml` — 同じ Feature/Behavior に、2件目のCondition(`max-length`)を追加するドラフト。マイルストーン間の `ChangeEvent` を実演するために使う。

単独では動かず、`markharness init` 済みのプロジェクトディレクトリに `axes/` をコピーし、`markharness knowledge apply` にドラフトYAMLを渡す形で使います。手順はリポジトリ直下の README.md を参照してください。
