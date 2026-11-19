//! Build the `test-programs` binaries to WebAssembly and expose their paths.
//!
//! This replaces Wasmtime's in-tree `test-programs-artifacts` build script,
//! which built every test program in the repository. Here we only need the
//! `nn_*` programs, so the logic is considerably smaller:
//!
//! 1. `cargo build --target wasm32-wasip1` the `test-programs` package.
//! 2. For each `nn_witx_*` binary, emit a constant pointing at the core module.
//! 3. For each `nn_wit_*` binary, additionally adapt the core module into a
//!    component and emit a `*_COMPONENT` constant pointing at it.
//! 4. Emit a `foreach_nn!` macro listing every program so the harness can
//!    enumerate them.

use heck::{ToShoutySnakeCase, ToSnakeCase};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use wasi_preview1_component_adapter_provider::{
    WASI_SNAPSHOT_PREVIEW1_ADAPTER_NAME, WASI_SNAPSHOT_PREVIEW1_COMMAND_ADAPTER,
};
use wit_component::ComponentEncoder;

const TARGET: &str = "wasm32-wasip1";

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR always set by cargo"));
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR always set"));
    // `test-programs/artifacts` -> `test-programs`.
    let test_programs_dir = manifest_dir.parent().unwrap().to_path_buf();

    println!("cargo:rerun-if-changed={}", test_programs_dir.display());

    let core_modules = build_test_programs(&out_dir, &test_programs_dir);
    let generated = generate_bindings(&out_dir, &core_modules);

    fs::write(out_dir.join("gen.rs"), generated).expect("failed to write generated bindings");
}

/// Invoke a nested `cargo build` for the wasm guest programs.
///
/// Returns `(program name, path to core module)` pairs.
fn build_test_programs(out_dir: &Path, test_programs_dir: &Path) -> Vec<(String, PathBuf)> {
    // Build into a directory separate from the outer build to avoid deadlocking
    // on the host target directory's lock.
    //
    // This deliberately does NOT live under `OUT_DIR`. `OUT_DIR` is keyed on the
    // enabled feature set, so putting the nested build there makes every feature
    // combination compile the whole guest toolchain again, at roughly half a
    // gigabyte apiece. Hanging it off the shared target directory instead means
    // all feature combinations reuse one nested build.
    let target_dir = shared_target_dir(out_dir).join("wasm-test-programs");

    let mut cmd = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.arg("build")
        .arg("--target")
        .arg(TARGET)
        .arg("--bins")
        .arg("--manifest-path")
        .arg(test_programs_dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir);

    let profile = if cfg!(feature = "release-artifacts") {
        cmd.arg("--release");
        "release"
    } else {
        "debug"
    };

    // Cargo sets these for the *outer* build; leaking them into the nested
    // build makes it pick up the host target and flags.
    for var in [
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTFLAGS",
        "CARGO_BUILD_TARGET",
        "CARGO_BUILD_RUSTFLAGS",
    ] {
        cmd.env_remove(var);
    }

    let status = cmd
        .status()
        .expect("failed to spawn cargo for test programs");
    assert!(
        status.success(),
        "building the wasm test programs failed; is the `{TARGET}` target installed? \
         try `rustup target add {TARGET}`"
    );

    let bin_dir = target_dir.join(TARGET).join(profile);
    let mut programs = Vec::new();
    for entry in fs::read_dir(&bin_dir).expect("wasm output directory missing") {
        let path = entry.expect("failed to read wasm output entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("wasm file with non-UTF-8 name")
            .to_string();
        if !name.starts_with("nn_") {
            continue;
        }
        programs.push((name, path));
    }
    programs.sort();
    assert!(
        !programs.is_empty(),
        "no `nn_*` wasm programs were produced in {}",
        bin_dir.display()
    );
    programs
}

/// Locate the shared target directory from `OUT_DIR`.
///
/// `OUT_DIR` is `<target>/<profile>/build/<pkg>-<hash>/out`, so the target
/// directory is four levels up. (When cross-compiling, `<profile>` is preceded
/// by a target triple and this lands in `<target>/<triple>`, which is still
/// inside the target directory.) Falling back to `OUT_DIR` itself keeps the
/// build working if Cargo ever changes that layout; it just costs disk.
fn shared_target_dir(out_dir: &Path) -> PathBuf {
    let mut dir = out_dir;
    for _ in 0..4 {
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return out_dir.to_path_buf(),
        }
    }
    dir.to_path_buf()
}

/// Emit path constants, `*_bytes!` macros, and the `foreach_nn!` macro.
fn generate_bindings(out_dir: &Path, core_modules: &[(String, PathBuf)]) -> String {
    let mut generated = String::new();
    let mut names = Vec::new();

    for (name, core_wasm) in core_modules {
        let shouty = name.to_shouty_snake_case();
        let snake = name.to_snake_case();

        generated += &format!("pub const {shouty}: &'static str = {core_wasm:?};\n");
        generated += &format!(
            "#[macro_export] macro_rules! {snake}_bytes {{\n    () => {{ include_bytes!({core_wasm:?}) }}\n}}\n"
        );

        // The WIT-based tests are run as components, so adapt them here.
        if name.starts_with("nn_wit_") {
            let component = componentize(out_dir, name, core_wasm);
            generated += &format!("pub const {shouty}_COMPONENT: &'static str = {component:?};\n");
        }

        names.push(name.clone());
    }

    generated += "#[macro_export]\nmacro_rules! foreach_nn {\n    ($mac:ident) => {\n";
    for name in &names {
        generated += &format!("        $mac!({name});\n");
    }
    generated += "    };\n}\n";

    generated
}

/// Adapt a preview1 core module into a component using the released adapter.
fn componentize(out_dir: &Path, name: &str, core_wasm: &Path) -> PathBuf {
    let module = fs::read(core_wasm).expect("failed to read core wasm module");
    let component = ComponentEncoder::default()
        .module(&module)
        .expect("failed to register core module with the component encoder")
        .validate(true)
        .adapter(
            WASI_SNAPSHOT_PREVIEW1_ADAPTER_NAME,
            WASI_SNAPSHOT_PREVIEW1_COMMAND_ADAPTER,
        )
        .expect("failed to register the preview1 adapter")
        .encode()
        .unwrap_or_else(|e| panic!("failed to componentize {name}: {e:?}"));

    let path = out_dir.join(format!("{name}.component.wasm"));
    fs::write(&path, component).expect("failed to write component");
    path
}
