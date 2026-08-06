---
name: skill-creator
description: "Create new skills, modify and improve existing skills, and measure skill quality. Use when users want to create a skill from scratch, edit or refine an existing skill, test a skill with sample prompts, optimize a skill's description for better triggering accuracy, or turn a workflow from the current conversation into a reusable skill."
argument-hint: "Describe the skill you want to create or improve"
---

# Skill Creator

> Respond in the language the user is using in the chat.

A skill for creating new skills and iteratively improving them — adapted for VS Code Copilot from the [anthropics/skills](https://github.com/anthropics/skills) repository.

## Overview

The core loop:

1. Figure out what the skill should do and roughly how
2. Write a draft of the SKILL.md
3. Run realistic test prompts against it (ideally comparing with-skill vs. without-skill)
4. Review the results with the user
5. Improve the skill based on feedback — by generalizing, not by patching
6. Repeat until the user is satisfied
7. Optionally, optimize the description for triggering accuracy

Your job is to figure out where the user is in this process and help them progress. If they already have a draft, jump straight to testing. If they say "just vibe with me, no formal testing", that's fine too — stay flexible.

## Communicating with the User

This skill may be used by people with varying levels of technical familiarity:
- If the user uses casual language, explain technical terms briefly
- If the user is clearly technical, use standard jargon freely
- When in doubt, briefly define terms like "frontmatter", "trigger", "description field"

## Creating a Skill

### Capture Intent

The current conversation might already contain the workflow the user wants to capture (e.g. they say "turn this into a skill"). If so, **extract answers from the conversation history first** — the tools used, the sequence of steps, corrections the user made, input/output formats observed. Then have the user fill the gaps and confirm before proceeding.

Establish:

1. What should this skill enable the agent to do?
2. When should this skill trigger? (what user phrases/contexts)
3. What's the expected output format?
4. Should we set up test cases? Skills with objectively verifiable outputs (file transforms, data extraction, code generation, fixed workflow steps) benefit from test cases. Skills with subjective outputs (writing style, design) often don't — suggest the appropriate default, but let the user decide.

### Interview and Research

Ask about edge cases, input/output formats, example files, success criteria, and dependencies. Check the workspace for existing patterns to follow. Wait to write test prompts until this is ironed out.

### Write the SKILL.md

**Location**: `.github/skills/<skill-name>/SKILL.md`

**Frontmatter**:

```yaml
---
name: skill-name              # 1-64 chars, lowercase + hyphens, must match folder
description: 'What it does and when to use it. Max 1024 chars.'
argument-hint: 'Optional hint for slash command usage'
---
```

#### Writing the description — be "pushy"

The description is the primary triggering mechanism, and agents tend to **undertrigger** skills — they skip them even when they'd help. Counteract this by making the description a little pushy: state what the skill does AND enumerate the contexts where it should be used, even when the user doesn't ask explicitly.

**Example:**

- Weak: `"How to build a dashboard to display internal data."`
- Strong: `"How to build a simple fast dashboard to display internal data. Use this whenever the user mentions dashboards, data visualization, metrics, or wants to display any kind of company data, even if they don't explicitly ask for a 'dashboard'."`

All "when to use" information goes in the description, not in the body — the body is only loaded *after* the skill triggers.

#### Anatomy of a Skill

```text
skill-name/
├── SKILL.md           # Required (name must match folder)
├── scripts/           # Executable code for deterministic/repetitive tasks
├── references/        # Docs loaded into context as needed
└── assets/            # Templates, boilerplate, icons, fonts
```

#### Progressive Disclosure

Skills load in three levels — design around this:

1. **Metadata** (name + description) — always in context (~100 words)
2. **SKILL.md body** — in context whenever the skill triggers (keep under 500 lines)
3. **Bundled resources** — loaded only as needed (unlimited; scripts can execute without being read)

If SKILL.md approaches 500 lines, add a layer of hierarchy: move detail into `references/` with clear pointers on when to read each file. For large reference files (>300 lines), include a table of contents.

**Domain organization**: when a skill supports multiple variants (frameworks, providers, formats), split by variant so only the relevant file gets loaded:

```text
cloud-deploy/
├── SKILL.md           # workflow + how to choose the variant
└── references/
    ├── aws.md
    ├── gcp.md
    └── azure.md
```

(この構成はこのテンプレートの `setup` スキルでも使っています — Google OAuth の手順を `references/google-oauth-example.md` に分離)

#### Writing Style

- **Explain the why.** Modern LLMs have good theory of mind. Instead of rigid ALL-CAPS MUSTs and NEVERs, explain the reasoning so the agent can generalize to cases you didn't anticipate. Finding yourself writing "ALWAYS"/"NEVER" repeatedly is a yellow flag — reframe with reasoning.
- **Use imperative form** in instructions.
- **Include examples** for output formats:

```markdown
## Commit message format
**Example:**
Input: Added user authentication with JWT tokens
Output: feat(auth): implement JWT-based authentication
```

- **Use relative paths** (`./scripts/...`) for skill resources.
- **Principle of lack of surprise**: a skill's contents should not surprise the user given its description. No malware, no misleading behavior, no data exfiltration.

## Testing a Skill

### Write realistic test prompts

Create 2-3 test prompts — **the kind of thing a real user would actually type**, not abstract requests. Concrete details (file names, personal context, casual phrasing, even typos) make tests realistic:

- Bad: `"Format this data"`
- Good: `"ok so my boss sent me this xlsx (in my downloads, 'Q4 sales final FINAL v2.xlsx') and she wants a column showing profit margin as a percentage. revenue is column C and costs column D i think"`

Share the prompts with the user before running: "Here are the test cases I'd like to try — do these look right, or do you want to add more?"

Record them in `<skill-name>/evals/evals.json` so they can be rerun in later iterations:

```json
{
  "skill_name": "example-skill",
  "evals": [
    { "id": 1, "prompt": "User's task prompt", "expected_output": "Description of expected result" }
  ]
}
```

### Run and compare

VS Code Copilot has no subagents, so testing is manual but still valuable:

1. **With-skill run**: open a **new chat session** (so the drafting conversation doesn't leak context) and run each test prompt. Verify the skill actually triggers.
2. **Baseline (recommended)**: run the same prompt in another fresh session with the skill temporarily renamed/disabled. The comparison shows whether the skill actually adds value.
3. Review outputs with the user, one test case at a time. Collect specific feedback per case.

## Improving a Skill

This is the heart of the loop. How to think about improvements:

1. **Generalize from feedback.** The skill will be used across many prompts, but you're iterating on only a few examples. If the skill only works for those examples, it's useless. Avoid fiddly, overfitted patches; if an issue is stubborn, try a different metaphor or a different working pattern instead of adding constraints.
2. **Keep the prompt lean.** Remove instructions that aren't pulling their weight. Read the *transcripts*, not just the final outputs — if the skill makes the agent waste time on unproductive steps, cut the parts causing it.
3. **Explain the why** behind every instruction (see Writing Style above). Terse or frustrated user feedback still contains an underlying need — understand it and transmit that understanding into the skill.
4. **Look for repeated work.** If every test run independently wrote a similar helper script or took the same multi-step detour, bundle that as `scripts/<name>` and tell the skill to use it. Write it once; save every future invocation from reinventing it.

Then rerun the test prompts (fresh sessions) and review again. Keep going until the user is happy, the feedback is all positive, or you're no longer making meaningful progress.

## Description Optimization

After the skill body is in good shape, offer to tune the description for triggering accuracy.

### How triggering works

The agent sees only name + description when deciding whether to load a skill, and it only consults skills for tasks it can't trivially handle alone. Simple one-step queries ("read this file") may not trigger a skill even with a perfect description. Test with substantive, multi-step prompts.

### Trigger test set

Write ~10 realistic queries: half **should-trigger**, half **should-not-trigger**.

- Should-trigger: different phrasings of the same intent (formal / casual), cases where the user doesn't name the skill or file type but clearly needs it, uncommon use cases.
- Should-not-trigger: **near-misses are the valuable ones** — queries sharing keywords with the skill but needing something else (adjacent domains, ambiguous phrasing where naive keyword matching would trigger incorrectly). Obviously-irrelevant negatives ("write a fibonacci function" for a PDF skill) test nothing.

Run each query in a fresh session and record whether the skill triggered. Fix misses by adding the missing contexts/phrasings to the description; fix false triggers by sharpening the boundary ("Do NOT use for ..."). Show the user before/after and the results.
