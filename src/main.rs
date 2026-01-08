// In order to be compatible with both desktop, wasm, and android, the example is both a binary and a library.
// Just forward to the library in main

fn main() {
    // tracing_subscriber::fmt().init();

    usher_lib::main();
}
