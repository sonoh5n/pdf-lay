fn main() {
    // On macOS, allow undefined symbols when building Python extension modules.
    // Python symbols are resolved at runtime by the Python interpreter.
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-arg=-undefined");
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-arg=dynamic_lookup");
}
