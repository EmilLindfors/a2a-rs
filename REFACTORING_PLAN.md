# Refactoring Plan: Capability Decomposition, Dispatch, and Idiomatic Modernization

This plan covers three intertwined refactors of the `a2a-rs` port layer:

1. **Capability decomposition** — split the bloated `AsyncTaskManager` trait into focused capability traits, and reconcile push-config CRUD with the existing `AsyncNotificationManager`.
2. **Dispatch refactor** — remove viral generics from `DefaultRequestProcessor` and handlers by switching to `Arc<dyn …>` at the composition edge.
3. **Idiomatic Rust modernization** — newtype IDs, extension-trait validation helpers, and other ecosystem-standard patterns.

> **Stability posture: greenfield / internal.** All crates are `0.3.0`; every consumer is in-workspace. A grep for concrete implementors finds **5 `impl AsyncTaskManager` blocks** (`InMemoryTaskStorage`, `SqlxTaskStorage`, `TestBusinessHandler`, `AutoStorage`, `SimpleAgentHandler`) — all inside this workspace, no external crates. Per `.claude/skills/api_stability_posture` and `CLAUDE.md`, the correct strategy is to **break the trait directly, fix all call sites in one PR, and bump to `0.4.0`**. There is no `#[deprecated]` shim, no bridge trait, and no multi-phase non-breaking rollout — that machinery would be protecting consumers who don't exist.

---

## Background: what the current code looks like

- `AsyncTaskManager` (`a2a-rs/src/port/task_manager.rs:90-220`, gated behind `#[cfg(feature = "server")]`) carries **17 methods**: CRUD, history/metadata, validation, `list_tasks_v3`, and four v1.0.0 push-config methods. The four push-config methods all have default impls returning `A2AError::UnsupportedOperation`.
- `AsyncNotificationManager` (`a2a-rs/src/port/notification_manager.rs:92`) already covers per-task push setup, retrieval, removal, plus URL-validation and `*_validated` convenience defaults. The push-config CRUD on `AsyncTaskManager` is a second, drifting take on the same capability.
- `InMemoryTaskStorage` and `SqlxTaskStorage` each implement **three** async traits (`AsyncTaskManager` + `AsyncNotificationManager` + `AsyncStreamingHandler`) on a single struct.
- `DefaultRequestProcessor<M, T, N, A, S>` (`a2a-rs/src/adapter/business/request_processor.rs:33`) has **five** generic parameters with viral `Send + Sync + 'static` bounds, all `Arc`-wrapped internally.
- `ReimbursementHandler<T>` requires `T: AsyncTaskManager + AsyncStreamingHandler + Clone + Send + Sync + 'static`. Other handlers follow the same pattern.
- All IDs (task, context, push-config) are passed as `&str` — stringly typed.

---

## Phase 1 — Capability Decomposition

Split `AsyncTaskManager` and reconcile push-config into the existing `AsyncNotificationManager`. Names follow the project's hex-arch convention (`.claude/rules/hexagonal_architecture.md`): no `Port` suffix, no technology words in port names.

### 1.1 New trait shape

```rust
// port/task_manager.rs

#[async_trait]
pub trait AsyncTaskLifecycle: Send + Sync {
    async fn create(&self, id: &TaskId, ctx: &ContextId) -> Result<Task>;
    async fn get(&self, id: &TaskId, history: Option<u32>) -> Result<Task>;
    async fn update_status(&self, id: &TaskId, state: TaskState, msg: Option<Message>) -> Result<Task>;
    async fn cancel(&self, id: &TaskId) -> Result<Task>;
    async fn exists(&self, id: &TaskId) -> Result<bool>;
}

#[async_trait]
pub trait AsyncTaskQuery: Send + Sync {
    async fn list(&self, params: &ListTasksParams) -> Result<ListTasksResult>;
    async fn get_metadata(&self, id: &TaskId) -> Result<serde_json::Map<String, serde_json::Value>>;
}
```

