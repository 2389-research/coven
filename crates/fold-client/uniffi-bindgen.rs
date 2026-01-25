// ABOUTME: UniFFI bindgen CLI for generating Swift/Kotlin bindings
// ABOUTME: Run with: cargo run --bin uniffi-bindgen -- generate --library target/release/libfold_client.dylib --language swift --out-dir ./bindings

fn main() {
    uniffi::uniffi_bindgen_main()
}
