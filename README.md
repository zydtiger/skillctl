# skillctl

`skillctl` is a portable Rust CLI for reproducibly managing Codex skills. It reads a declarative YAML lock, installs Git-pinned snapshots into `.agents/skills/`, detects local changes, and updates snapshots from their authoritative repositories. Already-vendored project skills remain usable after cloning without `skillctl`.

The ownership rule is deliberate: `mode: vendored` means `skillctl` owns and integrity-checks the destination; `mode: local` means the consuming project owns it and mutation commands never replace its files.

The lock mechanism is a `skillctl` convention, not an official built-in Codex lock-file feature.

## Installation

Run these commands from the `skillctl` project root. Install the binary separately:

```sh
cargo install --path .
```

Install the bundled agent skill under the required folder name:

```sh
mkdir -p ~/.agents/skills/skillctl-skill
cp SKILL.md ~/.agents/skills/skillctl-skill/SKILL.md
```

The installed directory is `skillctl-skill`, while the `SKILL.md` declared name remains `skillctl`. Copy `SKILL.md` again after updating this repository. Copying it does not install or update the CLI binary; reinstall an updated binary with `cargo install --path . --force`.

## Layout and lock schema

Project scope is the default. Commands discover `.agents/skills.lock.yaml` by walking upward from the current directory. `init` is the exception: without `--global`, it always initializes the current directory and never selects a parent lock. `--global` exclusively uses `${SKILLCTL_HOME:-$HOME}/.agents` (`SKILLCTL_HOME` is primarily a test/automation seam).

```text
project/
  .agents/
    skills.lock.yaml
    skills/
      issue-delivery/
```

New locks use schema version 2. Version 1 directory-source locks remain readable; file sources require version 2. Adding a file source to a valid v1 lock upgrades that lock to v2 while preserving its existing directory entries and installed markers.

```yaml
version: 2
skills:
  issue-delivery:
    mode: vendored
    source:
      repository: https://example.com/user/agent-workflows.git
      path: skills/issue-delivery
      ref: main
    resolved:
      commit: 0123456789abcdef0123456789abcdef01234567
      digest: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
    destination: issue-delivery
  skillctl-skill:
    mode: vendored
    source:
      repository: https://github.com/example/skillctl.git
      file: SKILL.md
      ref: main
    resolved:
      commit: 0123456789abcdef0123456789abcdef01234567
      digest: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
    destination: skillctl-skill
  project-owned-skill:
    mode: local
    destination: project-owned-skill
```

Schemas v1 and v2 reject unknown fields, unsupported versions, unsafe destinations, invalid conditional fields, duplicate destinations, and duplicate declared skill names. A v1 vendored source requires `path`; a v2 source requires exactly one of `path` or `file`. A `file` value must be a safe repository-relative path ending in `SKILL.md`. Vendored directories contain `.skillctl-managed`; it is metadata and is excluded from source digests.

Mutations validate content in isolated staging directories before replacement. Destination and lock changes use backups and rollback protection, and changed managed content is never replaced unless `sync` or `update` explicitly receives `--force`.

Project-vendored skill folders and `.agents/skills.lock.yaml` should normally be tracked together in the consuming project's Git repository. Global scope is machine-local unless `~/.agents` is separately managed in version control.

## Commands

```text
skillctl init
skillctl add <repository> --path <source-path> [--name NAME] [--ref REF]
skillctl add <repository> --file <path-to-SKILL.md> --name NAME [--ref REF]
skillctl sync [NAME]
skillctl check [NAME]
skillctl status
skillctl diff [NAME]
skillctl update [NAME]
skillctl remove <NAME>
skillctl list
```

Use `-g` or `--global` for global scope. `--json` emits one stable JSON document on stdout. `--dry-run` is accepted only for mutating commands and previews changes without writes. `--force` is accepted only by replacement operations (`sync` and `update`) and is required before overwriting changed managed content. Misleading flag/command combinations are errors.

`add` clones a Git URL or local Git repository, discovers its default branch when `--ref` is omitted, validates the selected skill, and pins the resolved commit and digest. Use `--path DIRECTORY` to vendor a complete skill tree, including its resources. Use `--file PATH/TO/SKILL.md --name DESTINATION` to install only that file as the destination's root `SKILL.md`; `--name` is required because it defines the containing folder. `--path` and `--file` are mutually exclusive. Local repository paths are stored as absolute paths so later commands work from nested project directories.

`sync` reproduces the exact locked commit without advancing it. `update` follows `source.ref`; use it only after changing the authoritative repository. `check` and `status` are network-free. `diff` compares installed files with the pinned source and reports deterministic added, removed, modified, and type-changed path records describing the installed tree relative to the pin; it may acquire the repository in an isolated temporary checkout. `remove` deletes only verified managed vendored content; a local entry is removed from the lock while its files remain.

For example, install only this repository's root skill without vendoring its Rust sources:

```sh
skillctl add https://github.com/zydtiger/skillctl.git \
  --file SKILL.md \
  --name skillctl-skill \
  --ref main
```

Safe workflow:

```sh
skillctl check
skillctl status
skillctl diff issue-delivery
skillctl update issue-delivery --dry-run
skillctl update issue-delivery
skillctl check
```

Global example: `skillctl --global list`. Global and project scopes never mix in one invocation. Do not edit hashes manually. Downloaded skill scripts are never executed; v1 rejects symlinks and special files.

## JSON and exit behavior

Every JSON response uses a deterministic envelope with `ok`, `scope`, `lock_file`, `skills`, `changes`, and `errors`. Command-specific records add fields without changing those envelope fields. JSON mode prints exactly one document to stdout. Exit code 0 means success (and a clean check); nonzero covers invalid locks, integrity mismatches, unsafe inputs, source failures, refused overwrites, and invalid flag use.

## Development

Required handoff gates:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
cargo install --path . --root "$(mktemp -d)"
python3 /Users/zyd/.codex/skills/.system/skill-creator/scripts/quick_validate.py .
```

Tests use temporary local Git repositories and isolated homes; they require neither a network connection nor the planned `agent-workflows` repository.

## Version 2 limitations

Version 2 has no hosted registry, dependency solver, hooks, symlinks, automatic commits/publication, permission-based immutability, or binary self-update. Diff output is path-oriented rather than a line-level unified patch. Git authentication and transport are delegated to the system `git` executable.