Note the method renames (`cancel_task` → `cancel`, `get_task` → `get`): once the trait name carries the noun, the method name shouldn't repeat it.

The `: Send + Sync` supertrait bound **stays**. These are server-side ports stored behind `Arc<dyn …>` and shared across an async runtime — they are `Send + Sync` in every real use. Keeping the bound on the trait means the `Arc<dyn AsyncTaskLifecycle>` fields in Phase 2 don't each have to repeat `+ Send + Sync`. (This is why the original §3.5 "move bounds to the use site" idea was dropped: it adds verbosity at every use site to buy a single-threaded flexibility no caller wants.)

### 1.2 Reconcile push-config into `AsyncNotificationManager`

The four v1.0.0 push-config methods move **out** of `AsyncTaskManager`. Push-notification config is one capability, so it belongs on `AsyncNotificationManager` — but **do not simply concatenate** the old single-config trio and the new multi-config trio into one six-method trait. That overlap is itself the drift to fix, and a six-method no-default trait re-creates the all-or-nothing coupling this refactor is trying to eliminate (see §1.4).

Two coherent options; **pick (a) unless there's a reason not to**:

**(a) Multi-config subsumes single-config (preferred).** The v1.0.0 multi-config CRUD is a superset. Collapse to one trait expressed in terms of the richer model; the legacy single-config helpers become thin conveniences in the `*Ext` trait (§1.4), not core methods.

```rust
// port/notification_manager.rs

#[async_trait]
pub trait AsyncNotificationManager: Send + Sync {
    async fn set_config(&self, config: &TaskPushNotificationConfig) -> Result<TaskPushNotificationConfig>;
    async fn get_config(&self, params: &GetTaskPushNotificationConfigParams) -> Result<TaskPushNotificationConfig>;
    async fn list_configs(&self, params: &ListTaskPushNotificationConfigsParams) -> Result<Vec<TaskPushNotificationConfig>>;
    async fn delete_config(&self, params: &DeleteTaskPushNotificationConfigParams) -> Result<()>;
}
```

**(b) Two capability traits if the models genuinely diverge.** If single-config-per-task and multi-config-per-task are distinct capabilities a handler might want independently, split them — `AsyncNotificationConfig` (single) and `AsyncNotificationConfigSet` (multi) — rather than forcing every notification adapter to implement both. This keeps each trait under the ~6-method line in hex-arch rule 2.

Whichever is chosen, the goal is a trait (or pair) where the method count stays small and there is no conceptual duplication between "set the config" and "set a config."

### 1.3 No `UnsupportedOperation` default methods on the new traits

The whole point of splitting is so an adapter implements only what it supports. Default methods returning `Err(UnsupportedOperation(...))` defeat that — the trait surface still demands them. The new core traits have **no** such defaults; an adapter that doesn't support a capability simply doesn't implement that trait.

This is exactly why §1.2 must not stack all six push-config methods on one trait: doing so while dropping defaults would force every notification adapter to implement multi-config CRUD it may not support — the all-or-nothing trap, relocated.

### 1.4 Move validation/convenience helpers to extension traits

Validation (`validate_task_params`, `get_task_validated`, `cancel_task_validated`) and the notification `*_validated` / `has_task_notification` conveniences are wrappers over the primitives. Idiomatic Rust separates them via the `*Ext` pattern used across the ecosystem (`StreamExt`, `FutureExt`, `IteratorExt`) — a blanket impl gives every implementor the conveniences for free while keeping the core trait small and `dyn`-friendly:

```rust
#[async_trait]
pub trait AsyncTaskLifecycleExt: AsyncTaskLifecycle {
    async fn get_validated(&self, params: &TaskQueryParams) -> Result<Task> {
        params.validate()?;
        self.get(&params.id, params.history_length).await
    }

    async fn cancel_validated(&self, params: &TaskIdParams) -> Result<Task> {
        params.validate()?;
        self.cancel(&params.id).await
    }
}
impl<T: AsyncTaskLifecycle + ?Sized> AsyncTaskLifecycleExt for T {}
```

