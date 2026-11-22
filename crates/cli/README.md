# aetelier-ledger-cli

Inspect Aetelier token balances and buy token bundles with USDC over x402 on Solana.

Every command prints exactly one JSON object on stdout, including errors; `--help` and `--version` print plain text. Prompts and warnings go to stderr.

## Install

Release binaries for macOS arm64 and Linux x86_64 and arm64 are attached to each [release](https://github.com/aetelierxyz/aetelier-ledger/releases); the [repository README](../../README.md#install) shows how to download and verify them. From source, with Rust 1.89 or later:

```bash
cargo install --locked --git https://github.com/aetelierxyz/aetelier-ledger --tag v0.0.1 aetelier-ledger-cli
```

## Commands

| Command | What it does | Needs |
|---|---|---|
| `catalog` | Bundles, plans and prices | nothing |
| `status` | Whether USDC top-ups are `operational`, `degraded` or `disabled` | nothing |
| `balance` | Token balances and entitlements | API key |
| `history [--limit N] [--offset N]` | Ledger entries, newest first | API key |
| `buy <sku> [--yes] [--wait S]` | Pays for a bundle and waits for settlement (default 180 s) | API key, keypair, network |
| `receipt <memo> [--wait S]` | Receipt of a top-up, optionally waiting for a final state | API key |
| `keygen <path>` | Writes a new payer keypair readable only by you; never overwrites | path |
| `address [--network N]` | Payer address and, with a network, its USDC balance | keypair |

## Settings

A flag wins over its variable.

| Setting | Flag | Variable | Default |
|---|---|---|---|
| Base URL | `--url` | `AETELIER_URL` | `https://aetelier.xyz` |
| API key | none | `AETELIER_API_KEY` | none |
| Payer keypair file | `--keypair` | `AETELIER_PAYER_KEYPAIR` | none |
| Network | `--network devnet\|mainnet` | `AETELIER_NETWORK` | none |
| Solana RPC | `--rpc-url` | `AETELIER_RPC_URL` | public endpoint of the network |

- `--url` and `--rpc-url` must use https, or http on a loopback host.
- The API key is created in the web app under Settings, API keys.
- The keypair file is a Solana CLI JSON byte array. Files readable by group or others are refused.
- The payer wallet needs USDC only; transaction fees are paid by the facilitator.
- This build does not pay on mainnet.

## What `buy` checks before signing

- The bundle and its price come from `catalog`; the prompt shows bundle, price, network, payer and, for a non-default URL, the host. `--yes` skips the prompt; without a terminal and without `--yes` the purchase is refused.
- With a reachable RPC, a missing USDC account or a short balance stops the purchase before anything is created; an unreachable RPC only prints a warning.
- The server's payment requirements must name the chosen network, the USDC mint of that network, the confirmed amount, the prepared memo and a fee payer other than you, and the signing window must still be open.
- The transaction is rebuilt locally from those requirements and signed only if it is byte-for-byte identical to the one the server prepared.
- A submitted payment is never retried automatically.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Settled, or the read succeeded |
| 1 | Usage or configuration error |
| 2 | Refused for safety, or declined |
| 3 | Not authorised (401, 403) |
| 4 | Payment rejected or expired, or not enough USDC |
| 5 | Pending: still settling, or the outcome after submitting is unknown (no answer, 5xx); run `receipt <memo>` |
| 6 | Temporary failure before any payment was sent (429, 5xx, network); safe to retry |

Errors print `{"error":{"code","message","http_status","memo"}}`; `http_status` and `memo` appear when known. `receipt` prints the server's reply and exits 0, 4 or 5 by state.

## Example

```bash
mkdir -p ~/.config/aetelier
aetelier-ledger-cli keygen ~/.config/aetelier/payer.json
export AETELIER_API_KEY=<API key> AETELIER_PAYER_KEYPAIR=~/.config/aetelier/payer.json AETELIER_NETWORK=devnet
aetelier-ledger-cli address
aetelier-ledger-cli buy data_100
aetelier-ledger-cli balance
```
