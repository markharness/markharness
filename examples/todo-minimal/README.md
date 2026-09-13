# todo-minimal

日本語版 / Japanese version: [README.ja.md](./README.ja.md)

A minimal, self-contained sample you can copy and run as-is, used in the minimal tutorial in the repository root [README.md](../../README.md).

- `axes/` — the axis registry (`workflow` / `ui` / `validation`) the Knowledge Intents reference. An unregistered axis is an `unknown_axis` error.
- `intent-v1.yml` — a minimal Knowledge Intent: Requirement (`todo-management`) → Feature (`add-todo`) → Behavior (`add-task`) → Scenario (`empty-title`). See [docs/en/cli-manual.md](../../docs/en/cli-manual.md) §1.2 for the format.
- `intent-v2.yml` — the same Intent with a second Scenario (`max-length`) added under the same Behavior. Restating the whole desired state leaves unchanged elements `unchanged` and creates only the addition — used to demonstrate both that declarative re-run and a `ChangeEvent` between milestones.

It does not run standalone — copy `axes/` into a project directory that has already had `markharness init` run on it, then pass an Intent file to `markharness knowledge reconcile`. See the repository root README.md for the full walkthrough.
