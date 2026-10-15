# Publish the crates

All packages publish to crates.io and share the workspace version. Publish
`osc7501-parser` first, then `osc7501-store`, then `osc7501-rs`. The store depends
on the parser. The producer depends on the parser and optionally the store.
Cargo resolves that optional dependency during packaging even when the `store`
feature is disabled.

## Verify the release commit

Review and commit the changes, then wait for CI to pass on that exact commit.
The matrix covers Rust 1.88 and stable on Linux, macOS, and Windows, plus parser
and store compilation on a target without `std`. Cross-compilation does not
verify native runtime behavior.

The local validation commands are listed in the repository README. Inspect all
package lists as well:

```sh
cargo package -p osc7501-parser --list --locked
cargo package -p osc7501-store --list --locked
cargo package -p osc7501-rs --list --locked
```

Each archive must contain its README and the MIT license text. The producer
archive must also contain `examples/host.rs` and its `host/program.rs`,
`host/scenario.rs`, and `host/ui.rs` modules. The host example requires the
`store` feature. Check repository links intended for external users before
release.

## Publish in dependency order

First verify and publish the parser:

```sh
cargo publish -p osc7501-parser --dry-run --locked
cargo publish -p osc7501-parser --locked
```

Wait until the parser version is available in the crates.io index. Then verify
and publish the store:

```sh
cargo publish -p osc7501-store --dry-run --locked
cargo publish -p osc7501-store --locked
```

Wait until the store version is available in the index. Then verify and
publish the producer, including its optional store feature:

```sh
cargo publish -p osc7501-rs --all-features --dry-run --locked
cargo publish -p osc7501-rs --all-features --locked
```

Run these checks without local dependency overrides. Before the first release,
a temporary parser override can verify store archive compilation. Temporary
parser and store overrides can verify the producer archive. Normal crates.io
dry runs require each dependency version in the index.

Tag the published commit and write release notes that identify the package
versions, supported protocol revision, and minimum Rust version.
