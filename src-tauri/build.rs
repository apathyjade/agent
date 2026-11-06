fn main() {
    // Windows test binaries link Tauri's common-controls imports
    // (`TaskDialogIndirect`), which only exist in comctl32 v6. Without an
    // embedded manifest the loader picks v5 and the test executable aborts
    // with STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139).
    #[cfg(windows)]
    {
        let manifest = std::path::Path::new(
            &std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()),
        )
        .join("tests.manifest");
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
            manifest.display()
        );
        println!("cargo:rerun-if-changed={}", manifest.display());
    }

    tauri_build::build()
}
