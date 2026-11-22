use std::future::Future;

use aetelier_ledger_x402::tx::associated_token_address;
use aetelier_ledger_x402::{Address, SolanaRpc};

use crate::error::CliError;

pub trait UsdcBalance {
    fn usdc_balance(
        &self,
        owner: &Address,
        mint: &Address,
    ) -> impl Future<Output = Result<Option<u64>, String>>;
}

pub struct RpcBalance {
    rpc: SolanaRpc,
}

impl RpcBalance {
    pub fn new(url: &str) -> Result<Self, CliError> {
        SolanaRpc::new(url)
            .map(|rpc| Self { rpc })
            .map_err(|e| CliError::usage("invalid_rpc_url", format!("--rpc-url {url}: {e}")))
    }
}

impl UsdcBalance for RpcBalance {
    fn usdc_balance(
        &self,
        owner: &Address,
        mint: &Address,
    ) -> impl Future<Output = Result<Option<u64>, String>> {
        let account = associated_token_address(owner, mint);
        async move {
            self.rpc
                .token_balance(&account)
                .await
                .map_err(|e| e.to_string())
        }
    }
}

pub fn usdc_display(atomic: u64) -> String {
    let fraction = format!("{:06}", atomic % 1_000_000);
    let trimmed = fraction.trim_end_matches('0');
    let shown = if trimmed.len() < 2 {
        &fraction[..2]
    } else {
        trimmed
    };
    format!("{}.{shown}", atomic / 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usdc_amounts_show_at_least_cents() {
        assert_eq!(usdc_display(0), "0.00");
        assert_eq!(usdc_display(1_000_000), "1.00");
        assert_eq!(usdc_display(9_500_000), "9.50");
        assert_eq!(usdc_display(400_000), "0.40");
        assert_eq!(usdc_display(1_234_567), "1.234567");
        assert_eq!(usdc_display(10_001), "0.010001");
    }
}