Apply the **same** treatment to `AsyncNotificationManager`'s existing convenience defaults (`set_*_validated`, `get_*_validated`, `has_task_notification`, `send_test_notification`) — an `AsyncNotificationManagerExt`. Result: core traits stay small; the convenience layer is automatic for every implementor; mocks only stub the primitives.

> Behavioral defaults that are *not* pure validation — `notify_task_status_update` / `notify_task_artifact_update` — are a separate question. They encode real delivery behavior, not convenience. Decide deliberately whether they belong on the core trait, an `*Ext`, or a separate notifier capability; don't sweep them into `*Ext` by reflex.

### 1.5 Do **not** introduce a marker "mixin" trait

An umbrella `trait AsyncTaskManager: AsyncTaskLifecycle + AsyncTaskQuery + AsyncNotificationManager {}` with a blanket impl looks convenient but:

- nothing implements it directly (existing impls split into the focused traits regardless),
- handlers bounded by the umbrella regain the all-or-nothing coupling the split is trying to eliminate,
- it hides which capability a handler actually needs.

Express requirements at use sites instead: `T: AsyncTaskLifecycle + AsyncTaskQuery` reads exactly the capabilities the handler exercises.

---

## Phase 2 — Dispatch Refactor

The viral-generic problem is not in handlers — it's in `DefaultRequestProcessor<M, T, N, A, S>` (five generic parameters). This is the highest-leverage fix in the plan.

### 2.1 Use `Arc<dyn …>` at the composition edge, generics nowhere else

```rust
pub struct DefaultRequestProcessor {
    message_handler: Arc<dyn AsyncMessageHandler>,
    task_lifecycle:  Arc<dyn AsyncTaskLifecycle>,
    task_query:      Arc<dyn AsyncTaskQuery>,
    notifications:   Arc<dyn AsyncNotificationManager>,
    streaming:       Arc<dyn AsyncStreamingHandler>,
    agent_info:      Arc<dyn AgentInfoProvider>,
}

impl DefaultRequestProcessor {
    pub fn new(
        message_handler: impl AsyncMessageHandler + 'static,
        task_lifecycle:  impl AsyncTaskLifecycle + 'static,
        task_query:      impl AsyncTaskQuery + 'static,
        notifications:   impl AsyncNotificationManager + 'static,
        streaming:       impl AsyncStreamingHandler + 'static,
        agent_info:      impl AgentInfoProvider + 'static,
    ) -> Self {
        Self {
            message_handler: Arc::new(message_handler),
            task_lifecycle:  Arc::new(task_lifecycle),
            task_query:      Arc::new(task_query),
            notifications:   Arc::new(notifications),
            streaming:       Arc::new(streaming),
            agent_info:      Arc::new(agent_info),
        }
    }
}
```

`impl Trait` in argument position keeps construction ergonomic without leaking generics into the type. The `Send + Sync` bounds ride in via the trait supertrait (§1.1), so the field types stay clean.

### 2.2 Why `dyn`, not enum dispatch, and not generics

The original sketch proposed `enum StorageAdapter { InMemory, Sqlx }`. Two reasons that's the wrong tool here:

1. **Feature-flag conflict.** `SqlxTaskStorage` lives behind `#[cfg(feature = "sqlx-storage")]`. A `StorageAdapter` enum would need `#[cfg]` on its variants, pushing feature flags into shared composition code (violates hex-arch rule 5).
2. **`async_trait` already allocates.** With `#[async_trait]`, every method call already goes through `Box<dyn Future>`. The vtable-vs-static-call delta is one indirect call per RPC — negligible against the downstream HTTP/SQL round-trip the call performs. The port boundary is a cold path.

