---
name: skillctl
description: Manage project-local and global Codex skills from .agents/skills.lock.yaml files using the skillctl CLI, including initializing locks, installing pinned vendored skills, checking integrity, comparing changes, synchronizing exact revisions, and updating shared skills safely.
---

# Use skillctl

Use project scope unless the user explicitly requests global scope with `--global`. Prefer `--json` whenever parsing results.

Run `skillctl check` before modifying managed skills. Treat every `mode: vendored` destination as read-only: change a shared skill in its authoritative source repository, then run `skillctl update`. Use `skillctl sync` to reproduce the locked revision without advancing it. Do not edit lock commits or digests manually.

Inspect with `status`, `list`, and `diff [NAME]`. Add a source with `add REPOSITORY --path PATH [--name NAME] [--ref REF]`; update its followed ref with `update [NAME]`; restore its exact pin with `sync [NAME]`; and remove its declaration with `remove NAME`. A removed local entry keeps its files. Use the same commands with `--global` only for explicitly requested machine-global management.

Preview replacement or deletion operations with `--dry-run`. Never use `--force` until reviewing and disclosing the local differences it will overwrite. Never alter `mode: local` skill files through this CLI.

Project-vendored directories and `.agents/skills.lock.yaml` should normally be committed together. Writes affect local files, so perform only the mutation the user requested. Installing this `SKILL.md` does not install the `skillctl` binary.

