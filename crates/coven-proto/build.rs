// ABOUTME: Build script for generating Rust code from coven.proto.
// ABOUTME: Uses tonic-build to compile protobuf definitions into Rust types.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile the proto file from the submodule
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto-src/coven.proto"], &["proto-src"])?;

    // Rerun if the proto file changes
    println!("cargo:rerun-if-changed=proto-src/coven.proto");

    Ok(())
}