Why `dyn` over the *current* generics: the five generic parameters are viral — they infect `DefaultRequestProcessor`'s type, every handler that holds one, and every call site, for zero runtime benefit on an I/O-bound boundary. Collapsing them to `Arc<dyn …>` removes that surface entirely. (Note: this is *not* justified by an "open plugin set" — there are only 5 in-workspace implementors. The justification is the cold-path + viral-generic ergonomics, not extensibility.)

Enum dispatch stays in the toolbox for hot paths inside adapter internals if profiling ever justifies it. It is the wrong tool for the port boundary.

### 2.3 Handler structs lose their generic too

```rust
// before
pub struct ReimbursementHandler<T>
where T: AsyncTaskManager + AsyncStreamingHandler + Clone + Send + Sync + 'static
{ task_manager: T, ... }

// after
pub struct ReimbursementHandler {
    task_lifecycle: Arc<dyn AsyncTaskLifecycle>,
    streaming:      Arc<dyn AsyncStreamingHandler>,
    ...
}

impl ReimbursementHandler {
    pub fn new(
        task_lifecycle: impl AsyncTaskLifecycle + 'static,
        streaming:      impl AsyncStreamingHandler + 'static,
    ) -> Self { ... }
}
```

The `Clone` requirement disappears (cloning `Arc<dyn …>` is an atomic increment).

---

## Phase 3 — Idiomatic Rust Modernization

### 3.1 Newtype IDs (apply "parse, don't validate" to the codebase's own identifiers)

The project's own `rust_best_practices.md` rule 4 demands newtypes for domain concepts, but task/context IDs are `&str` everywhere. The trait split is the moment to fix this:

```rust
// domain/ids.rs

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl FromStr for TaskId {
    type Err = A2AError;
    fn from_str(s: &str) -> Result<Self> {
        if s.trim().is_empty() {
            return Err(A2AError::ValidationError {
                field: "task_id".into(),
                message: "Task ID cannot be empty".into(),
            });
        }
        Ok(Self(s.to_owned()))
    }
}

impl AsRef<str> for TaskId { fn as_ref(&self) -> &str { &self.0 } }
impl fmt::Display for TaskId { /* ... */ }
```

Same shape for `ContextId`, `PushConfigId`. Pay-offs:

- "is the id empty?" runtime checks delete themselves (validation moves to the boundary, once).
- `cancel(context_id, task_id)` vs `cancel(task_id, context_id)` argument-order bugs become compile errors.
- Public API gets self-documenting signatures.

**Two caveats to scope before committing:**

- **`#[serde(transparent)]` bypasses `FromStr`.** A deserialized `TaskId` skips the empty-check — "parse, don't validate" only holds if construction *always* goes through `FromStr`/`TryFrom`. Either implement a custom `Deserialize` that routes through `FromStr`, or treat deserialized IDs as a validated-at-the-RPC-boundary concern and document that the constructor is the only validating path.
- **Blast radius is wider than the port methods.** Params structs (`TaskQueryParams`, `TaskIdParams`, …) and the generated/`buffa` wire views currently hold `String` IDs. Decide up front whether the newtypes live *in* the domain structs (one conversion at deserialization) or *only* in port signatures (convert at the boundary on every call). The former is cleaner but touches the generated-type seam; confirm the generated types can carry newtypes or plan the boundary conversion explicitly.

### 3.2 `Result` alias in the domain module

Every signature reads `Result<X, A2AError>`. Add the standard one-liner used by `std::io`, `serde_json`, and `sqlx`:

```rust
// domain/error.rs  (re-exported from the crate root)
pub type Result<T> = std::result::Result<T, A2AError>;
```

Trim the signatures while you're touching them all anyway.

### 3.3 Keep `#[async_trait]` on both trait definitions and the boxed impls

Native `async fn` in traits is stable, but **not object-safe**. Because Phase 2 stores these ports as `Arc<dyn AsyncTaskLifecycle>` etc., the trait *definitions* must keep `#[async_trait]` for `dyn` compatibility — and so must the concrete impls that are dispatched through those vtables. A native `async fn` impl produces an opaque future that does **not** match the `Pin<Box<dyn Future>>` signature the vtable expects; it won't compile through `dyn`.

