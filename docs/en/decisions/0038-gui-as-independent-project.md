# 0038: The GUI is an independent project, and `markharness gui` only launches it

## Status

Accepted (decided 2026-10-02). Not implemented. It keeps the policies of [0022](0022-remove-stage3-dashboard.md) (no UI bundled into the core) and [0032](0032-cli-read-model-seam.md) (the CLI's JSON is the external contract) and decides how to build a GUI on top of them.

## Background

Test knowledge in markharness has relationships among Requirements, Features, Behaviors, Scenarios, and TestCases. When people edit YAML directly, they must understand the related elements and the effects of a change, then preserve consistency while handling UIDs and file locations. This burden can discourage even small additions, as well as larger changes, and make it harder to keep test knowledge current.

The CLI outputs these relationships, Change Impact, and Release Coverage as JSON. The GUI is intended to let people follow the relationships, select what to change, edit test knowledge in that context, and check the resulting relationships. It also lets people who do not use a terminal (including non-developers) do this work on screen. A viewing-only stage is allowed during development, but it still sends people back to YAML to make changes; the GUI is therefore bundled into the markharness distribution only once it can edit. This record decides in what form the GUI is provided.

[0022](0022-remove-stage3-dashboard.md) removed the dashboard that was bundled into the core because it meant "permanently bearing the maintenance cost of a UI that does not serve the MVP's thesis." [0032](0032-cli-read-model-seam.md) made the JSON of `traceability`, `impact`, and `coverage` an external contract and left display to a separate tool.

The precedent is `git gui`. The official Git manual says git-gui is maintained as an independent project and that its stable releases are distributed as part of Git. It is launched by the subcommand `git gui`, which finds and runs a separate executable, `git-gui`. git-gui uses Git by calling the git CLI as a child process, and its implementation language is independent of Git itself.

## Decision

### 1. The GUI is a project independent of the core

The GUI is built in a separate repository (`markharness-gui`) as a separate executable (`markharness-gui`). Its implementation technology, screen design, and internal structure are decided by the GUI repository; the core does not assume them. The core holds no HTTP server, frontend assets, or browser-launching code.

### 2. The only point of contact with the core is the JSON the CLI outputs

The GUI calls `markharness` as a child process from outside and reads only its JSON output. It does not read the internal files under `.markharness/` directly, and it does not call the core's Rust library directly.

The viewing-only intermediate stage uses only the existing read outputs: `traceability` (with `--at` omitted, so the working tree), `binding list`, `coverage`, and `impact`. Detail fields such as axes, descriptions, and steps are not in the existing output. To show them, which editing needs, first add a read output to the CLI and have the GUI read that output (decision 10). `axes list` is not used in this intermediate stage. It returns only a list of axis definitions (`id` and `label`), and `traceability` returns no per-element axis, so the screen has nothing to attach the list to. Its output is also a bare array with neither `record_kind` nor `schema_version`, so decision 5's check of the record kind cannot apply to it.

### 3. `markharness gui` only launches

```
markharness gui [--dir <path>]
```

- It determines the target project root with `project_root::resolve`, like the other commands. If the project is not initialized, it fails with an error that tells the user to run `markharness init`.
- It looks for the executable `markharness-gui` in the same directory as `markharness` itself, then on `PATH`, and runs it. If it is not found, it exits with an error saying the GUI is not bundled and how to get it.
- It passes `--dir <absolute path of the root>` to the GUI and sets the environment variable `MARKHARNESS_BIN` to the absolute path of the `markharness` that launched it. The GUI calls the `markharness` at that path. Only when the GUI is started on its own does it use the `markharness` on `PATH`. This keeps the GUI calling only the `markharness` of the same distribution and avoids version mismatch.
- It waits for the GUI to exit and returns the GUI's exit code unchanged. It waits so that a launch failure or error is visible in the terminal and `Ctrl-C` ends it. Whether the GUI itself stays resident or detaches is up to the GUI.
- It is not a general mechanism for running arbitrary external commands (`markharness <name>`). It has only `gui` until something else is needed.

### 4. Distribution keeps the CLI-only archive unchanged and adds a separate GUI-bundled archive

So that people who use only the CLI (CI, AI) do not get a larger download for the GUI, the current CLI-only archive stays as it is. A separate GUI-bundled archive is built with `markharness` and `markharness-gui` in the same directory. The GUI artifact is the stable release published by the GUI repository, pinned and pulled in when `markharness` is released. The stable release pulled in is limited, as decision 10 says, to one that can edit test knowledge. Running `markharness gui` from a CLI-only distribution gives the "not bundled" error of decision 3.

### 5. Version compatibility is decided by `schema_version`; during 0.x it does not change, so the bundled combination is trusted

The GUI checks that the `record_kind` of the JSON it receives is the kind it expects and that its `schema_version` is a version the GUI supports, and stops displaying if not. It shows the supported version and the received version, guides the user to update, and does not show partial results. Showing wrong relations does more harm than stopping. The GUI treats a rise in `schema_version` as the signal of a change that requires it to read differently. Which changes raise it is decided by the core, as 0039 says.

The GUI reads only the fields it needs, and ignores unknown keys added to existing objects and unknown new arrays, because such additions do not make the relations within the range the GUI displays wrong. On the other hand, when a value the GUI does not know appears in a set of values whose meaning the GUI interprets (such as the `kind` of a relation, the `source` of a Requirement, or the `mode` of a binding), it does not silently drop that element but stops displaying, as an unsupported version. It stops in the same way when a field it needs is missing. This detects a case where `schema_version` is the same but a field that was added is absent from an older `markharness`, and a case where a newer `markharness` emits a new value. That no partial results are shown applies to these cases where the GUI stops.

However, by [0026](0026-module-inventory-and-plan-removal.md) decision 7 and [0039](0039-read-output-schema-version-frozen-in-prototype.md), the `schema_version` of public JSON is fixed at `1` during 0.x and is not raised even when the shape of an output changes. During that time the `schema_version` check cannot detect a compatibility change (only a missing required field can be detected). So during 0.x the bundled combination is trusted: the GUI calls only the `markharness` of the same distribution (decision 3), and the GUI's stable release is pinned to the `markharness` release it is bundled with (decision 4). As a result, when the GUI is started on its own and uses a `markharness` of a different version, a difference in the shape of an output cannot be detected.

The criterion for raising `schema_version` is decided when the criteria for 1.0 are met, as 0039 says. Once `schema_version` is actually raised, the GUI's check works as a compatibility judgment without any change to the GUI.

### 6. The CLI's error format is not unified

The GUI treats a non-zero exit code as failure and shows the standard-error text as it is. Some commands report errors as JSON and others as plain text, but there is no concrete case yet where the GUI needs to branch mechanically, so it is not unified.

### 7. The GUI runs `strictdoc export` to get StrictDoc display information

For a Requirement with `source: external`, `traceability` returns only up to `source_key`; it does not include the requirement's title or hierarchy. To show them, the GUI runs `strictdoc export --formats=json` and reads that JSON. This is an exception for the GUI alone, which keeps the policy that the core neither holds nor duplicates external content ([0023](0023-requirement-native-and-external-source.md)) and the decision that the core does not run `strictdoc` ([0036](0036-knowledge-intent-from-strictdoc.md) decision 2). StrictDoc stays an optional integration; where it is absent, the GUI shows only markharness's data.

The export takes time (see the measurements below). The GUI stores the export JSON as its basis and re-exports in the background only when the StrictDoc documents or configuration files are newer than that JSON. During this, it shows markharness's data first. The core imposes only one constraint: **never keep showing stale content. The user must have a way, which does not use the cache (a forced re-export), to get out of a stale display for sure.** The storage location, the details of the check, and the in-progress display are decided by the GUI repository.

### 8. The display is refreshed by a manual refresh action

The GUI does not watch files. It re-fetches the JSON when the user refreshes. After the GUI itself writes (edits), it re-fetches automatically. Changes made outside the GUI are picked up by the refresh action.

### 9. Not covered at this time

- Launching without a terminal (double-click, a folder-chooser screen, a Start menu entry). If this becomes necessary, it will be a separate desktop edition as a separate project, keeping its impact on this core small.
- A review snapshot for viewing a specific committed version (including a way to receive a StrictDoc export produced by CI). It will be designed separately, including identification of the version it was made from. It will not be added as a standalone option.

### 10. The condition for bundling the GUI into the markharness distribution is that it can edit test knowledge

The GUI is responsible for both viewing and editing. A viewing-only version may exist as an intermediate stage of development in the GUI repository, but it is not bundled into the GUI-bundled archive. Bundling starts with a stable release that can edit test knowledge.

Editing goes only through `knowledge reconcile`, as §14.2 of the design document `cli-read-model-design.md` requires, and the GUI does not rewrite Knowledge files directly. The detail fields that an editing screen needs (axes, descriptions, steps, and so on) are, as decision 2 says, first added to the CLI as read outputs, and the GUI reads those outputs.

## Scope of impact

- `src/cli.rs`: add a `Gui` subcommand. The file lookup and the assembly of arguments and environment variables for launching are split into units that can be tested. Implementation is a separate task done with TDD.
- `.github/workflows/release.yml`: add the GUI-bundled archive. How to pull in the GUI is decided once the GUI repository has its first stable release.
- `docs/ja/cli-manual.md`, `docs/en/cli-manual.md`: add a description of `markharness gui` together with the implementation.
- `CONTEXT.md`: the term "GUI" (already added).
- §14.1 and §14.3 of `docs/ja/design/cli-read-model-design.md` and `docs/en/design/cli-read-model-design.md`: two points that conflicted with this record are updated. (1) §14.1 limited the read commands the view uses to three (`traceability`, `impact`, `coverage`), but the GUI also uses `binding list` for verification means. `binding list` is not one of the initial read models of §3.3 but an output of the `outcome` family, so that the GUI reads it is decided within this record, and whether to add it to the read models is decided separately. (2) §14.3 said `markharness view` is not added; it now states that this record's `markharness gui` only launches a separate tool and the main tool holds no viewer implementation.
- **To be reconciled separately (not decided here)**: §13.4 of the same design document says the JSON Schemas and representative fixtures of the read models are managed as the public contract, and that an external tool imports the fixtures for each `record_kind` and `schema_version` for contract tests. This is consistent with decision 5, which judges compatibility by `schema_version`, but it does not decide what the GUI imports for contract tests. The document also has other statements that assume a differently named viewer. These will be reconciled on a separate branch after this record is agreed.
- **Not changed**: existing ADRs (including 0022, 0032, and 0033) and design documents other than the one above. This does not conflict with [0022](0022-remove-stage3-dashboard.md): no UI code goes into the core; the artifact of a separate project is only bundled into the distribution.

## Alternatives considered and not adopted

- **Embed the GUI (a local web server and assets) into the CLI binary**: distribution would be one binary, but the core's responsibilities, dependencies, and release frequency would be dragged along by UI changes. The frontend build (Node) would enter the core's development. The core would again bear the maintenance cost [0022](0022-remove-stage3-dashboard.md) avoided.
- **Put the GUI in a workspace of the same repository as the core**: a CLI contract change and the GUI fix could go in one PR. But the GUI involves a frontend build and differs from the CLI in how often it changes, in its rules, and in its toolchain, so the benefits of an independent history, rules, and toolchain outweigh that.
- **Launch only by searching `PATH`, without bundling**: this makes non-developers install separately and suffer version mismatch.
- **Have the GUI call the core's Rust library directly**: it would avoid the cost of starting a child process, but the GUI would depend on an internal API and break decision 2's "only the JSON is the contract."
- **Include a `--strictdoc-export` option that takes an exported file**: passing a file generated by CI, without information identifying the version, could combine an old export with new Knowledge and show a wrong diagram as a normal screen. The case that needs it is the "review snapshot" of decision 9; there is no reason now to add it as a standalone option.
- **Re-export every time and keep no cache**: no check logic is needed, but a project that uses source-code integration takes about 20 seconds per export (see below).
- **Have the core return display information for external requirements as CLI output**: the GUI would not need to call `strictdoc`, but it would bend the boundary [0023](0023-requirement-native-and-external-source.md) set deliberately, that the core neither holds nor duplicates external content.
- **Automatic refresh by file watching**: the implementation (especially on Windows) is excessive. As in [0033](0033-traceability-defaults-to-working-tree.md), `traceability` reads the working tree by default, so re-fetching gives the latest.

## Reference: time taken by `strictdoc export --formats=json`

Measured on 2026-10-02 with strictdoc 0.30.1 on Windows. One environment and one real project only; not measured on Linux or macOS.

| Target | Requirements | Default | `--no-parallelization` |
|---|---|---|---|
| Synthetic data (simple requirements, 8 `.sdoc` files) | 400 | 4.1 s | 2.0 s |
| StrictDoc's own whole repository (`strictdoc export .`) | 358 | 20.6 s | 18.6 s |
| A copy with only the same `docs/` and `spec/` | 358 | 6.3 s | 3.9 s |

- The main cause of the slowness is not the number of requirements but the scan for source-code traceability specified by `include_source_paths` in the configuration. This scan is unnecessary for the requirement titles and hierarchy the GUI needs.
- `--no-parallelization` roughly halved the time for small projects and shortened the whole-repository run by about 10%. The JSON content was identical in both modes for the two data sets checked. The StrictDoc help itself describes this option as "useful for debugging."
- Skipping the source-code integration (for example by overriding the configuration with `strictdoc export --config`) is an unverified possibility. StrictDoc's configuration is Python code, so changing a project's configuration partially may be hard.

## Open items (decided in the GUI repository)

- The GUI's runtime form (a native window, a small server plus a browser, and so on). It must be a self-contained artifact per OS that non-developers can use without extra installation.
- Where the export is stored, the details of the timestamp comparison (including how deleted files are handled), and the in-progress display.
- How to pull the GUI artifact into the GUI-bundled archive (decided once the GUI repository has its first stable release).
