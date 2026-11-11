fn main() {
    // The packaged texture kernels include a C++ object even for BC7-only use.
    if std::env::var_os("CARGO_FEATURE_ENCODE").is_some()
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        let runtime = if std::env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
            "c++"
        } else {
            "stdc++"
        };
        println!("cargo:rustc-link-lib={runtime}");
    }
}
