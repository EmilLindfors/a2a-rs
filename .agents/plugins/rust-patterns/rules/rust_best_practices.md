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

4. **Embrace Type Safety and Domain Modeling**
   - Use Newtypes (e.g., `struct UserId(u64)`) to enforce zero-cost compile-time boundaries between different domain concepts.
   - For complex system modeling, rely on Associated Types and Marker Traits to encode type states and invariants directly into the type system, ensuring invalid states are unrepresentable and cause compile errors, not runtime panics.

Source: https://microsoft.github.io/RustTraining/rust-patterns-book/
