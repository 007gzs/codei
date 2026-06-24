---
name: commit-messages
description: Generate descriptive git commit messages from staged changes. Use when the user asks for commit messages or help committing.
---

# Commit Message Helper

When the user wants a commit message:

1. Run `git diff --staged` (or `git diff` if nothing is staged) to inspect changes.
2. Draft a concise commit message:
   - Subject line ≤ 72 characters, imperative mood (e.g. "add", "fix", "update").
   - Optional body explaining **why**, not just what.
3. Match the repository's existing commit style if `git log` shows a pattern.

Do not run `git commit` unless the user explicitly asks to commit.
