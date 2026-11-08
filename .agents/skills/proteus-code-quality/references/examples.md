# Examples from the current PROTEUS tree

Use these owners as starting points, not a requirement to reproduce an old directory sketch.
The maintained [code map](../../../../docs/code-owners.md) links the actual modules and
review exercises. Preserve behavior before changing names or responsibilities.

## Package: Person Fabric

`fabricate-person.ts` owns task-free baseline orchestration and its prompt;
`adapt-person.ts` owns task adaptation; `parse-rules.ts` validates evidence-linked
rules; `parse-style.ts` handles style data; `neutral-profile.ts` owns the control.
`index.ts` explicitly exports the public API. The parser stays internal: tests and
internal consumers import its owner directly, without a new npm package or barrel.

Characterize baseline requests, adaptation inputs, evidence IDs/status and exact
source strings with the existing person-fabric tests. Repeat after a pure move
with `pnpm check:package person-fabric`. Prompt semantics and all-consumer field
migration are separate changes. A small related helper can remain with its caller.

## UI: ConnectionsPanel

`apps/desktop/src/features/connections/model/connection-session.ts` owns polling,
revision/disposal, pending GitHub timing, action state and QR clearing.
`connection-materials.ts` owns import and confirmed deletion through the same
activity gate. The UI owns rendering and local form/disclosure state; native
adapters remain in `shared/lib/connections.ts`.

The current UI constructs these models inside its SolidJS owner:

```tsx
const session = createConnectionSession(() => setEditing(false));
const materials = createConnectionMaterials(session.selected, session.activity, {
  onImported: (provider, source) => props.onImported?.(provider, source),
  onDataCleared: (provider) => props.onDataCleared?.(provider),
});
```

Pass changing values as getters, not `session.selected()` snapshots. Keep cleanup
with the timer/subscription. Existing `connection-session.test.ts` exercises late
polls, disconnect/import races, disposal, errors and provider deadlines with
synthetic ports and the real SolidJS client lifecycle. Do not introduce React
hooks, another `connection-polling.ts` merely to shorten a file, or a global store.

## Native: connection commands and external DTOs

`connections.rs` registers modules and keeps Tauri wrappers thin. The wrappers
clone the shared connection handle, run blocking scenarios and map errors safely.
`connection-session.rs`, provider-specific modules and the existing `social-http`,
`social-oauth`, `social-storage` owners retain their responsibilities.

Own Rust symbols stay snake_case; own wire fields are camelCase:

```rust
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionTiming {
    poll_after_ms: u64,
}
```

This is a naming example, not a request to add a timing DTO. External OAuth
`access_token` retains its required spelling. Stored data needs an explicit
versioned decoder; changing serde does not migrate an existing Keychain record.
Use shared TS/Rust wire fixtures plus legacy/collision tests, without live secrets.
Run Rust gates and the native build; browser mocks do not establish native behavior.
Keep required serialization gates and process-tree cleanup when moving code.
