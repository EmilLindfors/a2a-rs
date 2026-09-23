fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/a2a.proto");
    println!("cargo:rerun-if-changed=build.rs");

    // Tell it to use the protoc binary provided by protoc-bin-vendored
    unsafe {
        std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().unwrap());
    }

    // The generated message types are public API, so their field types are
    // pinned to the shapes buffa 0.3 emitted rather than left to buffa's
    // defaults, which changed in 0.9: singular message fields back on
    // `MessageField<T>` (a `Box`, not `Inline<T>`), and map fields on
    // `std::collections::HashMap<K, V>` with the std hasher (not foldhash).
    // Wire and JSON encoding are the same either way.
    let mut buffa = connectrpc_build::CodeGenConfig::default();
    // `buffa_config` replaces connectrpc-build's own defaults wholesale, and
    // JSON is the one it turns on.
    buffa.generate_json = true;
    buffa.pointer_fields = vec![(".".into(), buffa_codegen::PointerRepr::Box)];
    buffa.map_fields = vec![(
        ".".into(),
        buffa_codegen::MapRepr::Custom("::std::collections::HashMap".into()),
    )];

    // Generate connectrpc client and server code, along with buffa message types
    connectrpc_build::Config::new()
        .buffa_config(buffa)
        .files(&["proto/a2a.proto"])
        .includes(&["proto"])
        .compile()?;

    Ok(())
}
