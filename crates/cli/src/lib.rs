pub mod cli;
pub mod client;
pub mod config;
pub mod error;
pub mod flow;
pub mod keys;
pub mod rpc;
pub mod verify;
pub mod wire;

use std::path::{Path, PathBuf};
use std::str::FromStr;

use aetelier_ledger_x402::Address;
use aetelier_ledger_x402::tx::associated_token_address;
use serde::Serialize;
use serde_json::{Value, json};
use solana_signer::Signer;

use crate::cli::{Cli, Command};
use crate::client::Api;
use crate::config::{Endpoint, NetworkName, endpoint};
use crate::error::{CliError, Outcome};
use crate::flow::{BuyPlan, Confirm, Wait};
use crate::rpc::{RpcBalance, UsdcBalance, usdc_display};

pub struct Context<'a, C: Confirm> {
    pub api_key: Option<String>,
    pub mainnet_treasury: Option<Address>,
    pub confirm: &'a mut C,
    pub warn: &'a mut dyn FnMut(&str),
}

fn to_json<T: Serialize>(value: &T) -> Result<Value, CliError> {
    serde_json::to_value(value)
        .map_err(|e| CliError::transient("unexpected_response", e.to_string()))
}

fn required<T>(value: Option<T>, code: &str, message: &str) -> Result<T, CliError> {
    value.ok_or_else(|| CliError::usage(code, message))
}

fn keypair_path(path: Option<PathBuf>) -> Result<PathBuf, CliError> {
    required(
        path,
        "missing_keypair",
        "pass --keypair or set AETELIER_PAYER_KEYPAIR to the payer keypair file",
    )
}

pub async fn run<C: Confirm>(cli: Cli, ctx: Context<'_, C>) -> Outcome {
    match dispatch(cli, ctx).await {
        Ok(outcome) => outcome,
        Err(e) => Outcome::from_error(e),
    }
}

async fn dispatch<C: Confirm>(cli: Cli, ctx: Context<'_, C>) -> Result<Outcome, CliError> {
    let Context {
        api_key,
        mainnet_treasury,
        confirm,
        warn,
    } = ctx;
    let url = cli.url;
    let remote = || -> Result<(Endpoint, Api), CliError> {
        let endpoint = endpoint(&url)?;
        let api = Api::new(endpoint.url.clone(), config::api_key(api_key.clone()))?;
        Ok((endpoint, api))
    };
    let body = match cli.command {
        Command::Keygen { path } => keygen(&path)?,
        Command::Address {
            keypair,
            network,
            rpc_url,
        } => address(keypair, network, rpc_url).await?,
        Command::Catalog => to_json(&remote()?.1.catalog::<Value>().await?)?,
        Command::Status => to_json(&remote()?.1.status().await?)?,
        Command::Balance => to_json(&remote()?.1.entitlements().await?)?,
        Command::History { limit, offset } => to_json(&remote()?.1.ledger(limit, offset).await?)?,
        Command::Receipt { memo, wait } => {
            let (_, api) = remote()?;
            let (exit, body) = flow::receipt(&api, &memo, Wait::seconds(wait)).await?;
            return Ok(Outcome::with_exit(exit, body));
        }
        Command::Buy {
            sku,
            network,
            keypair,
            rpc_url,
            yes,
            wait,
        } => {
            let (endpoint, api) = remote()?;
            let network = required(
                network,
                "missing_network",
                "pass --network devnet|mainnet or set AETELIER_NETWORK to the network you expect to pay on",
            )?;
            api.require_api_key()?;
            let payer = keys::load_keypair(&keypair_path(keypair)?)?;
            let rpc_url = rpc_url.unwrap_or_else(|| network.network().default_rpc_url().into());
            let balance = RpcBalance::new(&rpc_url)?;
            let plan = BuyPlan {
                sku: &sku,
                network: network.network(),
                network_name: network.as_str(),
                payer: &payer,
                yes,
                wait: Wait::seconds(wait),
                mainnet_treasury,
                endpoint: &endpoint,
            };
            flow::buy(&api, Some(&balance), confirm, warn, &plan).await?
        }
    };
    Ok(Outcome::ok(body))
}

fn keygen(path: &Path) -> Result<Value, CliError> {
    let keypair = keys::keygen(path)?;
    Ok(json!({
        "path": keys::expand(path).display().to_string(),
        "address": keypair.pubkey().to_string(),
    }))
}

pub async fn address_with<B: UsdcBalance>(
    owner: &Address,
    network: NetworkName,
    balance: &B,
) -> Result<Value, CliError> {
    let net = network.network();
    let mint = Address::from_str(net.usdc_mint())
        .map_err(|e| CliError::usage("invalid_mint", e.to_string()))?;
    let held = balance
        .usdc_balance(owner, &mint)
        .await
        .map_err(|e| CliError::transient("rpc_error", e))?;
    Ok(json!({
        "address": owner.to_string(),
        "network": network.as_str(),
        "usdc_mint": net.usdc_mint(),
        "usdc_account": associated_token_address(owner, &mint).to_string(),
        "usdc_account_exists": held.is_some(),
        "usdc_balance_atomic": held,
        "usdc_balance": held.map(usdc_display),
    }))
}

async fn address(
    keypair: Option<PathBuf>,
    network: Option<NetworkName>,
    rpc_url: Option<String>,
) -> Result<Value, CliError> {
    let owner = keys::load_keypair(&keypair_path(keypair)?)?.pubkey();
    let Some(network) = network else {
        return Ok(json!({ "address": owner.to_string() }));
    };
    let rpc_url = rpc_url.unwrap_or_else(|| network.network().default_rpc_url().into());
    address_with(&owner, network, &RpcBalance::new(&rpc_url)?).await
}
