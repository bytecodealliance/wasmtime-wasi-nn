//! Guest-side test programs for `wasmtime-wasi-nn`.
//!
//! The binaries in `src/bin` are compiled to WebAssembly by the
//! `test-programs-artifacts` build script and executed by the host test
//! harness in `tests/test-programs.rs`.

pub mod nn;
