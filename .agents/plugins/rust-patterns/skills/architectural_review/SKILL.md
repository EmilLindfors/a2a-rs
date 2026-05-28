# Architectural Review (Rust)

This skill helps you perform an architectural review of Rust code based on advanced patterns found in the "Rust Patterns & Engineering How-Tos" guide.
When asked to review system architecture or suggest broad improvements in a Rust project, apply these architectural patterns:
https://microsoft.github.io/RustTraining/rust-patterns-book/

## Capability Mixins (Composition over Inheritance)
- Rust doesn't have class inheritance. Instead, use traits with associated types and default method implementations to compose behaviors.
- Define "Ingredient" traits containing primitive operations (e.g. `HasBus { type Bus; fn bus(&self) -> &Self::Bus; }`).
- Define "Mixin" traits that require ingredient traits as supertraits, and provide default implementations for domain logic (e.g. `trait FeatureMixin: HasBus`).
- Use blanket implementations (`impl<T: HasBus> FeatureMixin for T {}`) so any type with the ingredient automatically gets the mixin behavior at compile time with zero overhead.

## Typed Commands (GADT-style Return Types)
- Avoid "Untyped Swamp" signatures where generic byte buffers (`Vec<u8>`) are passed around indiscriminately.
- Create Domain Newtypes to represent physical/logical constraints strongly (e.g., `Celsius(f64)`, `Rpm(u32)`).
- Use a Command Trait where the associated type dictates the specific return type of that command (e.g., `trait Command { type Response; }`).
- When a command is executed, its concrete type ensures the correct return format is parsed and returned natively, turning unit-confusion runtime errors into compile-time errors.

## Concurrency and Parallelism
- Understand the distinction between concurrency (tasks making progress) and parallelism (simultaneous execution).
- **Threads vs. Rayon**: Use OS threads (`std::thread::spawn`) for long-running background tasks. Use `rayon::par_iter()` for data parallelism when processing collections. Use scoped threads (`std::thread::scope`) to safely borrow stack variables across short-lived parallel tasks.
- **Shared State**: Use `Mutex<T>` for exclusive access in short critical sections. Use `RwLock<T>` for read-heavy workloads with rare writes. Use `AtomicU64` and similar atomic types for simple flags/counters to avoid lock overhead.
- **Message Passing**: Prefer `crossbeam-channel` over `std::sync::mpsc` in production for multi-consumer support and select macros. Use bounded channels to implement backpressure and avoid OOM issues. Employ the Actor pattern with channels to encapsulate complex shared mutable state instead of wrapping everything in a Mutex.

## Smart Pointers and Interior Mutability
- **Heap vs Stack**: Use `Box<T>` for single ownership heap allocation, `Rc<T>` for single-threaded shared ownership, and `Arc<T>` for thread-safe shared ownership. Use `Weak<T>` to break reference cycles.
- **Interior Mutability**: Use `Cell<T>` for `Copy` types, `RefCell<T>` for runtime borrow-checking within a single thread, and `Mutex<T>`/`RwLock<T>` for multi-threaded interior mutability.
- Utilize `OnceLock` or `LazyLock` over deprecated macros like `lazy_static!`.

## Error Handling Architecture
- **Library vs Application**: For libraries, use `thiserror` to define precise, matching-friendly enum types so consumers can programmatically handle them. For applications, use `anyhow` for dynamic error reporting and simple `.context()` propagation.
- Use `#[from]` inside `thiserror` enums to build effective error conversion chains.

## General Design Feedback
- **Parse, don't validate**: Use the type system to enforce invariants across API boundaries (e.g., using `TryFrom` to parse an untyped input into a validated Newtype like `Port` or `ValidEmail`) instead of relying on runtime checks inside business logic functions.
- Consider memory layouts and cache friendliness: suggest contiguous memory layouts and enum dispatch over pointer-chasing `Vec<Box<dyn Trait>>` when the set of variants is known and finite.
- Utilize Zero-Copy deserialization with Serde (`&'a str` fields) for high-performance read-heavy parsing to avoid unnecessary heap allocations.

## Implementation Details
- During architectural review, look for places where untyped logic, massive enumerations, excessive cloning, thread contention, or poor trait configurations are causing friction or bugs.
- Suggest how to incrementally migrate toward Mixins, Typed Commands, or better concurrency models.
