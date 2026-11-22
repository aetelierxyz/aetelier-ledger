use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::config::{DEFAULT_URL, NetworkName};

#[derive(Debug, Parser)]
#[command(
    name = "aetelier-ledger-cli",
    version,
    about = "Inspect Aetelier token balances and buy token bundles with USDC over x402. Every result is one JSON object on stdout; the API key is read from AETELIER_API_KEY."
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        env = "AETELIER_URL",
        default_value = DEFAULT_URL,
        help = "Aetelier base URL"
    )]
    pub url: String,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "List bundles, plans and prices")]
    Catalog,
    #[command(about = "Show whether USDC top-ups are operational, degraded or disabled")]
    Status,
    #[command(about = "Show your token balances and entitlements")]
    Balance,
    #[command(about = "Show your ledger history, newest first")]
    History {
        #[arg(long, help = "Entries per page")]
        limit: Option<i64>,
        #[arg(long, help = "Entries to skip")]
        offset: Option<i64>,
    },
    #[command(about = "Buy a token bundle with USDC")]
    Buy {
        #[arg(help = "Bundle id from catalog, for example data_100")]
        sku: String,
        #[arg(
            long,
            env = "AETELIER_NETWORK",
            value_enum,
            help = "Network you expect to pay on"
        )]
        network: Option<NetworkName>,
        #[arg(
            long,
            env = "AETELIER_PAYER_KEYPAIR",
            help = "Payer keypair file (Solana CLI JSON)"
        )]
        keypair: Option<PathBuf>,
        #[arg(
            long,
            env = "AETELIER_RPC_URL",
            help = "Solana RPC for the USDC balance check"
        )]
        rpc_url: Option<String>,
        #[arg(long, help = "Buy without the confirmation prompt")]
        yes: bool,
        #[arg(long, default_value_t = 180, help = "Seconds to wait for settlement")]
        wait: u64,
    },
    #[command(about = "Show the receipt of a top-up")]
    Receipt {
        #[arg(help = "Memo printed by buy")]
        memo: String,
        #[arg(long, default_value_t = 0, help = "Seconds to wait for a final state")]
        wait: u64,
    },
    #[command(about = "Create a new payer keypair file readable only by you")]
    Keygen {
        #[arg(help = "Where to write the keypair; an existing file is never overwritten")]
        path: PathBuf,
    },
    #[command(about = "Show the payer address and, with --network, its USDC balance")]
    Address {
        #[arg(
            long,
            env = "AETELIER_PAYER_KEYPAIR",
            help = "Payer keypair file (Solana CLI JSON)"
        )]
        keypair: Option<PathBuf>,
        #[arg(
            long,
            env = "AETELIER_NETWORK",
            value_enum,
            help = "Network for the USDC balance"
        )]
        network: Option<NetworkName>,
        #[arg(
            long,
            env = "AETELIER_RPC_URL",
            help = "Solana RPC for the USDC balance"
        )]
        rpc_url: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, FromArgMatches};

    #[test]
    fn the_command_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn buy_parses_flags_and_defaults() {
        let matches = Cli::command()
            .mut_arg("url", |arg| arg.env(None::<&'static str>))
            .try_get_matches_from([
                "aetelier-ledger-cli",
                "buy",
                "data_100",
                "--network",
                "devnet",
                "--keypair",
                "/k.json",
                "--yes",
            ])
            .unwrap();
        let cli = Cli::from_arg_matches(&matches).unwrap();
        assert_eq!(cli.url, DEFAULT_URL);
        match cli.command {
            Command::Buy {
                sku,
                network,
                keypair,
                yes,
                wait,
                ..
            } => {
                assert_eq!(sku, "data_100");
                assert_eq!(network, Some(NetworkName::Devnet));
                assert_eq!(keypair, Some(PathBuf::from("/k.json")));
                assert!(yes);
                assert_eq!(wait, 180);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn url_is_global_and_networks_are_closed() {
        let cli = Cli::try_parse_from([
            "aetelier-ledger-cli",
            "status",
            "--url",
            "http://127.0.0.1:9",
        ])
        .unwrap();
        assert_eq!(cli.url, "http://127.0.0.1:9");
        assert!(
            Cli::try_parse_from([
                "aetelier-ledger-cli",
                "buy",
                "data_100",
                "--network",
                "testnet"
            ])
            .is_err()
        );
        assert!(Cli::try_parse_from(["aetelier-ledger-cli", "balance", "--api-key", "k"]).is_err());
    }
}
