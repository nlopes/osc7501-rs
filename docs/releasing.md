# Publish the crates

All three packages share the workspace version. Publish in this order:

1. `osc7501-parser`
2. `osc7501-store`
3. `osc7501-rs`

The store depends on the parser. The producer depends on both, even though its
store dependency is optional: Cargo still resolves it during packaging.

## Check the release commit

Review and commit the release changes, then wait for CI to pass on that exact commit.
Follow the [development workflow](../README.md#working-on-the-code) locally.
[CI](../.github/workflows/ci.yml) defines the full OS, Rust, and feature matrix.
The embedded-target build checks `no_std` compilation; native tests check runtime
behavior.

Inspect what will go into each archive:

```sh
cargo package -p osc7501-parser --list --locked
cargo package -p osc7501-store --list --locked
cargo package -p osc7501-rs --list --locked
```

Each archive needs its README and MIT license. The producer also needs
`examples/host.rs` and the `host/program.rs`, `host/scenario.rs`, and
`host/ui.rs` modules. Check public documentation links before publishing.

## Publish in dependency order

Run a dry run, then publish the parser:

```sh
cargo publish -p osc7501-parser --dry-run --locked
cargo publish -p osc7501-parser --locked
```

Wait for that version to appear in the crates.io index. Then publish the store:

```sh
cargo publish -p osc7501-store --dry-run --locked
cargo publish -p osc7501-store --locked
```

Wait for the store version to reach the index. Then publish the producer with
its optional store feature checked:

```sh
cargo publish -p osc7501-rs --all-features --dry-run --locked
cargo publish -p osc7501-rs --all-features --locked
```

Run publication checks without local dependency overrides. Before a first
release, overrides can check archive compilation, but normal dry runs need
the dependency versions in the index.

After all three publish, tag the release commit. Include the package versions,
protocol revision, and minimum Rust version in the release notes.
