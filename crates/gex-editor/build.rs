//! On Windows, gives gex.exe the GEX icon (assets/gex.ico): what Explorer and the taskbar
//! show for the program. Needs a resource compiler: windres (MinGW) or rc.exe (Visual
//! Studio); without one, gex.exe still builds, with the default icon.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/gex.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let ico = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("assets").join("gex.ico");
    let rc = out.join("gex.rc");
    // Resource scripts want forward slashes.
    if std::fs::write(&rc, format!("1 ICON \"{}\"\n", ico.to_string_lossy().replace('\\', "/"))).is_err() {
        return;
    }
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("msvc") {
        let res = out.join("gex.res");
        if Command::new("rc.exe").arg("/nologo").arg("/fo").arg(&res).arg(&rc).status().is_ok_and(|s| s.success()) {
            println!("cargo:rustc-link-arg-bins={}", res.display());
        } else {
            println!("cargo:warning=rc.exe wasn't found, so gex.exe gets the default icon");
        }
        return;
    }
    let obj = out.join("gex-icon.o");
    let ok = ["x86_64-w64-mingw32-windres", "windres"].iter().any(|tool| Command::new(tool).args(["-O", "coff", "-i"]).arg(&rc).arg("-o").arg(&obj).status().is_ok_and(|s| s.success()));
    if ok {
        println!("cargo:rustc-link-arg-bins={}", obj.display());
    } else {
        println!("cargo:warning=windres wasn't found, so gex.exe gets the default icon");
    }
}
