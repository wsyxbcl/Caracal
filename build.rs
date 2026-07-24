fn main() {
    // Embed the application icon into the Windows executable. Gated on the
    // target OS (not the host) so cross/native Windows builds pick it up while
    // Linux/macOS builds skip it. Requires a resource compiler (rc.exe on the
    // MSVC toolchain, present on the CI Windows runner).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/caracal.ico");
        if let Err(error) = res.compile() {
            println!("cargo:warning=failed to embed Windows icon: {error}");
        }
    }

    println!("cargo:rerun-if-changed=assets/caracal.ico");
    println!("cargo:rerun-if-changed=build.rs");
}
