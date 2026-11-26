# wasmtime-wasi-nn

This crate enables support for the [wasi-nn] API in Wasmtime. Currently it
contains implementations using OpenVINO, ONNX Runtime, PyTorch, and WinML.
Support for the preview1/WITX ABI and the component-model/WIT ABI is included.

[examples]: examples
[openvino]: https://crates.io/crates/openvino
[wasi-nn]: https://github.com/WebAssembly/wasi-nn
[bindings]: https://crates.io/crates/wasi-nn

## Testing before publication

The crate is not yet published on crates.io. To test the code in this repository
against the initial pull request, use a Git dependency pinned to its commit:

```toml
[dependencies]
wasmtime = "=51.0.0-rc.1"
wasmtime-wasi-nn = {
    git = "https://github.com/bytecodealliance/wasmtime-wasi-nn",
    rev = "8eb048373346d79d5808ebcb854eec9a0a462df7"
}
```

A branch dependency is also possible, but a commit is preferred for reproducible
builds:

```toml
wasmtime-wasi-nn = {
    git = "https://github.com/bytecodealliance/wasmtime-wasi-nn",
    branch = "standalone-build"
}
```

For local testing, use a path dependency:

```toml
wasmtime-wasi-nn = { path = "../wasmtime-wasi-nn" }
```

After the crate is published, the dependency can use a crates.io version:

```toml
[dependencies]
wasmtime = "=51.0.0-rc.1"
wasmtime-wasi-nn = "51.0.0"
```

Use a compatible Wasmtime version. This repository is currently pinned to the
Wasmtime release extracted for it.

## Using with Wasmtime

`wasmtime-wasi-nn` is a standalone Rust crate, but its host integration uses
Wasmtime directly. It depends on Wasmtime for:

- registering host functions with `wasmtime::Linker`;
- Wiggle-generated preview1/WITX host bindings;
- component-model resource types and `wasmtime::component::ResourceTable`.

It does not replace or modify the Wasmtime runtime.

Add `WasiNnCtx` to the embedder's store state, construct it with the backends
and registry that should be available, and register it with the linker.

### Preview1/WITX ABI

```rust
use wasmtime_wasi_nn::{Backend, InMemoryRegistry};
use wasmtime_wasi_nn::witx::WasiNnCtx;

struct Ctx {
    wasi_nn: WasiNnCtx,
    // Other host state, such as WasiP1Ctx.
}

let backends = wasmtime_wasi_nn::backend::list();
let registry = InMemoryRegistry::new();
let wasi_nn = WasiNnCtx::new(backends, registry.into());

wasmtime_wasi_nn::witx::add_to_linker(
    &mut linker,
    |ctx: &mut Ctx| &mut ctx.wasi_nn,
)?;
```

For directory-backed preloading, use `preload`:

```rust
let (backends, registry) =
    wasmtime_wasi_nn::preload(&[("openvino".into(), "/models".into())])?;
let wasi_nn = WasiNnCtx::new(backends, registry);
```

### Component-model/WIT ABI

For component modules, the host state must also expose a component resource
table:

```rust
use wasmtime::component::ResourceTable;
use wasmtime_wasi_nn::wit::{WasiNnCtx, WasiNnView};

struct Ctx {
    wasi_nn: WasiNnCtx,
    table: ResourceTable,
}

wasmtime_wasi_nn::wit::add_to_linker(
    &mut linker,
    |ctx: &mut Ctx| {
        WasiNnView::new(&mut ctx.table, &mut ctx.wasi_nn)
    },
)?;
```

The component view is recreated for each host call and connects wasi-nn
resources to the Wasmtime store's `ResourceTable`.

## Building

```console
cargo build
cargo build --release
```

The default build enables OpenVINO and WinML support. OpenVINO must be installed
on the host; WinML is available on supported Windows systems.

Optional backends include ONNX Runtime and PyTorch:

```console
cargo build --no-default-features --features onnx
cargo build --no-default-features --features onnx-download
cargo build --no-default-features --features onnx-pkg-config
cargo build --no-default-features --features pytorch
```

## Integrating into Wasmtime

To replace an in-tree wasi-nn implementation:

1. Remove or disable the existing `crates/wasi-nn` workspace member.
2. Add `wasmtime-wasi-nn` as a dependency of the Wasmtime embedding code.
3. Add `WasiNnCtx` to the embedder's `Store` state.
4. Register it with `Linker`.
5. Construct it with the backends and graphs that should be available.

The linker registration exposes the wasi-nn imports to guest modules. It does not
copy the implementation into Wasmtime or require source-tree patches.

This crate cannot be used directly in WAMR or other runtimes because its host
layer is implemented against Wasmtime's `Linker`, Wiggle, and component-model
types. Those runtimes may reuse backend and model-loading code, but require their
own host ABI integration.

## Example

An end-to-end example demonstrating ML classification is included in [examples].
`examples/classification-example` contains a standalone Rust project that uses
the [wasi-nn] API and is compiled for `wasm32-wasip1` with the high-level
wasi-nn [bindings].
