fn main() {
    // Sparkle is linked only for the `sparkle` cargo feature (the .app
    // bundle build). A plain `cargo run`/`clippy` stays a bare binary with
    // no framework dependency — dyld would otherwise fail to launch it
    // outside a .app, since the only other rpath is ../Frameworks.
    let sparkle = std::env::var("CARGO_FEATURE_SPARKLE").is_ok();
    let target = std::env::var("TARGET").unwrap_or_default();
    if sparkle && target.contains("apple-darwin") {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-search=framework={manifest}/vendor/sparkle");
        println!("cargo:rustc-link-lib=framework=Sparkle");
        // .app bundle first, then the vendor dir so a feature build can
        // still run unbundled during development.
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{manifest}/vendor/sparkle");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
