// ABOUTME: UniFFI bindgen CLI for generating Swift/Kotlin bindings
// ABOUTME: Run with: cargo run --bin uniffi-bindgen -- generate --library target/release/libcoven_client.dylib --language swift --out-dir ./bindings

fn main() {
    uniffi::uniffi_bindgen_main()
}
