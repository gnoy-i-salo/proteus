---
name: proteus-code-quality
description: Implement and refactor PROTEUS TypeScript packages, SolidJS 1.9 UI models, SCSS and native Rust with repository naming, ownership and compatibility rules. Use for changes to those areas, including RAG and UX work; preserve product scope and existing behavior.
---

# PROTEUS code quality

Read the repository `AGENTS.md` and `docs/code-style.md`. Inspect the relevant Beads issue and actual dirty tree before editing; shared contracts, IPC and CSS must have one coordinated owner across RAG, UX and quality work.

Choose the owner before extracting code. Package `index.ts` exposes explicit exports, internal imports point to owners, and external consumers use public package APIs. Keep small packages flat; split independent parsing, scenarios and transport without adding a universal `utils` directory or one module per trivial function. Local types remain local. Read [examples](references/examples.md) for concrete package, UI and Rust extractions.

Own TS fields use camelCase; types/components PascalCase; files/package IDs kebab-case. CSS is `blockMore__elementMore_modificatorMaybe`, with base and modifier applied together; own tokens use camelCase. Rust symbols remain snake_case; own new wire DTOs use camelCase. External keys, stable IDs/status values, source words and arbitrary maps are not names to normalize.

Before changing serialized fields, identify actual reader/writer/version and boundary owner. Add explicit legacy decoders and versioned encoders with red/green fixtures for old/new, collision, corruption, size, Unicode/whitespace and recovery. Never recursively rename arbitrary JSON or introduce both aliases into domain types. Keep original storage until the new value validates; migration must be idempotent. Do not claim an existing result reader when the format is export-only.

In SolidJS 1.9, scenario state and commands belong in the slice model; IPC is an adapter. Keep small visual signals in UI. Construct models inside the reactive owner, pass getters for changing props, and preserve cancellation, stale-result protection and cleanup. Derived values use functions/memos; effects manage side effects. Avoid React hooks, SolidJS 2 recipes and global stores introduced only to move code.

Tauri commands delegate to scenarios and convert errors safely. Preserve process-tree cleanup, timeout, permissions and credential ownership. Choose borrowing/cloning and locks from actual lifetime/concurrency needs; do not impose blanket bans on `clone` or `dyn Trait`. Review provider adapters and parsing separately. No new DI framework or whole Clippy pedantic profile without evidence.

Preserve BSC, approval protocol, task-free Fabric, frozen series, source identity and exact text, grants/holdout/revocation and export restrictions. Reasoning is only what the CLI explicitly exposes. Do not perform real provider calls, read token files or use synthetic answers in production.

For behavior, migrations and harness rules: focused failing test, smallest change, green. Tool startup failure is not red. For pure moves: characterize observable outputs before, repeat after; no forced failure. Then inspect the touched code for useful local simplification without broadening scope. Follow `pnpm check:package <name>` or `pnpm check:changed`; UI requires e2e, Rust requires `pnpm check:rust` and `pnpm build:desktop`. Preserve all existing gates and add negative fixtures for enforceable new restrictions. Never claim passing checks that were not run.

Use [evaluation cases](references/evaluation.md) to review this skill on real changes and [source decisions](references/sources.md) when updating its guidance. A skill is guidance; CI enforces the mechanically checkable rules. It grants no additional release or account permissions.
