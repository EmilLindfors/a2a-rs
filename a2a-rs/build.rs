fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../spec/a2a.proto");
    println!("cargo:rerun-if-changed=build.rs");

    // Tell it to use the protoc binary provided by protoc-bin-vendored
    unsafe {
        std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().unwrap());
    }

    // Generate connectrpc client and server code, along with buffa message types
    connectrpc_build::Config::new()
        .files(&["../spec/a2a.proto"])
        .includes(&["../spec"])
        .compile()?;

    Ok(())
}
