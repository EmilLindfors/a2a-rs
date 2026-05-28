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

## General Design Feedback
- Promote "Parse, don't validate". Use the type system to enforce invariants across API boundaries (e.g., using `NonEmptyVec` or `ValidatedEmail` types) instead of relying on checks inside functions.
- Consider memory layouts and cache friendliness: suggest contiguous memory layouts and enum dispatch over pointer-chasing `Vec<Box<dyn Trait>>` when the set of variants is known and finite.

## Implementation Details
- During architectural review, look for places where untyped logic, massive enumerations, or poor trait configurations are causing friction or bugs.
- Suggest how to incrementally migrate toward Mixins or Typed Commands.
