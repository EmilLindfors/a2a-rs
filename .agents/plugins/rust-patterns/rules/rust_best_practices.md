# Rust Best Practices

When writing, reviewing, or refactoring Rust code, strictly adhere to these core rules derived from the Rust Patterns Book:

1. **Avoid Code Bloat via Monomorphization**
   - Be cautious of using generics on large functions that are instantiated with many types.
   - Use the "outline" pattern: Extract the non-generic core logic into a separate function that the generic function calls to minimize the duplicated monomorphized code.

2. **Prefer Static Dispatch (Generics/Enum Dispatch) over Dynamic Dispatch (`dyn Trait`)**
   - Default to using generics (`impl Trait` or `<T: Trait>`) since they are monomorphized, have zero cost, and can be aggressively inlined by LLVM.
   - For a closed, finite set of types, use Enum Dispatch (implementing the trait on an Enum with variants) to maintain static dispatch while collecting them in a homogeneous structure.

3. **Use `dyn Trait` Only When Necessary**
   - Use dynamic dispatch (e.g. `Box<dyn Trait>`) only when the set of types is open (e.g. plugin architectures), when you need heterogeneous collections of varied trait implementors, or on cold paths (like error logging) where binary size matters more than execution speed.

4. **Embrace Type Safety and Domain Modeling (Parse, don't validate)**
   - Use Newtypes (e.g., `struct UserId(u64)`) to enforce zero-cost compile-time boundaries between different domain concepts.
   - Parse untyped inputs (e.g., primitive integers or strings) at the boundary using `TryFrom` or `FromStr` into validated types, rather than passing raw types and validating them deep in the business logic.
   - For complex system modeling, rely on Associated Types and Marker Traits to encode type states and invariants directly into the type system, ensuring invalid states are unrepresentable and cause compile errors, not runtime panics.

5. **Choose the Right Concurrency and Synchronization Tools**
   - For simple message passing, use bounded channels (to create natural backpressure and prevent OOM).
   - Use `std::thread::scope` for parallel processing that needs to borrow local stack variables safely. Use `rayon` for data-parallel collection processing.
   - Prefer `std::sync::OnceLock` or `std::sync::LazyLock` for lazy initialization instead of older macros like `lazy_static!`.
   - Use Mutexes for short critical sections, RwLocks for read-heavy shared data, and Atomics for simple flags or counters.
   - For complex shared mutable state, strongly consider the Actor pattern using channels rather than scattering Mutexes.

6. **Distinguish Error Handling by Context**
   - Libraries must define strongly typed, exhaustive error enumerations using crates like `thiserror` to allow consumers to programmatically react to failures.
   - Binaries and applications should favor the `anyhow` crate to propagate errors efficiently with `Result` and `.context("human readable context")`.
   - Never use `panic!` or `unwrap()` for expected failures; reserve them strictly for unrecoverable programming bugs.

Source: https://microsoft.github.io/RustTraining/rust-patterns-book/
