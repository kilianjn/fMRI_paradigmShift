//! Embeds the example paradigms (and their pictures) into the executable, so "File → Open example"
//! works from a single standalone binary.

use std::fs;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ex = root.join("examples");
    println!("cargo:rerun-if-changed=examples");
    println!("cargo:rerun-if-changed=build.rs");
    let mut files = vec![];
    walk(&ex, &mut files);
    let mut code = String::from("pub static EXAMPLE_FILES: &[(&str, &[u8])] = &[\n");
    for f in files {
        let rel = f.strip_prefix(&ex).unwrap().to_string_lossy().replace('\\', "/");
        let abs = f.to_string_lossy().replace('\\', "/");
        code.push_str(&format!("    ({rel:?}, include_bytes!({abs:?})),\n"));
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("examples.rs");
    fs::write(out, code).unwrap();

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
