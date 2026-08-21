# skillctl

`skillctl` is a Rust CLI for reproducibly managing Codex skills. It reads a declarative YAML lock, installs Git-pinned snapshots into `.agents/skills/`, detects local changes, and updates snapshots from their authoritative repositories. Already-vendored project skills remain usable after cloning without `skillctl`.

The ownership rule is deliberate: `mode: vendored` means `skillctl` owns and integrity-checks the destination; `mode: local` means the consuming project owns it and mutation commands never replace its files.

The lock mechanism is a `skillctl` convention, not an official built-in Codex lock-file feature.

`skillctl` runs on Linux and macOS. It uses POSIX file modes to record and restore the executable bit, and that bit is part of a tree's digest, so a host without those modes would compute a different digest for the same tree. Building elsewhere fails with an explicit message instead.

## Installation

Install the binary directly from the repository without cloning it. Prefer a tag so the installed binary is a known release:

```sh
cargo install --git https://github.com/zydtiger/skillctl.git --tag v0.2.1
```

Omit `--tag` to track the default branch, which may contain unreleased changes:

```sh
cargo install --git https://github.com/zydtiger/skillctl.git
```

To build from a local checkout instead, run this from the project root:

```sh
cargo install --path .
```

`cargo install` places the binary in `~/.cargo/bin`, which must be on `PATH`. Add `--force` to any of these commands to replace an already-installed binary.

Install the bundled agent skill under the required folder name, from the project root:

```sh
mkdir -p ~/.agents/skills/skillctl-skill
cp SKILL.md ~/.agents/skills/skillctl-skill/SKILL.md
```

The installed directory and the `SKILL.md` declared name are both
`skillctl-skill`. Copy `SKILL.md` again after updating this repository. Copying
it does not install or update the CLI binary; reinstall an updated binary with
one of the commands above.

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
      repository: https://example.com/user/shared-skills.git
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

Schemas v1 and v2 reject unknown fields, unsupported versions, unsafe destinations, invalid conditional fields, duplicate destinations, and duplicate declared skill names. A v1 vendored source requires `path`; a v2 source requires exactly one of `path` or `file`. A `file` value must be a safe repository-relative path ending in `SKILL.md`. Vendored directories contain `.skillctl-managed`; it is metadata and is excluded from source digests. Interpreter cache directories such as `__pycache__` are excluded too, so running a skill's script does not make it read as modified; they are also left out of installed trees.

Mutations validate content in isolated staging directories before replacement. Destination and lock changes use backups and rollback protection, and changed managed content is never replaced unless `sync` or `update` explicitly receives `--force`.

Project-vendored skill folders and `.agents/skills.lock.yaml` should normally be tracked together in the consuming project's Git repository. Global scope is machine-local unless `~/.agents` is separately managed in version control.

## Commands

```text
skillctl init
skillctl add <repository> --path <source-path> [--name NAME] [--ref REF]
skillctl add <repository> --file <path-to-SKILL.md> --name NAME [--ref REF]
skillctl sync [NAME]
skillctl check [NAME]
skillctl status [--offline]
skillctl diff [NAME]
skillctl update [NAME]
skillctl remove <NAME>
skillctl list
```

Use `-g` or `--global` for global scope. `--json` emits one stable JSON document on stdout. `--dry-run` is accepted only for mutating commands and previews changes without writes. `--force` is accepted only by replacement operations (`sync` and `update`) and is required before overwriting changed managed content. Misleading flag/command combinations are errors.

`add` clones a Git URL or local Git repository, discovers its default branch when `--ref` is omitted, validates the selected skill, and pins the resolved commit and digest. Use `--path DIRECTORY` to vendor a complete skill tree, including its resources. Use `--file PATH/TO/SKILL.md --name DESTINATION` to install only that file as the destination's root `SKILL.md`; `--name` is required because it defines the containing folder. `--path` and `--file` are mutually exclusive. Local repository paths are stored as absolute paths so later commands work from nested project directories.

`sync` reproduces the exact locked commit without advancing it. `update` follows `source.ref`; use it only after changing the authoritative repository. `diff` compares installed files with the pinned source and reports deterministic added, removed, modified, and type-changed path records describing the installed tree relative to the pin; it may acquire the repository in an isolated temporary checkout. `remove` deletes only verified managed vendored content; a local entry is removed from the lock while its files remain.

