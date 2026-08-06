---
description: "Develop a feature or fix using strict Test-Driven Development. Follows the Red-Green-Refactor cycle and tracks progress via a checklist."
agent: "agent"
---

# TDD Development

> Respond in the language the user is using in the chat.

You will implement the requested feature or fix using strict TDD.

## Workflow References

- **TDD cycle**: Follow [tdd-workflow](./../instructions/tdd-workflow.instructions.md) for the Red-Green-Refactor discipline.
- **Progress tracking**: Follow [checklist-workflow](./../instructions/checklist-workflow.instructions.md) to manage your work as a checklist.
- **Security**: Follow [security](./../instructions/security.instructions.md) to protect secrets at all times.

## Procedure

### Phase 1 — Plan

1. Understand the feature or fix the user wants.
2. Identify the behaviors that need to exist (each behavior = one TDD cycle).
3. Create a checklist file (`checklist-<feature>.md`) listing each behavior as a step.

### Phase 2 — Build (repeat for each behavior)

For each step in the checklist:

1. **Red**: Write a failing test for the behavior. Run it, confirm it fails.
2. **Green**: Write the minimum code to pass. Run all tests, confirm they pass.
3. **Refactor**: Clean up. Run all tests, confirm they still pass.
4. **Lint**: `npx eslint --fix <changed-files>`
5. **Update checklist**: Mark the step as complete in the checklist file.

### Phase 3 — Verify

After all behaviors are implemented:

1. Run the full test suite: `npm test`
2. Run the linter on the entire project: `npm run lint`
3. Run the vulnerability scan: `npm audit`
4. If any `moderate` or higher severity vulnerabilities are found:
   - Attempt `npm audit fix`
   - If that doesn't resolve it, document the issue and inform the user
5. Confirm no secrets are exposed in code or configuration.
6. Add a Summary to the checklist file.

### Phase 4 — PR Readiness (if pushing)

Before creating a PR, verify the [Pre-PR Checklist from PROJECT.md](../../PROJECT.md):

- [ ] All tests pass
- [ ] Lint clean (zero errors)
- [ ] `npm audit` — no moderate+ vulnerabilities
- [ ] No `eslint-plugin-security` warnings
- [ ] No secrets in code, logs, or chat output

## Principles

- Never write production code without a failing test.
- Keep steps small — each TDD cycle should take minutes, not hours.
- If stuck, the step is too big. Break it down further.
- Trust the tests. If they pass, the code works.
