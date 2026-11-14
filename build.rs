//! Build script: gives the Windows .exe its icon and version information.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Windows: give the .exe its icon and version information
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/icon/paradigmshift.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/paradigmshift.ico");
        res.set("ProductName", "ParadigmShift");
        res.set("FileDescription", "ParadigmShift – fMRI paradigm editor and presenter");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
