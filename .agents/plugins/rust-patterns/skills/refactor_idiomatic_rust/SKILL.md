# Refactor Idiomatic Rust

This skill helps you refactor Rust code into idiomatic patterns based on the "Rust Patterns & Engineering How-Tos" guide.
When asked to refactor or review Rust code for idiomatic usage, apply the principles derived from:
https://microsoft.github.io/RustTraining/rust-patterns-book/

## Generics and Monomorphization
- Prefer **Generics (Static Dispatch)** when dealing with a closed set of types or when performance in hot loops is critical (e.g., millions of calls). Generics are zero-cost abstractions as they inline perfectly, but be mindful of code bloat.
- Prefer **Trait Objects (`dyn Trait` Dynamic Dispatch)** for cold paths, heterogeneous collections, plugin systems, or when the cost of monomorphization code bloat outweighs the minor cost of vtable lookup.
- Use **`const fn`** where possible for simple constructors or logic that can be evaluated at compile time, reducing runtime overhead.

## Traits and Composition
- Distinguish between **Associated Types** (when there is *one* output/result per implementing type, e.g., `Iterator::Item`) and **Generic Parameters** (when a type can implement the trait for *many* target types, e.g., `From<T>`).
- Consider using **Enum Dispatch** as a middle ground between Generics and `dyn Trait`. If the set of types is closed, placing them in an enum and implementing the trait on the enum gives static dispatch performance with dynamic-like flexibility without heap allocation or vtable cost.
- Apply the **Extension Trait** pattern to safely add methods to foreign types (types you don't own). Define a trait with your method and provide a blanket implementation (`impl<T: SomeBound> MyExtension for T`).
- Use **Blanket Implementations** carefully but effectively (e.g. `impl<T: Display> ToString for T`) to give trait implementations for free to any type matching the constraints.
- Maintain **Trait Object Safety Rules**: Traits meant for dynamic dispatch must not have `Self: Sized` bounds on methods meant for the vtable, no generic type parameters on methods, and no `Self` in return position (unless boxed).

## Implementation Details
- Provide concise, actionable suggestions for refactoring.
- Show code snippets comparing "before" (un-idiomatic) and "after" (idiomatic).
