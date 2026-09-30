# 0037: Positioning against the related tool Sphinx-Needs, and not integrating with it for now

## Status

Accepted (decided 2026-09-30). No implementation follows from it. This records how the two tools relate, why markharness does not integrate with Sphinx-Needs, and what an integration would require. The conditions for revisiting the decision are at the end.

## Background

Sphinx-Needs is often named as a tool whose philosophy is close to markharness. This record keeps the comparison and the decision so that the similarities and differences do not have to be derived again each time.

The descriptions of Sphinx-Needs come from reading its official documentation (sphinx-needs.readthedocs.io). The source code was not read, and behavior the documentation does not state is treated as unverified.

## Similarities

- **Docs-as-Code.** Requirements and test knowledge are written as text and managed in Git. Neither tool makes a database or a GUI the source of truth.
- **Elements with IDs, and links between them.** A Sphinx-Needs need (type, ID, links, extra fields) corresponds to markharness's Requirement / Feature / Behavior / Scenario.
- **Visible traceability and verification coverage.** Sphinx-Needs shows them with filters, tables, and flow diagrams. markharness shows them with `traceability` and Release Coverage.
- **Machine-checked consistency.** Sphinx-Needs uses warnings and schema validation. markharness uses `validate`.

## Differences

| Aspect | Sphinx-Needs | markharness |
|---|---|---|
| Main subject | Requirements, specifications, and tests embedded in documents, and the relations between them | Test knowledge (Feature / Behavior / Scenario) and its change impact |
| How the source of truth is read | The Sphinx build collects needs from documents and builds a graph | Reads YAML in Git directly (external requirements go through a StrictDoc JSON export; ADR 0036) |
| Time axis | A snapshot of the current documents by default. `needs.json` can keep the needs of several versions side by side, and `needimport` can select a version with `:version:` | Derives `ChangeEvent` automatically by comparing Git tree SHAs between milestone tags |
| Handling of tests | A test is a kind of need. Execution results come from a separate extension | Generates `TestCase` deterministically from a Scenario and declares the verification means with `binding` |
| Validation rules | Declarative schema validation, for example through `needs_schema_definitions` | Checks built into `validate` |
| External requirements | References another project's `needs.json` through `needs_external_needs` | Pins a StrictDoc requirement with `source: external` (ADR 0023) |

Two points matter. Sphinx-Needs is strong at drawing the present relations. markharness answers what changed between versions and what needs re-checking. That gap is where a complementary relationship could form, and it is also why the two tools do not compete.

## Decision

**markharness does not implement Sphinx-Needs integration for now.** `source: external` keeps pointing only at StrictDoc (ADR 0030 §1). The reasons are:

1. **The intended users do not overlap.** markharness targets developers and QA teams close to development. It does not aim at the document-centered and regulated organizations where Sphinx-Needs is strong.
2. **There is no demonstrated demand.** No user of Sphinx-Needs has asked for it, and there is no plan to use it ourselves. Under YAGNI, the design is extended when a real requirement arrives, to fit that requirement.
3. **Supporting two kinds of external source is costly to maintain.** The meaning of `source_key`, the granularity of `source_locator`, stale pins, and the branches in `validate` and traceability would each multiply by the number of external source kinds.
4. **Some information an integration needs cannot be confirmed from the public documentation.** See "What an integration would require" below.

Depending on a build artifact such as `needs.json` is not, by itself, a reason to decline. `knowledge intent-from-strictdoc` (ADR 0036) already takes a JSON file that the user produced with `strictdoc export`, which has the same shape. An earlier draft of this analysis called that dependency a tension with Git-native operation. It contradicts ADR 0036, so it is not used as a reason.

### Relation to why StrictDoc was chosen

StrictDoc was chosen as the external source because it manages requirements as text in Git (`.sdoc` and Markdown), which fits the Git-native design. The StrictDoc official FAQ describes StrictDoc as a close successor of Doorstop: it began as a fork of Doorstop, and was later restarted from scratch. The two share no code but share design principles (requirements as text, kept next to the code). Doorstop keeps one YAML file per requirement, while StrictDoc keeps one document per `.sdoc` file that can hold many requirements. This reason for choosing StrictDoc does not apply to the build-centered model of Sphinx-Needs.

