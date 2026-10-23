# Publish the crates

Each crate declares its own version in `Cargo.toml` and has its own
`CHANGELOG.md`. Release the crates that changed, in dependency order:

| Package | Manifest | Release tag |
| --- | --- | --- |
| `osc7501-parser` | [parser](../crates/osc7501-parser/Cargo.toml) | `osc7501-parser-v<VERSION>` |
| `osc7501-store` | [store](../crates/osc7501-store/Cargo.toml) | `osc7501-store-v<VERSION>` |
| `osc7501-rs` | [producer](../crates/osc7501/Cargo.toml) | `osc7501-v<VERSION>` |

The store depends on the parser. The producer depends on both, even though its
store dependency is optional: Cargo still resolves it during packaging.
The producer's package name is `osc7501-rs`; its library and tag prefix use
`osc7501`.

Update each released crate's version, dated changelog entry, and comparison
links. Update dependency version requirements in affected crates and keep
`Cargo.lock` in sync. The crates do not need to share a version.

## Check the release commit

Review and commit the release changes, then wait for CI to pass on that exact commit.
Follow the [development guide](../DEVELOPMENT.md#check-a-change) locally.
[CI](../.github/workflows/ci.yml) defines the full OS, Rust, and feature matrix.
The embedded-target build checks `no_std` compilation; native tests check runtime
behavior.

Create each release tag from the final reviewed commit, or verify that an
existing tag points to it. If release files change afterward, validate the new
commit before choosing its release tag. A local tag is not yet on GitHub.

Check repository, example, and changelog links without signing in. Public
packages need documentation their users can access. The docs.rs pages become
available after publication and successful documentation builds.

## Inspect the archives

Inspect what will go into each archive:

```sh
cargo package -p osc7501-parser --list --locked
cargo package -p osc7501-store --list --locked
cargo package -p osc7501-rs --list --locked
```

Each archive keeps its source, README, MIT license, changelog, and documented
examples. The manifests exclude the top-level integration tests and the parser
benchmark. Tests remain available in the repository. Keep modules imported by
shipped examples, including their test modules, so the examples remain usable
outside the checkout.

For the first release, assemble the unpublished crates together:

```sh
cargo package --workspace --all-features --no-verify --locked
```

This creates archives in `target/package` without building them. Extract them
outside the checkout and build all targets with default and all features on
stable and the minimum supported Rust version. Check their doctests and rustdoc
too. Before dependencies are published, use temporary dependency overrides that
point only to the extracted archives. This verifies archive contents and builds;
it does not replace the registry-based dry runs below.

## Publish in dependency order

Run a dry run, then publish the parser:

```sh
cargo publish -p osc7501-parser --dry-run --locked
cargo publish -p osc7501-parser --locked
```

Wait for that version to appear in the crates.io index. Then check and publish
the store with its optional owned storage enabled:

```sh
cargo publish -p osc7501-store --all-features --dry-run --locked
cargo publish -p osc7501-store --all-features --locked
```

Wait for the store version to reach the index. Then publish the producer with
its optional store feature checked:

```sh
cargo publish -p osc7501-rs --all-features --dry-run --locked
cargo publish -p osc7501-rs --all-features --locked
```

Run publication checks without local dependency overrides. A dependent crate's
dry run cannot complete until its required versions exist in the index. For
later releases, publish only the new versions; existing dependency versions
need no new release.

## Publish tags and release notes

After publication, push the release tags for the packages released and create
their GitHub releases. Confirm that each tag points to the validated release
commit and matches its changelog links. Include the package version, protocol
revision, and minimum Rust version in the release notes. Check the crates.io,
docs.rs, and changelog release links once the documentation builds finish.
