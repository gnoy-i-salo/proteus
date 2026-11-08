# PROTEUS agent harness

## Start

- Read `README.md`, `docs/architecture.md` and `docs/harness.md` before changes.
- Track work with Beads: `bd ready --json`, `bd show <id>`, then `bd update <id> --status in_progress`.
- If `bd` is missing, report it and retain a concrete handoff; do not invent issue IDs or claim synchronization.
- Use `bd prime` for the installed version's workflow. Husky owns Git hooks; initialize Beads with `--skip-hooks --skip-agents`.
- Main branch is `trunk`. All npm packages are private and named `@proteus-int/*`.

## Kimi subagents

- Use the installed Kimi CLI as a subagent for independent, bounded implementation, testing or review tasks within the user's authorized scope. Run independent assignments in parallel while the main agent advances other work; keep dependent steps sequential.
- Check `kimi --help` for the installed version. Use a fresh non-interactive invocation (`kimi --prompt "<assignment>" --output-format stream-json`); do not resume an unrelated session. If Kimi is unavailable or cannot authenticate, report the limitation and continue locally without claiming delegation succeeded.
- Give each subagent the Beads issue ID, relevant context, required repository instructions and skills, explicit allowed files, acceptance criteria and required checks. Include `AGENTS.md` and the Start reading requirements; do not assume the CLI inherits this chat's context.
- Use a separate worktree for delegated edits, based on the agreed source state. Preserve existing uncommitted work and ensure required changes are present in that baseline. Assign one owner per file; shared contracts, manifests, architecture and migrations need one coordinated owner. Read-only reviews may use the current tree.
- The main agent owns Beads transitions and snapshot exports, coordinates assignments, reviews diffs and integrates accepted changes. Delegation does not expand permission to commit, push, publish, access accounts or change unrelated files.
- Keep credentials and private source corpora out of assignments and logs. Installed CLIs own authentication; never read or copy token files. Repository TDD, product boundaries and quality gates apply equally to Kimi.
- Require a handoff with changed files, behavior, actual check results and remaining limitations. Reject out-of-scope edits, weakened checks and unsupported claims that tests passed. The main agent inspects the result and runs relevant checks on the combined tree before closing the issue; a successful CLI exit alone is not acceptance.

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
- Apply [the code standard](docs/code-style.md) to all three release epics. Own TypeScript fields and variables use camelCase, types/components PascalCase; paths and npm package IDs remain kebab-case. This replaces the former persisted-field kebab-case rule.
- CSS Modules use `blockMore__elementMore_modificatorMaybe`: camelCase parts, `__` before an element, one `_` before a modifier; assign base and modifier together. Use `styles.modelSettings__row` and camelCase tokens (`--personaTurquoise`, `$personaTurquoise`). The former `--modifier` class spelling is obsolete.
- Rust identifiers stay snake_case/PascalCase; own serialized DTOs use camelCase after the coordinated boundary migration. External API/config/HTML/framework names retain their required spelling. Stable IDs/status values, source text, citations and opaque maps are not field names to rename.
- Decode exact legacy schemas only at their owning boundaries; version new writers and reject conflicting aliases/unknown versions without overwriting originals. Never recursively rename arbitrary JSON or publish both aliases in domain interfaces.
- Keep package `index.ts` exports-only; use direct relative imports inside a package. Separate SolidJS 1.9 scenario models from presentation and keep Tauri commands thin. Read the repository skill `.agents/skills/proteus-code-quality/SKILL.md` when changing packages, UI models, SCSS or native code. It supplements checks; it does not replace them.
- Required filenames (`AGENTS.md`, `README.md`, `SKILL.md`, `Cargo.toml`, `Cargo.lock`, `PULL_REQUEST_TEMPLATE.md`) and generated platform icons are explicit exceptions. Migration status and still-pending enforcement are recorded in `docs/code-style.md`; do not broaden exceptions to bypass existing gates.

## Product boundaries

- Read `docs/requirements.md` for the original board and full application scope.
- Model selectors belong in PROTEUS settings, per component plus a global apply-all control. Brain Scoring is deterministic and has no model selector.
- Preserve turquoise/violet persona palettes, the gray central logo head and pixel-style native window controls.
- Provider integrations stay behind ports. Unconfigured adapters must fail explicitly; no fabricated model output or network activity from demo settings.
- Native executable embeds the frontend and invokes the user's installed Codex/Kimi CLIs through Rust IPC. No separate application Node server; authentication stays with each CLI.
- Use permitted public source material or explicitly granted selected sources. Separate facts from hypotheses; preserve source names, authorship, relationships and exact wording through STALKER, Person Fabric and Person. Do not anonymize the shared task or persona context. Model-processing permissions, credential isolation and export restrictions remain independent controls.
- Traces include timing, answers and reasoning text explicitly exposed by Codex/Kimi CLI streams. Never extract encrypted/raw provider state or relabel an answer as reasoning. BSC is the board's experimental metric, not a validated human intelligence measure.
- Keep provider credentials out of frontend code, storage and Git. Installed CLIs own their authentication; do not read or copy their token files.

## Finish

- Review `git diff --check` and relevant test results.
- Update/close the Beads issue only when its acceptance criteria are met. Export a handoff snapshot with `bd export -o .beads/issues.jsonl` when task state changes.
- Local Dolt state is ignored by Git. A committed JSONL snapshot is interchange, not live synchronization or a full database backup.
