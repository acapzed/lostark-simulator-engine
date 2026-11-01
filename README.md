<p align="center">
  <img src="./lostarksim_title.svg" alt="Lost Ark Simulator" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-WASM-CE412B?style=flat&logo=rust&logoColor=white" alt="Rust/WASM" />
</p>

# Lost Ark Simulator Engine

The event-driven Rust and WebAssembly combat engine for [Lost Ark Simulator](https://lostarksim.com/).

## Overview

- **Input:** Prepared build data and simulation settings in JSON
- **Output:** Aggregate results or a trace of a single simulation run
- **Runtime:** WebAssembly in the application; native Rust for library tests

This public repository is a standalone snapshot of the application engine at source commit `66f86bf051bd008093e26384360a1f6d634482ea`, exported on November 1, 2026. The project is preserving its current implementation; new feature development and game-data research are on hold. A class module's presence does not imply complete game-data coverage or validated support for every build.

`fixtures/` contains prepared requests, presets, and one source-audit fixture. `skill-data/` contains the selected input files referenced by Rust compile-time includes. This is not a complete game-data export. Building or testing this repository requires neither the private application nor the private research repository. Existing GPL-3.0-only licensing is retained.

## Build and test

With Rust, Cargo, and Python installed, run from the repository root:

```sh
cargo check --locked
cargo test --locked --lib
python tools/sync-from-app.py --check
```

This snapshot has passed 496 native library tests and a WASM-target check. CI runs the snapshot fingerprint check, native checks and tests, and the WASM-target check on pushes and pull requests. Native tests and target checks do not replace browser shared-memory runtime verification.

To check the WASM target locally:

```sh
rustup target add wasm32-unknown-unknown
cargo check --locked --target wasm32-unknown-unknown
```

WASM target checking does not generate a threaded browser package. Production packaging still requires the matching wasm-bindgen CLI, nightly shared-memory build flags, and wasm-opt used by the application build.

## Repositories

| Repository | Scope | Access |
| --- | --- | --- |
| [Application](https://github.com/acapzed/lostark-simulator) | Product UI, Workers/D1, Rust/WASM engine, and production inputs | Private |
| [Engine](https://github.com/acapzed/lostark-simulator-engine) | Standalone Rust snapshot and required test inputs | Public; GPL-3.0-only |
| [Research](https://github.com/acapzed/lostark-simulator-research) | Preserved source material, research tools, and historical records | Private |

Updates between repositories are explicit. There is no automatic synchronization, submodule, or build-time dependency on the private repositories. Private links require access to the corresponding repository.

## Updating the snapshot

Maintainers with the application checkout can explicitly export a committed revision:

```sh
python tools/sync-from-app.py --app ../lostark-simulator --revision <commit>
```

The exporter copies Rust sources, fixtures, and literal include dependencies from Git. It adapts only crate-relative include paths and the GPL package metadata, refuses local edits in managed paths, and records source/output SHA-256 hashes in [engine-source-manifest.json](engine-source-manifest.json). No application secrets, server configuration, research archive, or automatic remote dependency is included. Exporting does not modify the application or publish changes automatically.

See the [project checkpoint](docs/PROJECT-STATE.md) for scope, validation, and remaining work. The checkpoint and source comments are primarily in Korean.

## License

Copyright (C) 2026 Minjea Kim.

This project is licensed under the GNU General Public License v3.0 only (`GPL-3.0-only`). See [LICENSE](LICENSE).

Lost Ark is a trademark of Smilegate RPG; this project is not affiliated with Smilegate RPG.
