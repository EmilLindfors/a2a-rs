---
name: refactor_to_hexagonal
description: Guide an incremental migration of an existing Rust codebase toward ports-and-adapters / hexagonal architecture. Use when the user wants to refactor a tangled service, extract a domain core, introduce port traits, or split a monolithic module into domain/port/adapter layers without a big-bang rewrite.
---

# Refactor to Hexagonal (Rust)

Drive an **incremental** migration from a flat or layered-by-framework Rust codebase to one organized by ports and adapters. The goal is small, reviewable PRs — never a big-bang rewrite.

Read `rules/hexagonal_architecture.md` first; this skill operationalizes those rules into a migration sequence.

## When to recommend this refactor

Recommend hex when you see at least two of:
- Business logic interleaved with HTTP handlers or DB queries.
- A test suite that needs a running database or HTTP mock to exercise core logic.
- Difficulty swapping infrastructure (e.g. "we want to try Postgres alongside SQLite" or "we want a CLI version of this service").
- Errors from `sqlx`, `reqwest`, `axum`, etc. propagating up through what should be business code.

Do **not** recommend it for:
- Small CLIs or scripts where there is no real "domain."
- Pure libraries that already have no I/O.
- Codebases where the dominant problem is something else (perf, concurrency, type safety) — fix that first.

## Migration sequence

Drive the refactor in this order. Each step should be a separate, mergeable change.

### Step 1 — Carve out a `domain/` module

- Find the data types the business cares about (entities, value objects, status enums) and move them into `domain/`.
- Strip framework attributes from these types (`#[derive(sqlx::FromRow)]`, `#[serde(rename_all)]` for HTTP, `axum` extractors). If serialization is needed in *every* adapter, keep `serde` derives; if it's only the HTTP adapter, define a DTO there and map.
- Introduce newtypes for primitive-typed identifiers and bounded values (`struct TaskId(String)`, `struct Port(u16)`). Parse at construction.
- Define a single `DomainError` enum with `thiserror`. Initially it can have a catch-all variant; tighten over time.

**Done when**: `cargo check -p <crate> --no-default-features` compiles the domain module in isolation, with no third-party I/O crates pulled in.

### Step 2 — Extract one port

Pick the **smallest, highest-pain** capability first. Storage is usually a good first target.

- Define a `trait` in `port/<capability>.rs` whose methods take and return **domain types only**.
- Methods return `Result<T, DomainError>`, not the adapter's error type.
- Keep the trait small. If it has more than ~6 methods, you probably picked too coarse a slice — split it.

**Done when**: the trait compiles with only domain types in scope, and no infrastructure crates are imported in the port module.

### Step 3 — Wrap the existing implementation as the first adapter

- Move the existing concrete code (the SQL queries, the HTTP handlers) into `adapter/<concern>/`. Don't rewrite it — move and rename only.
- Make the existing concrete type `impl` the new port trait. Convert its internal errors to `DomainError` at the trait boundary using `#[from]` variants.
- Keep the concrete `pub` type available behind a feature flag (`sqlite`, `http-client`).

**Done when**: the rest of the application still calls the same concrete type by name, but now you *could* substitute another impl.

### Step 4 — Add the in-memory adapter

- Implement the port trait with a `HashMap`/`Vec`-backed type in `adapter/<concern>/in_memory.rs`. Expose it from the crate root, **not** behind `#[cfg(test)]`.
- This is the lever that makes the whole refactor worth doing: now domain and application tests run without infrastructure.

**Done when**: at least one previously-integration test can be rewritten as a fast unit test using the in-memory adapter.

### Step 5 — Invert dependencies at call sites

Now go to each consumer of the concrete adapter and replace it with the port trait.

- For each function that previously took `&SqliteTaskStore`, change it to `<S: TaskStore>` or `&dyn TaskStore`. Prefer the generic by default; reach for `dyn` only on cold paths or heterogeneous collections (see `rust_best_practices.md` rule 2).
- The call site (typically `main` or a builder) is the one place that still names the concrete adapter.

**Done when**: a `grep` for the concrete adapter type returns only its definition, its `impl` blocks, and the composition root.

### Step 6 — Add the application/service layer

Only after at least one port is fully inverted:

- Move use-case orchestration into `application/` or `services/`. These functions take ports as generic parameters.
- Remove `#[cfg(feature = "…")]` from this layer. If you can't, a leak from step 5 wasn't actually fixed — go back.

### Step 7 — Repeat

Pick the next port (auth, notifications, transport) and run steps 2–5 again. Each capability is its own small PR.

## Anti-patterns during migration

Flag and refuse to do these when the user asks:

- **Boil-the-ocean refactor.** "Let's redesign the whole crate at once." No. One port at a time.
- **Port-per-technology.** Don't introduce `trait HttpStore` and `trait SqliteStore` — that's the *adapter* layer leaking up. There is one `TaskStore` port; there are many adapters.
- **Generic-everywhere over-engineering.** Don't add a port trait for capabilities that have exactly one implementation and no test seam to gain. Hex is justified by *substitutability* (real impl, fake impl, alternative impl). If none of those exist, the trait is overhead.
- **Anaemic domain.** Don't extract domain types into `domain/` and leave all the logic in adapters. Pull the rules that operate on those types in with them.
- **Hex for a CLI tool.** A 200-line CLI that reads a file, transforms it, and writes a file does not need ports.

## Output format

When asked to plan or perform a migration, produce:

1. **Assessment** — 2–4 sentences on whether hex is justified here and which capability is the right first target.
2. **Migration plan** — numbered steps mapped onto the sequence above, with file paths and approximate diff sizes.
3. **First-PR diff or patch** — if asked to actually do the work, perform **only Step 1 (or Step 2 if Step 1 is already done)** and stop there. Do not chain steps in one change.

Source for the underlying Rust patterns: https://microsoft.github.io/RustTraining/rust-patterns-book/
