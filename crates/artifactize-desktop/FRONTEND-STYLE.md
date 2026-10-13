# Frontend checklist

Rules for the desktop UI (Svelte 5, TypeScript). Data flows one way:
**state** (one source of truth) → **view** (derived) → **action** → back to state.
Items marked `S-` come from the Svelte docs' [Best practices](https://svelte.dev/docs/svelte/best-practices);
`U-` are this checklist's own.

## State

- **U-LIBRARY-OWNS**: domain state and decisions come from the artifactize library through the generated IPC bindings; the UI never re-implements them.
- **U-ONE-OWNER**: each fact, UI-only ones included, has exactly one owner: a class with `$state` fields, shared with `createContext`. A library snapshot is `$state.raw`, replaced whole.

## View

- **S-DERIVED**: everything computed from state or props is `$derived`, and pure.
- **S-EFFECT-ESCAPE**: `$effect` is an escape hatch, like `unsafe`: `{@attach}`, `createSubscriber`, `<svelte:window>` and event handlers come first. A remaining `$effect` has an `// EFFECT:` comment saying why, and never writes state.

## Action

- **U-ACTION-COMMAND**: a user action is an event handler that sends a library command; the result returns through the owner and the view follows. Instant feedback is a temporary [override of a `$derived`](https://svelte.dev/docs/svelte/$derived#Overriding-derived-values), rolled back on failure, never a second `$state`.
