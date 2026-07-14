# Project guidance

## Purpose and ownership

`skillctl` is an independent Rust CLI that materializes reproducible Codex skills from strict YAML locks. A `mode: vendored` destination is owned by `skillctl`, is pinned to an immutable Git commit, and must match its deterministic digest and marker. A `mode: local` destination belongs to the consuming project: validate its shape, but never replace, hash-enforce, or delete its files. Committed project snapshots let a fresh clone use skills without installing the CLI.

## Required reading and documentation ownership

Before changing behavior, read this file, the relevant README command/schema sections, root `SKILL.md`, `Cargo.toml`, the affected source modules, and their tests. Keep contracts and their documentation synchronized:

- Update `AGENTS.md` when project organization, safety rules, handoff gates, or contribution practice changes.
- Update `README.md` when user-visible commands, schema, output, installation, workflows, or limitations change.
- Update `SKILL.md` when an agent's operational use of the CLI changes; keep it concise and distributable.
- Update `Cargo.toml` for direct dependency/build changes and regenerate `Cargo.lock`; never hand-edit the lock.
- Update module documentation and neighboring code when responsibility boundaries or invariants change.
- Add or adjust tests with every corresponding behavior, error, schema, safety, transaction, or output contract change.

Documentation and tests are part of each contract, not follow-up cleanup.

## Source organization

- `src/cli.rs`: Clap parsing, flag compatibility, command dispatch, and orchestration.
- `src/error.rs`: structured command failures that preserve machine-readable error results.
- `src/scope.rs`: project upward discovery, explicit init root, isolated global resolution, and paths.
- `src/lockfile.rs`: strict versioned YAML schema, path-versus-file selectors, conditional validation, names, collisions, and safe relative paths.
- `src/source.rs`: system-Git acquisition, revision/default-branch resolution, directory or single-file snapshot export, tree and `SKILL.md` validation. Never execute acquired content.
- `src/digest.rs`: deterministic, framed, sorted source-tree hashing and comparisons.
- `src/install.rs`: marker handling, entry state, read-only operations, staging, and mutation policy.
- `src/transaction.rs`: atomic lock writes, destination replacement, backup, rollback, and cleanup.
- `src/output.rs`: human rendering and the stable single-document JSON API.
- `tests/`: CLI-level integration coverage with temporary Git repositories, projects, and homes; no network or user-global state.

Keep responsibilities narrow and files reasonably sized. Prefer explicit data passed between layers over hidden process state.

## Safety invariants

- Never execute downloaded scripts, hooks, binaries, or other skill content.
- Reject unsafe UTF-8 paths, absolute destinations, empty components, traversal, symlinks, and special files. Permit `source.path: .` only as the explicit repository-root directory selector.
- Never overwrite modified vendored content without an explicit supported `--force` operation after reporting differences.
- Never mutate a `mode: local` skill directory.
- Keep the lock and installed tree mutually consistent with atomic writes or tested rollback protection.
- Validate the full staged tree and digest before replacing anything. Exclude only the generated marker from source digesting.
- Keep read-only commands network-free except `diff`, whose isolated pinned checkout must never mutate installed state.
- Preserve stable, deterministic JSON because it is a public API.

## Validation gates

Before handoff, run all of:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
cargo install --path . --root <temporary-directory>
python3 /Users/zyd/.codex/skills/.system/skill-creator/scripts/quick_validate.py .
```

Also smoke-test the installed binary and inspect `git diff`/`git status`. Report exact blockers instead of claiming skipped checks passed.

## Git hygiene

Preserve unrelated user work and avoid destructive reset or checkout commands. Do not commit by default. Never push, publish, create/configure remotes, create issues or pull requests, merge, tag, or release without explicit approval.

Use concise imperative Conventional Commit-style subjects when commits are requested, optionally scoped: `prefix: summary` or `prefix(scope): summary`.

- `feat`: user-visible capability
- `fix`: incorrect behavior
- `docs`: documentation-only changes
- `refactor`: internal restructuring without behavior change
- `test`: tests or fixtures without production behavior changes
- `chore`: maintenance not covered elsewhere
- `build`: build system or dependency changes
- `ci`: continuous-integration configuration
- `perf`: measurable performance improvement
- `revert`: intentional reversal of an earlier commit

Choose one primary prefix for each focused commit. Avoid vague subjects such as `update files` or `misc fixes`.

## Work organization

Keep changes focused and pair behavior changes with tests. Separate schema/contract decisions from mechanical cleanup when that improves review. When a forge is later configured, use issues and pull requests as reviewable units and check existing work before proposing duplicates. Do not assume GitHub or Gitea; inspect repository configuration first.

## Skill distribution and compatibility

Root `SKILL.md` is authoritative and installs as `~/.agents/skills/skillctl-skill/SKILL.md`. Copying it does not install the binary.

Version the lock schema. Continue reading version 1 directory-source locks. New locks use version 2, which accepts exactly one of `source.path` or `source.file`; file selectors must target a repository-relative `SKILL.md`. Every supported version must fail clearly on unknown fields so misspellings cannot weaken guarantees. Preserve deterministic source resolution, tree hashing, ordering, and output. Treat stable JSON output as an API: additive changes require care and breaking changes require an explicit compatibility decision.