```rust
// trait definition: #[async_trait] — needed for dyn compat
#[async_trait]
pub trait AsyncTaskLifecycle: Send + Sync { ... }

// concrete impl: ALSO #[async_trait] — it is dispatched via Arc<dyn …>
#[async_trait]
impl AsyncTaskLifecycle for InMemoryTaskStorage {
    async fn create(&self, id: &TaskId, ctx: &ContextId) -> Result<Task> { ... }
    // ...
}
```

> There is **no** allocation saving available on the concrete side here: the future is boxed regardless because the call goes through the vtable. Dropping `#[async_trait]` from a concrete impl is only valid for a trait that is *never* boxed — which these are not. (An earlier draft of this plan claimed `InMemoryTaskStorage` could drop it and save an allocation; that was wrong given Phase 2.)

If a future trait is genuinely *always* monomorphized (never `Arc<dyn …>`), it can use native `async fn` and drop `#[async_trait]` entirely — but none of the traits in this refactor qualify.

### 3.4 Method naming in narrower traits

Once methods live on a capability-scoped trait, the noun-prefix is redundant. `lifecycle.cancel(&id)` beats `lifecycle.cancel_task(&id)`. Apply consistently when splitting:

| Old | New |
|---|---|
| `cancel_task(&id)` | `AsyncTaskLifecycle::cancel(&id)` |
| `get_task(&id, hist)` | `AsyncTaskLifecycle::get(&id, hist)` |
| `update_task_status(...)` | `AsyncTaskLifecycle::update_status(...)` |
| `task_exists(&id)` | `AsyncTaskLifecycle::exists(&id)` |
| `list_tasks_v3(...)` | `AsyncTaskQuery::list(...)` |
| `get_task_metadata(...)` | `AsyncTaskQuery::get_metadata(...)` |

The `_v3` suffix in particular is technical debt that the rename retires.

---

## Phase 4 — Cross-port orchestration via capability mixins, and the service/transport split

Phases 1–3 make each port small and independently implementable. Phase 4 addresses the dual question that falls out of that: **where does behavior that spans two ports live, and is `DefaultRequestProcessor` actually the application service it looks like?** This phase is governed by the new `.claude/rules/hexagonal_architecture.md` §9 ("Composing ports: services first, mixins for cross-port behavior that travels").

### 4.0 Two smells this phase fixes

1. **`DefaultRequestProcessor` conflates two layers.** It both orchestrates the ports (`send_message`, `cancel_task`, …) *and* is the ConnectRPC transport adapter — it `impl A2aService`, speaks `connectrpc::Context`/`ConnectError`, decodes `buffa` views, and `map_err`s domain errors to wire errors (`adapter/business/request_processor.rs`). The use-case orchestrator that depends only on port traits is an **application** concern; the ConnectRPC/buffa glue is an **adapter** concern. They are currently the same struct.
2. **The storage adapter self-broadcasts.** `InMemoryTaskStorage::cancel` calls `self.broadcast_status_update(...)` *inside* the persistence mutator (`adapter/storage/task_storage.rs:386`). Persistence is doing streaming orchestration internally — which is the only reason the storage adapter is forced to implement `AsyncStreamingHandler` at all. Cross-port orchestration ("commit, then announce") belongs on the composed service, not buried in an adapter.

### 4.1 Capability mixin for cross-port behavior

"Update status **and** broadcast it" spans `AsyncTaskLifecycle` + `AsyncStreamingHandler`. Express it once as a blanket-impl'd mixin over **accessor ingredients**, not as a method bespoke to one struct (prototyped in `a2a-rs/src/application/task_status_broadcast.rs`):

