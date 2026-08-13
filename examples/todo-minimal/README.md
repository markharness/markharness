# todo-minimal

日本語版 / Japanese version: [README.ja.md](./README.ja.md)

A minimal, self-contained sample you can copy and run as-is, used in the minimal tutorial in the repository root [README.md](../../README.md).

- `axes/` — the axis registry (`workflow` / `ui` / `validation`) required by `markharness knowledge apply`.
- `draft-v1.yml` — a minimal chain of Requirement (`todo-management`) → Feature (`add-todo`) → Behavior (`add-task`) → Condition (`empty-title`) → one ExpectedResult. This is the draft YAML format read by `markharness knowledge apply`/`validate` (see [docs/en/cli-manual.md](../../docs/en/cli-manual.md) §1.3 for details).
- `draft-v2.yml` — a draft that adds a second Condition (`max-length`) to the same Feature/Behavior. Used to demonstrate a `ChangeEvent` between milestones.

It does not run standalone — copy `axes/` into a project directory that has already had `markharness init` run on it, then pass a draft YAML to `markharness knowledge apply`. See the repository root README.md for the full walkthrough.
