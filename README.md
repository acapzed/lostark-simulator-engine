<p align="center">
  <img src="./lostarksim_title.svg" alt="Lost Ark Simulator" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-WASM-CE412B?style=flat&logo=rust&logoColor=white" alt="Rust/WASM" />
</p>

# Lost Ark Simulator Engine

A high-performance, event-driven combat simulation engine for [Lost Ark Simulator](https://lostarksim.com/).

## Overview

- **Input:** Prepared JSON build and simulation settings
- **Output:** Aggregate results or a single-run trace
- **Runtime:** Rust compiled to WebAssembly

The web application and research archive are maintained separately. This public repository is a standalone snapshot of the application engine at source commit `66f86bf051bd008093e26384360a1f6d634482ea` (2026-11-01). It includes the current engine modules; the existence of a class module does not imply complete game-data coverage or validated support for every build.

`fixtures/` contains prepared requests, presets, and one source-audit fixture. `skill-data/` contains the selected input files referenced by Rust compile-time includes. This is not a complete game-data export. Building or testing this repository requires neither the private application nor the private research repository. Existing GPL-3.0-only licensing is retained.

## Test

```sh
cargo check --locked
cargo test --locked --lib
python tools/sync-from-app.py --check
```

The snapshot is validated with 496 native library tests and a WASM-target type check. Native tests do not replace browser/shared-memory runtime verification. CI runs the snapshot fingerprint check, native check/tests, and `wasm32-unknown-unknown` check on pushes and pull requests.

```sh
rustup target add wasm32-unknown-unknown
cargo check --locked --target wasm32-unknown-unknown
```

WASM target checking does not generate a threaded browser package. Production packaging still requires the matching wasm-bindgen CLI, nightly shared-memory build flags, and wasm-opt used by the application build.

## Updating the snapshot

Maintainers with the application checkout can explicitly export a committed revision:

```sh
python tools/sync-from-app.py --app ../lostark-simulator --revision <commit>
```

The exporter copies Rust sources, fixtures, and literal include dependencies from Git. It adapts only crate-relative include paths and the GPL package metadata, refuses local edits in managed paths, and records source/output SHA-256 hashes in [engine-source-manifest.json](engine-source-manifest.json). No application secrets, server configuration, research archive, or automatic remote dependency is included. Exporting does not modify the application or publish changes automatically.

See [the checkpoint](docs/PROJECT-STATE.md) for scope, validation, and remaining work.

## License

Copyright (C) 2026 Minjea Kim.

This project is licensed under the GNU General Public License v3.0 only (`GPL-3.0-only`). See [LICENSE](LICENSE).

Lost Ark is a trademark of Smilegate RPG; this project is not affiliated with Smilegate RPG.
