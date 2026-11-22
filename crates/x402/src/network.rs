use crate::error::X402Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    SolanaMainnet,
    SolanaDevnet,
}

impl Network {
    pub const MAINNET_CAIP2: &'static str = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
    pub const DEVNET_CAIP2: &'static str = "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1";
    pub const MAINNET_USDC: &'static str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    pub const DEVNET_USDC: &'static str = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";

    pub fn caip2(self) -> &'static str {
        match self {
            Network::SolanaMainnet => Self::MAINNET_CAIP2,
            Network::SolanaDevnet => Self::DEVNET_CAIP2,
        }
    }

    pub fn usdc_mint(self) -> &'static str {
        match self {
            Network::SolanaMainnet => Self::MAINNET_USDC,
            Network::SolanaDevnet => Self::DEVNET_USDC,
        }
    }

    pub fn default_rpc_url(self) -> &'static str {
        match self {
            Network::SolanaMainnet => "https://api.mainnet-beta.solana.com",
            Network::SolanaDevnet => "https://api.devnet.solana.com",
        }
    }

    pub fn explorer_tx_url(self, signature: &str) -> String {
        match self {
            Network::SolanaMainnet => format!("https://solscan.io/tx/{signature}"),
            Network::SolanaDevnet => format!("https://solscan.io/tx/{signature}?cluster=devnet"),
        }
    }

    pub fn from_caip2(s: &str) -> Result<Self, X402Error> {
        match s.trim() {
            Self::MAINNET_CAIP2 => Ok(Network::SolanaMainnet),
            Self::DEVNET_CAIP2 => Ok(Network::SolanaDevnet),
            other => Err(X402Error::Config(format!(
                "X402_NETWORK `{other}` is not a supported Solana CAIP-2 id"
            ))),
        }
    }

    pub fn matches_genesis(self, genesis_hash: &str) -> bool {
        self.caip2()
            .strip_prefix("solana:")
            .is_some_and(|reference| genesis_hash.starts_with(reference))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use solana_address::Address;

    use super::*;

    #[test]
    fn network_ids_and_mints_match_spec_tables() {
        assert_eq!(
            Network::from_caip2(Network::DEVNET_CAIP2).unwrap(),
            Network::SolanaDevnet
        );
        assert_eq!(
            Network::from_caip2(Network::MAINNET_CAIP2).unwrap(),
            Network::SolanaMainnet
        );
        assert!(
            Network::from_caip2("solana-devnet").is_err(),
            "v1 names are not CAIP-2"
        );
        assert!(Address::from_str(Network::MAINNET_USDC).is_ok());
        assert!(Address::from_str(Network::DEVNET_USDC).is_ok());
        assert!(
            Network::SolanaDevnet
                .explorer_tx_url("sig")
                .ends_with("cluster=devnet")
        );
        assert_eq!(
            Network::SolanaMainnet.explorer_tx_url("sig"),
            "https://solscan.io/tx/sig"
        );
    }

    #[test]
    fn genesis_hash_binds_to_its_own_network_only() {
        let devnet = "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";
        let mainnet = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";
        assert!(Network::SolanaDevnet.matches_genesis(devnet));
        assert!(Network::SolanaMainnet.matches_genesis(mainnet));
        assert!(!Network::SolanaDevnet.matches_genesis(mainnet));
        assert!(!Network::SolanaMainnet.matches_genesis(devnet));
        assert!(!Network::SolanaDevnet.matches_genesis(""));
        assert!(!Network::SolanaDevnet.matches_genesis("EtWT"));
    }
}