```rust
pub trait HasTaskLifecycle { fn lifecycle(&self) -> &dyn AsyncTaskLifecycle; }
pub trait HasStreaming     { fn streaming(&self)  -> &dyn AsyncStreamingHandler; }

#[async_trait]
pub trait TaskStatusBroadcast: HasTaskLifecycle + HasStreaming + Send + Sync {
    async fn update_and_broadcast(&self, id: &TaskId, state: TaskState, msg: Option<Message>)
        -> Result<Task, A2AError>
    {
        let task = self.lifecycle().update_status(id, state, msg).await?;
        // build TaskStatusUpdateEvent from `task`, then:
        self.streaming().broadcast_status_update(id.as_str(), event).await?;
        Ok(task)
    }
}
impl<T: HasTaskLifecycle + HasStreaming + Send + Sync + ?Sized> TaskStatusBroadcast for T {}
```

Rules that keep this hexagonal (see §9): the accessor returns are bounded by **port traits** (`&dyn AsyncTaskLifecycle`), never adapters; the default touches only those ports + pure domain constructors; the mixin attaches to the **composed assembly** at the edge, never to an inner adapter. This is *not* the §1.5 umbrella trap — ingredients are accessors, not capabilities, and nothing bounds its storage on the mixin.

**Testability payoff:** a "partial platform" rig wires only the two ingredients over an in-memory adapter and gains exactly `update_and_broadcast` — tested with no transport and no processor. A host exposing only one ingredient fails to compile at the call site, not at runtime.

### 4.2 Split `DefaultRequestProcessor` into an application service + a transport adapter

