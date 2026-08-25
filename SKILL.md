---
name: skillctl-skill
description: Manage project-local and global agent skills from .agents/skills.lock.yaml files using the skillctl CLI, including initializing locks, installing pinned vendored skill directories or individual SKILL.md files, checking integrity, comparing changes, synchronizing exact revisions, and updating shared skills safely.
---

# Use skillctl

Use project scope unless the user explicitly requests global scope with `--global`. Prefer `--json` whenever parsing results.

Run the network-free `skillctl check` integrity gate before modifying managed skills, and pass `--global` whenever the target is the global installation: scope is never inferred, so a bare `check` run outside any project fails with a scope error that points at `--global` rather than falling back to the global lock at `$HOME/.agents`. Use `skillctl status` to query upstream and see separate local integrity, upstream availability, and recommended actions; use `skillctl status --offline` when source access is unwanted or unavailable. Treat every `mode: vendored` destination as read-only: change a shared skill in its authoritative source repository, then run `skillctl update`. Use `skillctl sync` to reproduce the locked revision without advancing it. Do not edit lock commits or digests manually.

For automation, run `skillctl status --json` and read `local_status`, `upstream_status`, `pinned_commit`, `upstream_commit`, `content_changed`, and `recommended_action` from each skill record. Treat `not_checked` as an intentional offline result, `unreachable` as a source-access problem that does not erase local state, and `source_advanced` as a ref movement with unchanged selected-skill content.

Inspect lifecycle state with `status`, locked-versus-installed file changes with `diff [NAME]...`, and declarations with `list`. Add a complete skill directory with `add REPOSITORY --path DIRECTORY [--name NAME] [--ref REF]`. When only one repository file should be installed, use `add REPOSITORY --file PATH/TO/SKILL.md --name DESTINATION [--ref REF]`; never use file mode for a skill that needs bundled resources. Advance vendored entries along their configured ref with `update [NAME]...`; restore exact pins with `sync [NAME]...`; and remove a declaration with `remove NAME`. `sync`, `check`, `diff`, and `update` accept zero or more names: omitting NAME applies the command to every lock entry (the default for a bare `update`), and naming several entries updates exactly that subset in one invocation. For a `mode: local` entry, `remove NAME` removes only its lock declaration and leaves its destination files untouched. Changing the followed ref itself requires `remove NAME` and then `add` again with the new `--ref`; `add` refuses to overwrite an existing entry. Use the same commands with `--global` only for explicitly requested machine-global management.

Preview replacement or deletion operations with `--dry-run`. Never use `--force` until reviewing and disclosing the local differences it will overwrite. Never alter `mode: local` skill files through this CLI.

Project-vendored directories and `.agents/skills.lock.yaml` should normally be committed together. Writes affect local files, so perform only the mutation the user requested. Installing this `SKILL.md` does not install the `skillctl` binary.
