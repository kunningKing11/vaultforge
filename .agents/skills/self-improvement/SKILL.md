---
name: self-improvement
description: Review completed VaultForge wallet changes for documentation and maintenance drift, then update only the supporting information whose accuracy or usefulness materially depends on those changes. Use after modifying frontend or Rust code, wallet contracts, providers, transactions, configuration, workflows, dependencies, tests, or project structure.
---

# Self Improvement

Keep VaultForge's supporting project information aligned with the implemented wallet without creating documentation churn or overstating chain support.

## Post-change workflow

1. Finish and verify the primary implementation before starting this review. Do not use documentation edits to conceal incomplete wallet behavior or failing checks.
2. Inspect the final diff and identify effects users or maintainers need to understand, including changed wallet lifecycle behavior, provider/RPC behavior, transaction states, frontend/backend contracts, commands, configuration, dependencies, tests, CI, release automation, or file layout.
3. Find the canonical references for those effects. Read `AGENTS.md` and the relevant sections of `README.md` and `ROADMAP.md` before editing.
4. Apply the decision gate below. If no supporting information needs an update, make no extra changes.
5. Make the smallest coherent edits needed to keep the affected references accurate.
6. Re-read the implementation and documentation together, inspect the final diff, and run lightweight checks appropriate to the files changed.

## Decision gate

Update supporting information only when at least one of these is true:

- An existing statement, command, example, path, or diagram became false or misleading.
- A user-visible wallet behavior or maintainer-visible workflow would otherwise be difficult to discover or operate correctly.
- A roadmap item changed status because a real wallet capability was implemented, removed, or deliberately marked unavailable.
- A durable project convention, dependency, workflow, architecture description, data contract, or directory layout changed.
- Public command documentation, configuration comments, help text, examples, or CI instructions no longer match the implementation.

For wallet work, be especially careful to update descriptions only when the corresponding chain-specific derivation, balance, fee, signing, broadcast, and status behavior is actually present. Do not turn frontend scaffolding, simulator compatibility, or a network registry entry into a claim of production support.

## Likely targets

Inspect only targets related to the change:

- `README.md`, `ROADMAP.md`, and any relevant files under `docs/`
- `AGENTS.md` when a durable wallet architecture, security, or contributor convention changed
- Frontend/backend contract notes, command usage, configuration examples, and public docstrings
- `package.json`, `src-tauri/Cargo.toml`, `lefthook.yaml`, and `scripts/` when commands or verification behavior changed
- `.github/workflows/` when quality, release, caching, lockfile, or artifact behavior changed
- Dependency and lockfile guidance when installation or reproducibility behavior changed

Prefer updating the canonical source over duplicating the same explanation in several files.

## Restraint rules

- Do not rewrite, reformat, or reorganize unrelated documentation for consistency or style alone.
- Do not add speculative chain support, roadmap promises, version bumps, timestamps, or changelog entries unless the repository requires them.
- Do not document every private implementation detail; focus on stable wallet behavior and operationally useful context.
- Do not modify generated artifacts when their source can be updated instead.
- Preserve unrelated user changes and follow repository-specific validation, response, and commit instructions.
- Do not claim that local balance mutation, fake signatures, or simulated fees represent real wallet behavior.
- Do not commit or push follow-up edits unless the user has authorized it.

## Completion report

Report which supporting files changed and why. If the decision gate found no useful update, state that the existing VaultForge documentation remains accurate and that no follow-up edit was needed.
