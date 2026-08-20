fn main() {
    slint_build::compile("ui/main.slint").unwrap();

    // Delivery 3 (hve2-trunk): compile the standalone shell chrome + tokens
    // so the headless tests (src/shell/ui_tests.rs) can instantiate them;
    // ui/main.slint references them only from delivery 4 on. Separate output
    // file so `include_modules!()` in main.rs keeps resolving to ui/main.slint.
    let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = std::env::var_os("OUT_DIR").unwrap();
    let shell_src = manifest.join("ui/shell.slint");
    let shell_out = std::path::Path::new(&out_dir).join("shell.slint.rs");
    let deps = slint_build::compile_with_output_path(
        &shell_src,
        &shell_out,
        slint_build::CompilerConfiguration::default(),
    )
    .unwrap_or_else(|e| panic!("failed to compile ui/shell.slint: {e:?}"));
    for dep in deps {
        println!("cargo:rerun-if-changed={}", dep.display());
    }
}
