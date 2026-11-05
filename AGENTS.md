# PROTEUS agent harness

## Start

- Read `README.md`, `docs/architecture.md` and `docs/harness.md` before changes.
- Track work with Beads: `bd ready --json`, `bd show <id>`, then `bd update <id> --status in_progress`.
- If `bd` is missing, report it and retain a concrete handoff; do not invent issue IDs or claim synchronization.
- Use `bd prime` for the installed version's workflow. Husky owns Git hooks; initialize Beads with `--skip-hooks --skip-agents`.
- Main branch is `trunk`. All npm packages are private and named `@proteus-int/*`.

## Small changes and atomic commits

- One task and one independently reviewable concern per change. Avoid unrelated refactors and dependency churn.
- Use small, atomic Conventional Commits: `feat(person-fabric): ...`, `fix(intelligence): ...`, `test(harness): ...`.
- Review the staged diff. Stage explicit paths; never include secrets, generated databases, build output or unrelated work.
- Do not bypass hooks, force-push, rewrite other contributors' history, or commit failed checks.
- After a slice is complete, report the behavior, checks actually run and remaining limitations.

## TDD

- For behavior changes: write a focused regression test, run it and confirm the intended failure, implement the smallest change, then refactor with tests green.
- A test failing because a tool cannot start is not the red phase. Do not claim a test ran without its result.
- Validate the affected package and its dependents with `pnpm check:package <name>`; use `pnpm check:changed` for a mixed change.
- Before pushing, run `pnpm check`. UI behavior also needs `pnpm test:e2e`; native changes need `pnpm check:rust` and `pnpm build:desktop`.
- When changing the harness, add negative fixtures demonstrating that prohibited changes are rejected.
- CI is the full gate. Never weaken checks or add blanket exclusions to make a change pass.

## Architecture and naming

- `architecture/proteus.c4` is the source of truth for package dependencies. Update the model, manifests and tests together.
- No package cycles, relative cross-package imports or deep imports. Use public `@proteus-int/<package>` APIs.
- Desktop follows FSD: `app → pages → widgets → features → entities → shared`; layers may skip lower layers. No upward or cross-slice imports.
- Cross-slice imports use `index.ts`. `app` and `shared` are not sliced; shared segments can reference each other.
- Use kebab-case for paths, package names, persisted custom fields, CSS variables and SCSS variables. CSS Modules use BEM: `model-settings__row--active`, accessed as `styles['model-settings__row']`.
- Language constraints: TypeScript identifiers use camelCase/PascalCase, Rust identifiers use snake_case, and third-party API/config keys retain their required spelling. Required filenames (`AGENTS.md`, `README.md`, `Cargo.toml`, `Cargo.lock`, `PULL_REQUEST_TEMPLATE.md`) and generated platform icons are explicit exceptions.

## Product boundaries

- Read `docs/requirements.md` for the original board and full application scope.
- Model selectors belong in PROTEUS settings, per component plus a global apply-all control. Brain Scoring is deterministic and has no model selector.
- Preserve turquoise/violet persona palettes, the gray central logo head and pixel-style native window controls.
- Provider integrations stay behind ports. Unconfigured adapters must fail explicitly; no fabricated model output or network activity from demo settings.
- Native executable embeds the frontend and invokes the user's installed Codex/Kimi CLIs through Rust IPC. No separate application Node server; authentication stays with each CLI.
- Use only permitted public source material; separate facts from hypotheses and remove direct identifiers before Person Fabric.
- Traces mean observable timing and answer summaries, not private model reasoning. BSC is the board's experimental metric, not a validated human intelligence measure.
- Keep provider credentials out of frontend code, storage and Git. Installed CLIs own their authentication; do not read or copy their token files.

## Finish

- Review `git diff --check` and relevant test results.
- Update/close the Beads issue only when its acceptance criteria are met. Export a handoff snapshot with `bd export -o .beads/issues.jsonl` when task state changes.
- Local Dolt state is ignored by Git. A committed JSONL snapshot is interchange, not live synchronization or a full database backup.