## Smart solutions in Sphinx-Needs

Each mechanism of Sphinx-Needs was reviewed for whether markharness should take it.

| Sphinx-Needs mechanism | Treatment in markharness |
|---|---|
| **Declarative schema validation.** Besides types, patterns, and enums, it declares lower and upper bounds on link counts (`minContains` / `maxContains`), checks that follow links up to four hops, and conditional rules, all in JSON Schema | **A new idea worth noting.** markharness has no way for users to add validation rules declaratively. No concrete check that needs it has appeared yet, so it is not introduced now. Consider it when rules such as "every Requirement is linked to at least one Feature" start to differ between users |
| **`id_prefix` to avoid ID collisions for external needs** | The same solution is already in use. `intent-from-strictdoc` prefixes `id` with `sd-` (ADR 0036 §3) |
| **`needextend`.** Overrides, appends to, or deletes values from another location without editing the original need | The same idea is already in use. An external Requirement cannot hold `label` or `description`, but it can keep `axis` on the markharness side (ADR 0023 §5) |
| **`needs_reproducible_json`.** Removes timestamps so the output is reproducible | The same idea is already in use. The `id` of a generated Intent depends only on the input MID, not on execution order (ADR 0036, Invariants) |
| **Keeping several versions side by side in `needs.json`** | Not adopted. Git already holds the version history, and markharness derives changes by comparing tree SHAs between tags. That is the differentiator itself |

Of these, only declarative schema validation is worth considering. The others are already solved the same way, or Git covers their role.

## What an integration would require

The issues to settle if an integration is ever needed are recorded here in advance.

1. **Input format.** Using `needs.json` (the output of the `needs` builder) is the natural choice, the same shape as ADR 0036. markharness would not run Sphinx (same reason as ADR 0036 §2).
2. **Source location.** ADR 0036 §4 decided `source_locator` through an index of MIDs because StrictDoc's JSON has no source file path. The public `needs.json` documentation does not list position fields such as `docname` or `lineno`, and whether they exist is unverified. Check a real output before integrating.
3. **Granularity of change detection.** Change detection in ADR 0023 is a blob diff of the file that `source_locator` points to. Sphinx-Needs writes many needs in one `.rst` or Markdown file, so file granularity is too coarse. A per-need identifier (something like `source_key`) would be needed separately from the blob diff.
4. **Stability of IDs.** `needs_id_required` defaults to `False`, so IDs may be auto-generated. Whether an auto-generated ID stays stable across edits could not be confirmed from the documentation. As ADR 0036 §6 requires that MIDs be persisted in the source, an integration would assume that IDs are explicit (`needs_id_required = True`).
5. **Naming.** When a second input format arrives, revisit the name `intent-from-strictdoc` and the premise that `source: external` always means StrictDoc (ADR 0030 §1). ADR 0036 defers generalization until that point.

## Alternatives considered and rejected

- **Implement Sphinx-Needs integration now.** Rejected because there is no demonstrated demand and many points are unverified.
- **Parse `.rst` or Markdown (MyST) directly to read needs.** Sphinx-Needs syntax depends on extensions and build settings. Rejected for the same reason ADR 0036 rejected a custom `.sdoc` parser.
- **Replace markharness's `ChangeEvent` with the version history in `needs.json`.** Git already holds the version history, so there is no reason to keep a duplicate in a build artifact.

## Triggers to revisit

Revisit this decision when either of the following holds.

- A developer or QA team user actually asks to reference requirements managed in Sphinx-Needs as external Requirements.
- Users actually ask to add different validation rules declaratively (considering schema validation).

## Sources

- Sphinx-Needs: https://sphinx-needs.readthedocs.io/en/latest/ (overview, Builders, Configuration, Schema validation, `needextend`, `needimport`). Checked on 2026-09-30.
- StrictDoc F.A.Q.: https://strictdoc.readthedocs.io/en/latest/latest/docs/strictdoc_03_faq.html (relation to Doorstop). Checked on 2026-09-30.
