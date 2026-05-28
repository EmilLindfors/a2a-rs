---
name: hexagonal_review
description: Review a Rust codebase for hexagonal architecture compliance — dependency direction, port shape, adapter grouping, error boundaries, feature-flag placement, and testability. Use when the user asks to review architecture, audit layering, check ports-and-adapters discipline, or evaluate whether a Rust project's structure is sound.
---

# Hexagonal Review (Rust)

Audit a Rust codebase against the ports-and-adapters discipline defined in `rules/hexagonal_architecture.md`. This skill complements `architectural_review` (which covers Rust patterns generally) by focusing specifically on layering, boundaries, and dependency direction.

## What to inspect

Work through the codebase systematically. For each layer, check:

### Domain layer
- Are domain types free of I/O, async runtimes, and framework imports?
- Are domain errors defined with `thiserror` as exhaustive enums?
- Are invariants enforced at construction (`TryFrom`, newtypes) rather than re-validated everywhere?
- Does the domain compile with **no feature flags** enabled?

### Port layer
- Is each port a **narrow capability trait**, not a technology trait?
- Do port method signatures use **domain types only** — never `reqwest::Response`, `sqlx::Row`, `axum::extract::*`, raw `serde_json::Value` blobs, or generic byte buffers?
- Do port return types use the **domain error type**, not adapter errors or `Box<dyn Error>` / `anyhow::Error`?
- Are sync and async variants both warranted? (Don't duplicate by reflex.)
- Is the port trait object-safe only if you actually use `dyn` dispatch on it? If not, drop the constraint and gain flexibility.

### Application / service layer
- Do services depend on **port traits only**, never on concrete adapter types?
- Are concrete adapters constructed elsewhere (in `main`, in a wiring module) and **injected** into the service?
- Is the service free of `#[cfg(feature = "…")]` gates? Feature gating belongs at the adapter boundary.

### Adapter layer
- Are adapters grouped by **technical concern** (`transport/`, `storage/`, `auth/`) rather than by port name?
- Does each adapter convert its lower-level errors (`reqwest::Error`, `sqlx::Error`) into the **domain error** before returning across the port boundary?
- Are framework-specific re-exports at the crate root properly `#[cfg(feature = "…")]`-gated?
- Is there an **in-memory adapter** for every port, available outside `#[cfg(test)]` so downstream users can test against it too?

### Composition root
- Is there exactly one place (typically `main`, a `bootstrap` module, or a builder) where concrete adapters are picked and wired together?
- Does that place use generics (`<T: TaskManager>`) for static dispatch by default, falling back to `Arc<dyn TaskManager>` only when heterogeneity or plugin extension is genuinely required?

## Smells — flag and explain

When you find any of these, name the file, quote the offending signature, and explain the leak:

1. **Domain dependency leak** — `domain/*.rs` importing an adapter-layer crate.
2. **Port leaking technology** — a trait method returning `reqwest::Response`, taking `axum::extract::Json<T>`, or exposing a raw SQL row.
3. **Error leaking up** — `Result<T, sqlx::Error>` or `anyhow::Result<T>` in a library-internal port.
4. **Service constructing adapters** — `MyService::new()` calling `HttpClient::new(...)` directly.
5. **Adapter folder shaped by port** — `adapter/task_manager/` instead of `adapter/storage/in_memory_task.rs`.
6. **Feature-gated port** — `#[cfg(feature = "http")] pub trait MyPort` (gate the impl, not the trait).
7. **God port** — a single trait with 12+ methods spanning unrelated capabilities. Split it.
8. **Untyped swamp at the boundary** — ports passing `Vec<u8>`, `serde_json::Value`, or stringly-typed config across the seam instead of validated newtypes.

## Output format

Produce a review with three sections:

1. **Summary** — overall verdict in 2–4 sentences. Where is the architecture strong, where is it leaking.
2. **Findings** — numbered list. Each finding cites `path/to/file.rs:line`, names the smell from the list above (or describes a new one), and proposes a concrete fix.
3. **Optional next steps** — if multiple findings share a root cause (e.g. "the domain error type doesn't have a variant for storage failures, so adapters are leaking `sqlx::Error` upward"), call that out as the single highest-leverage change.

Keep findings actionable. Do not list cosmetic style nits — those belong to `refactor_idiomatic_rust`.

## When this skill is NOT the right tool

- If the user is asking about generic Rust idioms (generics vs. dyn, error handling style, iterator chains), invoke `refactor_idiomatic_rust` or `architectural_review` instead.
- If the user wants a step-by-step migration of a non-hexagonal codebase **toward** ports and adapters, use `refactor_to_hexagonal`.

Source for the underlying patterns: https://microsoft.github.io/RustTraining/rust-patterns-book/
