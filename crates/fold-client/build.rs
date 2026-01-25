// ABOUTME: Build script for UniFFI binding generation
// ABOUTME: Generates Swift/Kotlin bindings from Rust types

fn main() {
    uniffi::generate_scaffolding("src/fold_client.udl").unwrap();
}