`update` reports one of three outcomes per entry, matching the classification `status` reports upstream:

| Outcome | Condition | Effect |
| --- | --- | --- |
| `no-op` | commit and digest both match, destination installed | nothing changes |
| `pin-only` | digest matches, commit advanced, destination installed | records the new commit and refreshes the managed marker; the installed destination is not replaced |
| `update` | digest differs, or the destination is missing | reinstalls the destination and records the new commit and digest |

A `pin-only` outcome occurs when an unrelated commit advances a shared `source.ref` without changing the selected skill. Because nothing is replaced, it neither rewrites installed files nor requires `--force` for a locally modified destination; such a destination keeps its local changes and continues to report `modified`. Use `diff` and `sync --force` to reconcile it deliberately.

Pin-only advances are written before any content update in the same run, so the lock and every refreshed marker stay consistent even when a later entry fails.

`check` is the deterministic offline integrity gate. It validates the lock and schema, destination safety, installed skill structure, managed markers, declared-name conflicts, and vendored digests without acquiring any source. A clean run prints one concise `OK` summary; any integrity failure is actionable and exits nonzero. It never reports update availability.

`status` is the lifecycle dashboard. By default it queries each unique repository/ref once, then compares every selected skill's upstream digest with its locked digest. Its `LOCAL` column reports `clean`, `modified`, `missing`, `invalid`, or project-owned `local` state. Its `UPSTREAM` column reports `current`, `update_available`, `source_advanced` when the ref moved but that selected skill did not change, `unreachable`, `invalid`, or `not_applicable` for local entries. An unreachable source does not discard local integrity results or make a successfully produced dashboard fail. Use `status --offline` to skip all source access; vendored entries then report `not_checked`, with one footer explaining the skipped upstream checks.

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

Every JSON response uses a deterministic envelope with `ok`, `scope`, `lock_file`, `skills`, `changes`, and `errors`. Command-specific records add fields without changing those envelope fields. JSON mode prints exactly one document to stdout.

Status skill records expose separate `local_status` and `upstream_status` fields plus `pinned_commit`, `upstream_commit`, `content_changed`, `upstream_details`, and `recommended_action`. `content_changed` is `true` or `false` after a successful vendored upstream comparison and `null` when it is not applicable or could not be checked. For compatibility, the earlier `state`, `commit`, and `digest` fields remain; `update_status` also remains as an alias of `upstream_status`, but no longer emits the ambiguous legacy value `unknown`. Consumers should migrate to the explicit fields. Status enum values use the underscore spellings shown above.

Exit code 0 means command success and, for `check`, a clean integrity result. Nonzero covers invalid locks, integrity mismatches, unsafe inputs, source failures in commands that require acquisition, refused overwrites, and invalid flag use. `status` reports an unreachable upstream source in the dashboard while exiting 0 because the lifecycle inspection itself completed; lock/schema failures still exit nonzero.

## Versioning

`skillctl` is pre-1.0 and follows semantic versioning under the `0.x` convention. The CLI, the `--json` document, and the lock schema are the public surfaces.

A breaking change to any of them bumps the minor version: a removed or renamed command or flag, a removed or repurposed JSON field, a changed exit-code meaning, or a lock schema change that an older `skillctl` cannot read. Everything else bumps the patch version, including fixes, additive JSON fields or values, new optional flags, and new commands.

Releases are Git tags named `vX.Y.Z` with GitHub Release notes describing the user-visible changes. The repository keeps no changelog file. Install a specific release with `cargo install --git … --tag vX.Y.Z`.

## Development

Formatting and Clippy run as commit hooks, and CI runs that same configuration
over every file. Install the runner once per machine:

```sh
uv tool install prek
prek install
```

Then run the checks the hooks do not carry:

```sh
cargo test
cargo build --release
cargo install --path . --root "$(mktemp -d)"
```

Tests use temporary local Git repositories and isolated homes; they require neither a network connection nor any external repository.

## Version 2 limitations

Version 2 has no hosted registry, dependency solver, hooks, symlinks, automatic commits/publication, permission-based immutability, or binary self-update. Diff output is path-oriented rather than a line-level unified patch. Git authentication and transport are delegated to the system `git` executable.
