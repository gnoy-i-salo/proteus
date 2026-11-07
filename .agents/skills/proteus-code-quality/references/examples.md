# Examples from PROTEUS

These examples describe the intended extraction from the observed baseline; they are not claims that the final modules already exist. Resolve current paths when applying them. Preserve outputs before changing names or behavior.

## Package: Person Fabric

Before: `packages/person-fabric/src/index.ts` contains `FabricatedPerson`, `parseRules`, long prompt schemas, `fabricatePerson`, `adaptPerson` and neutral-profile operations. Rules validate evidence references, disallow unknown/conflicting support, and distinguish profile rules from task skills.

After: `fabricate-person.ts` owns baseline orchestration and its prompt; `adapt-person.ts` owns task adaptation; `parse-rules.ts` owns evidence-linked validation; `neutral-profile.ts` owns the control. The local result type stays with its owning module. `index.ts` explicitly exports the public API. `parseRules` does not become public only to simplify internal imports. A tightly related helper can stay with its caller.

```ts
// index.ts after extraction
export { fabricatePerson } from './fabricate-person';
export type { FabricatedPerson } from './fabricate-person';
export { adaptPerson } from './adapt-person';
```

Characterization: run existing person-fabric tests first; capture task-free baseline request, adaptation input, evidence IDs/status and exact source strings. Repeat after extraction with `pnpm check:package person-fabric`. Keep prompt changes and all-consumer field migration out of the pure move.

## UI: ConnectionsPanel

Before: `apps/desktop/src/features/connections/ui/connections-panel.tsx` owns JSX plus polling, `revision`, `disposed`, pending GitHub timing, QR clearing, import and deletion. An asynchronous reply is applied only when its revision is current and the owner remains alive.

After: `model/connection-polling.ts` owns polling/timer lifetime; action state belongs to a narrowly scoped model when independently useful. UI owns rendering and disclosure state. The adapter remains `shared/lib/connections.ts`. Preserve revision checks and QR removal on action/error/disposal; don't put all existing code unchanged into one controller.

```tsx
// Called while the component's owner is active. Props stay reactive.
const polling = createConnectionPolling({
  selected: () => props.provider,
  statuses: connectionStatuses,
});
return <Show when={polling.error()}>{polling.error()}</Show>;
```

This is an interface sketch, not a replacement polling implementation. The model registers cleanup beside its timer/subscription, ignores late responses and handles rejected promises. Tests dispose the owner during a pending request, change selection before a reply and simulate pending polling intervals. Existing e2e verifies busy/error/QR states and keyboard behavior. No network is needed.

## Native: connection commands

Before: `apps/desktop/src-tauri/src/connections.rs` mixes connection state, provider-specific transitions and command wrappers; `social-oauth.rs`, `social-http.rs`, `social-storage.rs` already exist. The extraction must reuse them.

After: keep `configure_connection`/`begin_connection` as thin Tauri boundaries calling typed scenarios, preserve existing blocking-task strategy, and move independent provider branches to their owner. The command still validates inputs and maps failures without exposing tokens. Keep state locks away from long I/O when the actual concurrency invariant permits it. Do not replace a required serialization gate merely to make a lint happy.

```rust
// Own new DTO only; existing storage needs a versioned decoder first.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionTiming {
    poll_after_ms: u64,
}
```

An OAuth response with `access_token` retains that external spelling in its provider DTO. Rust `poll_after_ms` stays snake_case, while the own wire key is `pollAfterMs`. Test serialized keys and optional/null handling on both boundaries, plus timeout/cancel/provider failures. Run Rust gates and the native build; browser mocks cannot establish actual native behavior.
