---
description: "Clean up completed checklist files. Use when a task is done and you want to archive or remove checklist-*.md files from the project root."
agent: "agent"
---

# Cleanup — Checklist Files

> Respond in the language the user is using in the chat.

Remove or archive completed checklist files from the project root.

## Procedure

1. **List all checklist files** in the project root matching `checklist-*.md`.
2. **For each file**, read it and check its status:
   - If ALL steps are marked `[x]` (complete) or `[~]` (skipped): it is **done**.
   - If any steps are still `[ ]` (incomplete): it is **in progress**.
3. **Show a summary** to the user:

   ```text
   Checklist files found:
   ✅ checklist-setup-external-api.md — all steps complete
   ✅ checklist-add-export-feature.md — all steps complete
   🔄 checklist-write-parser.md — 3 of 5 steps complete
   ```

4. **Ask the user** what to do with the completed files:
   - **Delete**: Remove completed checklist files permanently.
   - **Keep**: Leave everything as-is.

5. **Execute** the user's choice. Never delete in-progress checklists.
