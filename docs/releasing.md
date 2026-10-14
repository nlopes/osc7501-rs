# Publish the crates

Both packages publish to crates.io and share the workspace version. Publish
`osc7501-parser` first, then `osc7501-rs`, which depends on that parser version.

## Verify the release commit

Review and commit the changes, then wait for CI to pass on that exact commit.
The matrix covers Rust 1.88 and stable on Linux, macOS, and Windows, plus parser
compilation on a target without `std`. Cross-compilation does not verify native
runtime behavior.

The local validation commands are listed in the repository README. Inspect both
package lists as well:

```sh
cargo package -p osc7501-parser --list --locked
cargo package -p osc7501-rs --list --locked
```

Each archive must contain its README and the MIT license text. Crate overviews
and runnable examples are included in the archives and rendered on docs.rs.
Check any repository links intended for external users before release.

## Publish in dependency order

First verify and publish the parser:

```sh
cargo publish -p osc7501-parser --dry-run --locked
cargo publish -p osc7501-parser --locked
```

Wait until the parser version is available in the crates.io index. Then verify
and publish the producer:

```sh
cargo publish -p osc7501-rs --dry-run --locked
cargo publish -p osc7501-rs --locked
```

Run these checks without local dependency overrides. Before the first parser
release, a temporary parser override can verify producer archive compilation,
but its normal crates.io dry run requires the published parser.

Tag the published commit and write release notes that identify the package
versions, supported protocol revision, and minimum Rust version.
