# Homebrew release operations

A stable tag publishes two native macOS archives to an immutable GitHub
release. It then sends the verified release identity to
`quality-gates/homebrew-tap`. The GitHub release is the release commit point. Do
not delete it when tap publication fails.

## One-time repository setup

1. Enable immutable releases for `quality-gates/messrust`. Protect stable
   `vMAJOR.MINOR.PATCH` tags so that only release maintainers can create them.
2. Create an organization-owned GitHub App with only **Actions: write**. Install
   it only on `quality-gates/homebrew-tap`. Do not add it to a ruleset bypass
   list.
3. Create a protected `homebrew` environment in `messrust`. Add
   `HOMEBREW_TAP_APP_ID` and `HOMEBREW_TAP_APP_PRIVATE_KEY` as environment
   secrets. Permit only stable release tags to use this environment.
4. Put the generic `publish-formula.yml` workflow on the `main` branch of
   `homebrew-tap`. Protect the tap default branch and require its formula tests.

The release repository does not store a personal access token. The protected
job makes a short-life GitHub App token that can start workflows only in the
tap repository.

## Normal release

1. Set the `Cargo.toml` package version to `MAJOR.MINOR.PATCH` and merge that
   change.
2. Create and push the matching `vMAJOR.MINOR.PATCH` tag.
3. Monitor the `Release` workflow and the linked tap workflow.

The workflow verifies the remote tag and Cargo package version. It runs
`cargo test --all-targets --locked` and self-analysis. Native Intel and Apple
Silicon runners build the release executable. The workflow packages and tests
the exact bytes on the matching runners.

The immutable GitHub release contains only these assets:

- `messrust_VERSION_darwin_arm64.tar.gz`
- `messrust_VERSION_darwin_amd64.tar.gz`
- `checksums.txt`

Each archive contains `messrust` and `LICENSE` at its top level. The tap request
contains the tool, tag, version, release ID, source commit, asset names, and
SHA-256 values. The tap must verify this untrusted data against the immutable
release before it makes a formula.

## Recovery

Start the `Release` workflow manually from the same tag reference and give it
the same tag as input. Selecting the tag reference lets the protected
`homebrew` environment apply its stable-tag policy. The workflow has
state-aware retry behavior:

- It keeps a matching draft asset and uploads only missing assets.
- It stops if a draft asset has different bytes or if the draft has an extra
  asset.
- It verifies an existing immutable release and repeats only tap publication.
- It stops if an existing published release is not immutable.
- It does not replace or delete a release asset.

A repeat tap request must converge on the same formula change. If tap
publication fails, keep the tag and GitHub release. Correct the tap workflow,
branch policy, environment approval, credentials, or formula check. Then retry
the same release tag.
