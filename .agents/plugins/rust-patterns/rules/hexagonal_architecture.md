# Hexagonal Architecture (Ports & Adapters) in Rust

When writing, reviewing, or refactoring Rust code that follows hexagonal architecture, enforce these rules. They complement `rust_best_practices.md` — apply both.

## 1. The Dependency Rule (non-negotiable)

Source code dependencies point **inward only**: `adapter → application → domain`. The domain knows nothing about the application; the application knows nothing about adapters. If a domain file `use`s an HTTP client, a database driver, or a web framework, that is a bug.

- **Domain** (`domain/`): pure types, value objects, domain errors, validation. No I/O, no async runtime, no framework types. `serde` derives are acceptable; `reqwest`/`axum`/`sqlx`/`tokio::net` are not.
- **Application** (`application/` or `services/`): use-case orchestration. Depends on **port traits**, never on concrete adapters. No `#[cfg(feature = "http-server")]` here — feature gating belongs at the adapter layer.
- **Ports** (`port/`): trait definitions, owned by the inner layers, expressing what the application needs from the outside world.
- **Adapters** (`adapter/`): concrete implementations of ports. The only layer allowed to import third-party I/O, framework, or driver crates.

## 2. Ports group by business capability, not by technology

A port is one trait per **capability** (`TaskManager`, `MessageHandler`, `NotificationManager`, `Authenticator`). Do **not** create `HttpPort`, `DatabasePort`, or `SqlitePort` — those are adapter concerns.

- Keep traits narrow. If a trait grows past ~6–8 methods, it is probably two capabilities.
- Provide sync **and** async variants only when both are genuinely needed by callers (`TaskManager` and `AsyncTaskManager`). Don't duplicate by reflex.
- Default method implementations on the trait are fine for cross-cutting validation that every adapter would otherwise repeat (e.g. `validate_task_params`).

## 3. Adapters group by technical concern

Within `adapter/`, sub-modules are organized by **what kind of outside world** they touch:

```
adapter/
  transport/     // HTTP, WebSocket, gRPC — protocol surfaces
  storage/       // databases, filesystem — persistence
  auth/          // JWT, OAuth2, API key — credentials
  business/      // composite/orchestrating adapters (e.g. a request processor)
  error/         // adapter-specific error types
```

One adapter implements one port. If an adapter implements two ports, ask whether it should be split.

## 4. Errors cross the boundary by conversion, never by leak

- Domain defines a domain error enum (`A2AError`, `MyDomainError`) with `thiserror`.
- Adapters define their own error type (`HttpClientError`, `SqlxStorageError`) with `thiserror`.
- The adapter's error type implements `From<…>` for the lower-level errors it wraps (reqwest, sqlx, etc.).
- At the port boundary, the adapter converts its error into the domain error before returning. Use `#[from]` variants on the domain error enum where the conversion is unambiguous.
- **Libraries** return strongly typed domain errors. **Binaries** may use `anyhow::Result` at the outermost layer (main, CLI handlers) but not inside library crates.
- Never let `reqwest::Error`, `sqlx::Error`, or `axum::Error` appear in a port trait signature.

## 5. Feature flags gate adapters, not domain or ports

- Domain and port modules must compile with **zero features** enabled.
- Each optional adapter lives behind its own feature (`http-server`, `sqlite`, `auth`).
- Re-exports at the crate root that depend on a feature must be `#[cfg(feature = "…")]`-gated.
- Do not feature-gate trait definitions themselves — gate only their implementations.

## 6. Testability is the point

- Domain logic is tested with plain unit tests. No mocks needed; no test doubles needed.
- Application services are tested against **in-memory adapter implementations** of their ports (`InMemoryTaskStorage`, `NoopAuthenticator`). Provide these in-memory adapters as first-class types, not just in `#[cfg(test)]`.
- Adapter tests are integration tests against the real outside system (or a containerised one), not against mocks of the framework.

## 7. Composition happens at the edge

- `main` (or a `bootstrap`/`wire` module) is the only place that picks concrete adapters and assembles them into the application.
- Application code accepts ports by generic parameter (`<T: TaskManager>`) or `Arc<dyn TaskManager>` — choose static dispatch by default, `dyn` only when you genuinely need heterogeneity or plugin-style extensibility (see `rust_best_practices.md` rules 2 and 3).
- Do not call `MyConcreteAdapter::new()` from inside a service. That couples the inner layer to the outer layer.

## 8. Smells to flag in review

- A `domain/` file that imports `reqwest`, `axum`, `sqlx`, `tokio::net`, or `tokio::fs`.
- A port trait whose method signature exposes an HTTP type, a SQL row, or a framework request/response.
- A `Box<dyn Error>` or `anyhow::Error` in a library-internal port return type.
- An application service that constructs a concrete adapter directly instead of receiving it.
- Adapter modules organized by *port name* (`adapter/task_manager/`) instead of by *technology* (`adapter/storage/`, `adapter/transport/`).
- A feature flag that gates anything in `domain/` or `port/`.
- A port that exists to model one specific technology (`PostgresPort`, `HttpPort`) — collapse it back into the capability it secretly is.