The mixin needs a host that holds both ports. After Phase 2 that host exists (the processor's `Arc<dyn …>` fields), but it's the wrong *layer* to host it while it's still the ConnectRPC adapter. Split the roles:

```rust
// application/task_service.rs  — inner; mixes ports, speaks domain + A2AError only
pub struct TaskService {
    message_handler: Arc<dyn AsyncMessageHandler>,
    task_lifecycle:  Arc<dyn AsyncTaskLifecycle>,
    task_query:      Arc<dyn AsyncTaskQuery>,
    notifications:   Arc<dyn AsyncNotificationManager>,
    streaming:       Arc<dyn AsyncStreamingHandler>,
    agent_info:      Arc<dyn AgentInfoProvider>,
}
impl TaskService {
    pub async fn cancel(&self, id: &TaskId) -> Result<Task, A2AError> { /* port calls, no connectrpc */ }
}
impl HasTaskLifecycle for TaskService { fn lifecycle(&self) -> &dyn AsyncTaskLifecycle { self.task_lifecycle.as_ref() } }
impl HasStreaming     for TaskService { fn streaming(&self)  -> &dyn AsyncStreamingHandler { self.streaming.as_ref() } }
// → TaskService gains `update_and_broadcast` for free.

// adapter/transport/connectrpc.rs  — outer; thin glue over TaskService
#[async_trait]
impl A2aService for ConnectRpcAdapter {
    async fn cancel_task(&self, ctx, req) -> Result<(Task, Context), ConnectError> {
        let id: TaskId = req.to_owned_message().id.parse().map_err(map_err)?;
        let task = self.service.cancel(&id).await.map_err(map_err)?;
        Ok((task, ctx))
    }
}
```

`map_err`, `map_status_update`, `map_metadata`, the buffa view decoding, and **`NoopStreamingHandler`** (a concrete `AsyncStreamingHandler` — an adapter) all stay on the transport side. Only port orchestration crosses inward. Once `TaskService::update_and_broadcast` owns the "commit then announce" step, the storage mutators (§4.0.2) drop their internal broadcast and the persistence adapter no longer needs to be a streaming handler.

### 4.3 Sequencing: this is a follow-up PR, not part of the Phase 1–3 PR

- The **mixin module + accessor impls** (§4.1) are small and reuse Phase 2's fields. They can ride along in the Phase 1–3 PR or land immediately after — low risk, additive. **✅ Landed:** `application::task_status_broadcast` (`HasTaskLifecycle`, `HasStreaming`, `TaskStatusBroadcast`) is wired into the crate and the accessors are implemented on `DefaultRequestProcessor`; a `compile_fail` doc test pins the one-ingredient-doesn't-compile guarantee. Not yet *consumed* in the request flow — that is the §4.2 work below.
- The **service/transport split** (§4.2) touches the transport layer and the storage adapters' broadcast behavior. It is structurally larger than the trait refactor and should be its **own PR (target `0.5.0`)**, sequenced *after* Phase 2 lands the `Arc<dyn …>` fields it builds on. Bundling it into the Phase 1–3 PR would blur a clean trait-layer change with a layering move.
  - **✅ Landed (service extraction):** `application::TaskService` now owns the six ports and all use-case orchestration (`send_message`, `send_streaming_message`, `get`, `list`, `cancel`, `subscribe`, push-config CRUD, `extended_agent_card`), speaking only domain types + `A2AError`. It hosts the `HasTaskLifecycle`/`HasStreaming` accessors (moved off the processor), so it owns `update_and_broadcast`. `DefaultRequestProcessor` is now a thin ConnectRPC transport adapter that decodes `buffa` views, delegates to the service, and re-encodes — its public constructors are unchanged, so no call sites moved. `map_*` helpers and `NoopStreamingHandler` stay transport-side. Deviation from the §4.2 sketch: the transport adapter kept the name `DefaultRequestProcessor` and its `adapter/business/` location rather than being renamed to `ConnectRpcAdapter`/relocated to `adapter/transport/connectrpc.rs` — pure cosmetic churn the greenfield posture doesn't demand, and it spares ~20 call sites a mechanical rename. Revisit if a second transport (e.g. raw HTTP/JSON) ever needs the same service.
  - **✅ Landed (§4.0.2 storage self-broadcast removal):** `InMemoryTaskStorage`/`SqlxTaskStorage` `update_status`/`cancel` are now persistence-only — they no longer call `broadcast_status_update`. "Commit then announce" moved to the mixin hosts: `TaskService::cancel` uses `cancel_and_broadcast` (added to `TaskStatusBroadcast` alongside `update_and_broadcast`), and the message handlers that drive transitions during `process_message` now host the mixin and route through `update_and_broadcast` — `DefaultMessageHandler` gained a streaming port + `HasTaskLifecycle`/`HasStreaming` impls, and `ReimbursementHandler` (which already held both ports) implements the accessors and broadcasts at all five transition sites including the background worker. Tests in `application::task_status_broadcast` pin the new contract: a bare `storage.update_status`/`cancel` notifies no subscriber, while routing through the mixin announces each mutation exactly once (which also retired a latent double-broadcast). The behavioral coupling §4.0.2 targeted (mutation silently triggering streaming as a side effect) is gone; agents that drive `update_status` directly on storage now opt into broadcasting by hosting the mixin.
  - **✅ Landed in 0.4 (final struct-split):** The storage adapters no longer implement `AsyncStreamingHandler`. Streaming fan-out was extracted into a dedicated `adapter::streaming::InMemoryStreamingHandler` (owns the subscriber registry), and push-webhook delivery became its own `AsyncPushNotifier` port (backed by `PushNotificationRegistry`, with `PushNotificationSender` as the swappable backend seam). The `TaskStatusBroadcast` mixin gained a `HasPushNotifier` ingredient so "commit → announce to subscribers → fire push" is orchestrated in one place. `InMemoryTaskStorage`/`SqlxTaskStorage` are now persistence + push-config CRUD only, and hand out their notifier via `push_notifier()`. Spec-compliant behavior change: subscribing no longer replays current task state (the initial snapshot is delivered by the service/transport). This was originally scoped for 0.5; pulled into 0.4.

---

## Migration Plan (single PR)

The posture is greenfield/internal (see top), so this is **one breaking PR** to `0.4.0`, not a phased non-breaking rollout. The work is mechanical because there are only 5 implementors and all call sites are in-workspace. Sequence the edits so the workspace is buildable at the end of each step, but ship them together.

1. **Domain primitives.** Add `TaskId`/`ContextId`/`PushConfigId` newtypes in `domain/ids.rs` (with the §3.1 deserialization decision made). Add `pub type Result<T> = std::result::Result<T, A2AError>;`.
2. **New traits.** Define `AsyncTaskLifecycle`, `AsyncTaskQuery`, and their `*Ext` traits in `port/task_manager.rs`. Reconcile push-config into `AsyncNotificationManager` per §1.2 (option a or b) and add `AsyncNotificationManagerExt`. Keep `#[async_trait]` on every definition. **Delete `AsyncTaskManager` outright** — no deprecation marker, no bridge impl.
3. **Composition edge.** Switch `DefaultRequestProcessor` to `Arc<dyn …>` fields + `impl Trait` constructor args (§2.1). Switch `ReimbursementHandler` and other handler structs to non-generic `Arc<dyn …>` (§2.3).
4. **Implementors.** Rewrite the 5 `impl AsyncTaskManager` blocks as the focused impls (`AsyncTaskLifecycle` + `AsyncTaskQuery`, and notification impls where relevant): `InMemoryTaskStorage`, `SqlxTaskStorage`, `TestBusinessHandler`, `AutoStorage`, `SimpleAgentHandler`. Keep `#[async_trait]` on each (§3.3). Switch `&str` ID params to `&TaskId`/`&ContextId`. Apply the §3.4 renames.
5. **Call sites.** Update `a2a-agents`, `a2a-client` tests, `a2a-mcp`, and the examples that *reference* the trait/methods (≈22 files) to the new names and signatures.
6. **Release.** Bump `a2a-rs` to `0.4.0`. CHANGELOG entry with the §3.4 rename table and the push-config reconciliation note.

---

## Validation checklist

- [ ] `cargo check --workspace --all-features`
- [ ] `cargo check --workspace --no-default-features` (catches `#[cfg]` regressions in ports/domain — recall `a2a-rs` itself requires default features)
- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-features -- -D warnings`
- [ ] Grep confirms `AsyncTaskManager`, `cancel_task`, `get_task`, `list_tasks_v3`, and `&str` task-ID params return **nothing** (the trait is deleted, not shimmed)
- [ ] Public API diff reviewed; breaks documented in CHANGELOG

---

## What this plan deliberately does not do

- **No deprecation shim / bridge trait / phased rollout.** The repo is `0.3.0` with only in-workspace consumers; backward-compat machinery would protect an audience that doesn't exist. Break directly, fix call sites, bump `0.4.0`.
- ~~**Does not split the concrete storage structs.**~~ **Done in 0.4 (§4.3, final).** The storage adapters shed their streaming and push-delivery roles: `InMemoryStreamingHandler` now owns the subscriber registry, and the `AsyncPushNotifier` port owns webhook delivery. `InMemoryTaskStorage`/`SqlxTaskStorage` are persistence + push-config CRUD only.
- **Does not introduce enum dispatch.** Reserved for hot adapter internals if profiling justifies it; wrong tool at the port boundary.
- **Does not migrate trait *definitions* to native `async fn`.** They are `dyn`-dispatched, so `#[async_trait]` stays on definitions and on the boxed impls alike (§3.3).
- **Does not move `Send + Sync` to use sites.** Kept as supertrait bounds (§1.1) — these ports are always shared across an async runtime, so use-site repetition buys nothing.
- **Does not change transport, storage backends, or authentication *in the `0.4.0` PR*.** The Phase 1–3 PR is a trait-layer refactor only. The transport-touching service/adapter split and the storage adapter shedding its streaming role are deferred to **Phase 4** as a separate `0.5.0` PR (§4.3) — not in scope here, but no longer "out of scope" for the plan as a whole.
