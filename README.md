# aetelier-ledger

[![ci][badge-ci]][url-ci]
[![Rust][badge-rust]][url-rust]
[![MIT licensed][badge-license]][url-license]

[badge-ci]: https://github.com/aetelierxyz/aetelier-ledger/actions/workflows/ci.yml/badge.svg
[url-ci]: https://github.com/aetelierxyz/aetelier-ledger/actions/workflows/ci.yml
[badge-rust]: https://img.shields.io/badge/rust-1.89%2B-orange.svg
[url-rust]: rust-toolchain.toml
[badge-license]: https://img.shields.io/badge/license-MIT-blue
[url-license]: LICENSE

The transaction builder and the command line client behind Aetelier token top-ups: USDC on Solana, paid over x402, the open payment protocol.

| Crate | Path | What it does |
|---|---|---|
| `aetelier-ledger-x402` | `crates/x402` | x402 v2 wire types, the Solana `exact`-scheme USDC transfer builder, the checks on a signed submission, Solana network constants and a balance-only RPC client |
| `aetelier-ledger-cli` | `crates/cli` | The `aetelier-ledger-cli` binary: catalog, status, balance, history, buy, receipt, keygen and address. See [crates/cli/README.md](crates/cli/README.md) |

## One builder on both sides

- The Aetelier top-up endpoint builds every payment transaction with `aetelier-ledger-x402` (`TopupTx::build`).
- `aetelier-ledger-cli buy` rebuilds that transaction locally from the payment requirements with the same crate and signs only if it is byte for byte identical to the one the server prepared. Anything else is refused with `transaction_mismatch` before signing.
- A settled devnet payment is captured in `crates/x402/testdata` and rebuilt byte for byte by the tests.
- `bincode` and the Solana crates are pinned to exact versions in `Cargo.toml`, so the serialised bytes cannot change without a change to this repository.
- On mainnet the CLI pays only the Aetelier treasury built into the binary, `74voCLtQkUTUU51pKC3zf38wCmbnE6rNmt284pWGsFNf` (USDC account `GKJmKAryG2nZEDVNz99CkM4LZwP5JAeGARq1zo5NBQHg`); any other `payTo` is refused with `pay_to_mismatch` before signing.

## Install

Release binaries are built for `aarch64-apple-darwin`, `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`. Each tarball holds the binary, `LICENSE` and `README.md`.

```bash
tag="$(gh release view -R aetelierxyz/aetelier-ledger --json tagName -q .tagName)"
target=aarch64-apple-darwin
gh release download "$tag" -R aetelierxyz/aetelier-ledger -p "aetelier-ledger-cli-$tag-$target.tar.gz" -p SHA256SUMS
shasum -a 256 -c SHA256SUMS --ignore-missing
tar -xzf "aetelier-ledger-cli-$tag-$target.tar.gz"
mkdir -p ~/.local/bin
install -m 0755 "aetelier-ledger-cli-$tag-$target/aetelier-ledger-cli" ~/.local/bin/
```

From source, with Rust 1.89 or later:

```bash
cargo install --locked --git https://github.com/aetelierxyz/aetelier-ledger --tag "$tag" aetelier-ledger-cli
```

## Verify

Every tarball and the binary inside it carry a GitHub build provenance attestation, releases are immutable, and `SHA256SUMS` lists every tarball.

```bash
gh attestation verify "aetelier-ledger-cli-$tag-$target.tar.gz" -R aetelierxyz/aetelier-ledger --signer-workflow aetelierxyz/aetelier-ledger/.github/workflows/release.yml --source-ref "refs/tags/$tag"
gh attestation verify "aetelier-ledger-cli-$tag-$target/aetelier-ledger-cli" -R aetelierxyz/aetelier-ledger --signer-workflow aetelierxyz/aetelier-ledger/.github/workflows/release.yml --source-ref "refs/tags/$tag"
gh release verify "$tag" -R aetelierxyz/aetelier-ledger
shasum -a 256 -c SHA256SUMS --ignore-missing
```

Release tags are SSH-signed. Check one against the signers listed in `.github/allowed_signers`:

```bash
git clone https://github.com/aetelierxyz/aetelier-ledger
cd aetelier-ledger
git -c gpg.ssh.allowedSignersFile=.github/allowed_signers tag -v "$tag"
```

The release workflow publishes only from a signed annotated tag that is on `main` and equals `v` plus the crate version, and only after the tests pass.

## macOS

The macOS binary carries an ad hoc signature and is not notarized. A browser download is quarantined, and Gatekeeper blocks it on first run. Files fetched with `curl` or `gh`, and binaries built by `cargo install`, are not quarantined. After verifying a browser download:

```bash
xattr -d com.apple.quarantine aetelier-ledger-cli
```

## Build and test

```bash
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

The toolchain is pinned in `rust-toolchain.toml`; the minimum supported Rust version is 1.89.

## Links

- Aetelier: https://aetelier.xyz
- Security reports: [SECURITY.md](SECURITY.md)
- License: MIT, see [LICENSE](LICENSE)
