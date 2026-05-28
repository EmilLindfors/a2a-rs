---
description: Review the current Rust diff (or specified files/PR) for idiomatic Rust patterns and hexagonal-architecture compliance, in one pass.
argument-hint: "[scope] — optional: 'diff' (default), 'branch', a path, or a PR number"
---

# /rust-review

Run a combined Rust-patterns and hexagonal-architecture review over the scope given in `$ARGUMENTS` (default: the working-tree diff against the merge base of the current branch).

## Procedure

1. **Determine scope.**
   - If `$ARGUMENTS` is empty or `diff`, review uncommitted changes plus committed changes on the current branch vs. its merge base with `main`/`master`. Use `git diff` and `git log` to enumerate touched files.
   - If `$ARGUMENTS` is `branch`, review every Rust file changed on the current branch vs. its merge base.
   - If `$ARGUMENTS` looks like a path or glob, review those files.
   - If `$ARGUMENTS` is a number (a PR), fetch that PR's diff via the available GitHub tooling and review it.

2. **First pass — idiomatic Rust.** Apply the `refactor_idiomatic_rust` skill. Flag: generics vs. `dyn` misuse, missing `?`/error-propagation cleanups, imperative loops that would read better as combinators, weak closure bounds, stringly-typed APIs, missing `impl Into`/`AsRef`/`Cow` opportunities, panic-prone code in expected-failure paths.

3. **Second pass — advanced architecture.** Apply the `architectural_review` skill. Flag: opportunities for capability mixins, typed commands replacing untyped buffers, mis-chosen concurrency primitives (Mutex where atomic/channel fits), unbounded channels, `lazy_static!` instead of `OnceLock`/`LazyLock`, library code returning `anyhow::Error`.

4. **Third pass — hexagonal compliance.** Apply the `hexagonal_review` skill against the touched files. Flag the smells listed in `rules/hexagonal_architecture.md` §8: domain leaking framework types, ports exposing technology, errors leaking up, services constructing adapters, feature flags on the wrong layer.

5. **Synthesize.** Combine findings into a single review. **Deduplicate** — if one root cause produces findings across multiple passes (e.g. "this port returns `reqwest::Error`" is both an error-architecture issue and a hex-boundary leak), report it once and tag it with both passes it came from.

## Output

```
## Summary
<2–4 sentences. Overall verdict. Highest-leverage change.>

## Findings

### 🟥 Blocking — <count>
- `path/to/file.rs:LL` — <pass tag> — <smell>. Fix: <concrete suggestion>.

### 🟧 Important — <count>
- `path/to/file.rs:LL` — <pass tag> — <smell>. Fix: <concrete suggestion>.

### 🟨 Suggestions — <count>
- `path/to/file.rs:LL` — <pass tag> — <smell>. Fix: <concrete suggestion>.

## Next step
<Optional. If multiple findings share a root cause, name the single change that resolves the most.>
```

Pass tags: `[idiomatic]`, `[architecture]`, `[hex]`.

## Rules

- Cite file paths and line numbers. Vague advice without locations is not a finding.
- Do not list cosmetic style preferences (formatting, naming bikesheds) — `rustfmt` and `clippy` own those.
- Be specific about the *why*. "Use `OnceLock`" is not enough; "Use `OnceLock` instead of `lazy_static!` — std-only, no macro, supports `const` initializers" is.
- If the diff has no real findings, say so in one sentence. Do not pad with weak suggestions.
- Never invent code that isn't there. If you suspect a problem in code that wasn't read, read it before flagging it.
