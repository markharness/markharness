---
description: "Create a task checklist before starting work. Tracks progress step by step by writing and updating a checklist file."
agent: "agent"
---

# Plan Checklist

> Respond in the language the user is using in the chat.

You are about to start a task. Before writing any code, create a structured checklist to track progress.

Follow the checklist workflow defined in [checklist-workflow](./../instructions/checklist-workflow.instructions.md).

## Your Job

1. **Understand the task**: Ask clarifying questions if the goal is ambiguous. Infer the most useful interpretation if it's reasonably clear.
2. **Break it down**: Decompose the task into small, concrete steps. Each step should be verifiable — you should be able to tell when it's done.
3. **Create the checklist file**: Write it to `checklist-<task-name>.md` in the project root.
4. **Work through the steps**: Execute each step one at a time. Mark each step complete in the file immediately after finishing it.
5. **Finish with a summary**: When all steps are done, add a Summary section to the checklist file.

## Important

- If you are also using the `/dev-tdd` workflow, each TDD cycle (Red-Green-Refactor) should be a step in the checklist.
- Run the linter after every code change: `npx eslint --fix <file>`
- If you encounter a blocker, record it in the Notes section and inform the user.
