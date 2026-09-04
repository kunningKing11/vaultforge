---
name: code-structure
description: Understand VaultForge's Tauri wallet architecture, frontend/backend boundaries, chain providers, wallet lifecycle, transaction flow, and repository layout. Use when the user asks about code structure or when structural context is needed before making changes.
metadata:
  version: "0.1.0"
  triggers:
    - codebase.?structure
    - codebase.?layout
    - codebase.?arch
    - project.?layout
    - repo.?structure
    - file.?structure
    - tauri.?architecture
    - frontend.?backend
    - wallet.?architecture
    - chain.?provider
    - rpc.?flow
    - transaction.?flow
    - how does.*work
    - what does.*do
    - explain.*code
    - understand.*code
---

# code-structure

## Agent instructions

When this skill is loaded, read the repository's actual architecture and project guidance before answering or changing code:

- `AGENTS.md` — authoritative wallet architecture, security requirements, supported-chain scope, module boundaries, data contracts, and verification requirements.
- `README.md` — project purpose, setup, development commands, and user-facing capabilities.
- `ROADMAP.md` — planned wallet, chain, and product work.

For implementation details, inspect the relevant source instead of assuming that a documented or exposed chain is fully implemented:

- `src/` — TypeScript frontend composition, views, events, commands, wallet API contracts, network definitions, and styling.
- `src-tauri/src/` — Rust application setup, Tauri commands, wallet lifecycle, storage, derivation, providers, transaction construction/signing, and tests.
- `src-tauri/Cargo.toml` and `package.json` — dependency and script boundaries.
- `.github/workflows/` and `lefthook.yaml` — CI, release, and pre-commit verification behavior.

Use `AGENTS.md` as the primary reference for architecture and security decisions. Treat `README.md` and `ROADMAP.md` as supporting project references, and verify both against the current implementation.

## Response guidance

When asked about the codebase, first identify which boundary the question concerns:

- Frontend shell and screen selection: `src/render.ts`, `src/views/`, `src/events.ts`, and `src/commands.ts`.
- Frontend contracts and chain presentation: `src/main.ts`, `src/networks.ts`, and related API/types modules.
- Rust application wiring: `src-tauri/src/lib.rs`, the thin desktop launcher in `src-tauri/src/main.rs`, and `src-tauri/src/commands/`.
- Wallet identity, encrypted persistence, and lifecycle: wallet, storage, encryption, and derivation modules.
- Chain-backed data and transfers: provider modules plus transaction, signing, broadcast, and status modules.
- Quality and release behavior: `lefthook.yaml`, `scripts/`, and `.github/workflows/`.

Describe the real data flow where relevant: user action -> frontend command/API boundary -> Tauri command -> wallet/provider/storage or transaction domain logic -> `WalletSession` or transaction result -> frontend state/rendering.

Point to specific files and responsibilities rather than duplicating large sections of code. Keep the distinction between provider-derived funds and local UI state explicit, and do not infer production chain support from a network list alone.
