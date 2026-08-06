---
description: "Use when managing task progress with a checklist file. Covers creating, updating, and completing checklist items during any multi-step workflow."
applyTo: "checklist-*.md"
---

# Checklist Workflow

> Respond in the language the user is using in the chat.

This instruction defines the standard process for tracking work progress through a checklist file.
It is referenced by multiple prompts to ensure consistent behavior.

## Creating a Checklist

Before starting any multi-step task, create a markdown checklist file:

- **Filename**: `checklist-<task-name>.md` (e.g., `checklist-setup-external-api.md`)
- **Location**: Project root
- **Format**:

```markdown
# Task: <descriptive title>
Created: <date>

## Steps
- [ ] Step 1: <clear, actionable description>
- [ ] Step 2: <clear, actionable description>
- [ ] Step 3: <clear, actionable description>

## Notes
<any context, decisions, or blockers>
```

## Rules

1. **One checklist per task**: Do not mix unrelated work in the same checklist.
2. **Granular steps**: Each step should be completable in a single focused effort. If a step feels too large, break it down.
3. **Update immediately**: Mark a step as done (`- [x]`) right after completing it — not in a batch later.
4. **Add notes as you go**: Record decisions, surprises, or blockers in the Notes section so context is preserved.
5. **Never delete steps**: If a step turns out to be unnecessary, mark it with `- [~] Skipped: <reason>` instead of removing it.
6. **Final review**: When all steps are complete, add a `## Summary` section at the bottom with a one-line outcome.

## Integrating with Other Workflows

When used alongside TDD or other prompts, the checklist drives the overall task sequence while the specialized workflow (e.g., Red-Green-Refactor) governs each individual step's execution.
