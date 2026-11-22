use std::str::FromStr;
use std::time::Duration;

use aetelier_ledger_x402::types::encode_header;
use aetelier_ledger_x402::{Address, Network, PaymentPayload, SvmPayload, X402_VERSION};
use serde_json::{Map, Value, json};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tokio::time::Instant;

use crate::client::{Api, DEFAULT_RETRY_AFTER, SubmitAnswer, http_error};
use crate::config::Endpoint;
use crate::error::{CliError, Exit};
use crate::rpc::{UsdcBalance, usdc_display};
use crate::verify::{Expectation, check_prepare};
use crate::wire::{Catalog, ReceiptState, TopupReceipt, TopupReply};

pub trait Confirm {
    fn confirm(&mut self, summary: &str) -> Result<bool, CliError>;
}

pub const MAX_WAIT: Duration = Duration::from_secs(365 * 24 * 3600);

pub struct TerminalConfirm;

impl Confirm for TerminalConfirm {
    fn confirm(&mut self, summary: &str) -> Result<bool, CliError> {
        use std::io::{BufRead, IsTerminal, Write};
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            return Err(CliError::refused(
                "confirmation_required",
                "stdin is not a terminal; pass --yes to buy without the prompt",
            ));
        }
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "{summary}\nProceed? [y/N] ");
        let _ = err.flush();
        let mut line = String::new();
        stdin
            .lock()
            .read_line(&mut line)
            .map_err(|e| CliError::refused("confirmation_required", e.to_string()))?;
        Ok(matches!(
            line.trim().to_ascii_lowercase().as_str(),
            "y" | "yes"
        ))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Wait {
    pub total: Duration,
    pub interval: Option<Duration>,
}

impl Wait {
    pub fn seconds(total: u64) -> Self {
        Self {
            total: Duration::from_secs(total),
            interval: None,
        }
    }
}

pub struct BuyPlan<'a> {
    pub sku: &'a str,
    pub network: Network,
    pub network_name: &'a str,
    pub payer: &'a Keypair,
    pub yes: bool,
    pub wait: Wait,
    pub mainnet_treasury: Option<Address>,
    pub endpoint: &'a Endpoint,
}

pub fn receipt_exit(receipt: &TopupReceipt) -> Exit {
    match receipt.state {
        ReceiptState::Settled => Exit::Ok,
        _ if receipt.is_final() => Exit::Rejected,
        _ => Exit::Pending,
    }
}

pub async fn buy<B: UsdcBalance, C: Confirm>(
    api: &Api,
    balance: Option<&B>,
    confirm: &mut C,
    warn: &mut dyn FnMut(&str),
    plan: &BuyPlan<'_>,
) -> Result<Value, CliError> {
    if plan.network == Network::SolanaMainnet && plan.mainnet_treasury.is_none() {
        return Err(CliError::refused(
            "mainnet_not_enabled",
            "this build has no mainnet treasury address, so it does not pay on mainnet",
        ));
    }
    api.require_api_key()?;
    let payer = plan.payer.pubkey();
    let catalog: Catalog = api.catalog().await?;
    let Some(bundle) = catalog.bundle(plan.sku).cloned() else {
        return Err(CliError::usage(
            "unknown_sku",
            format!("{} is not a bundle; run catalog for the list", plan.sku),
        ));
    };
    let mint = Address::from_str(plan.network.usdc_mint())
        .map_err(|e| CliError::usage("invalid_mint", e.to_string()))?;
    if let Some(source) = balance {
        match source.usdc_balance(&payer, &mint).await {
            Ok(None) => {
                return Err(CliError::rejected(
                    "no_usdc_account",
                    format!("{payer} has no USDC account yet; send USDC to {payer} first"),
                ));
            }
            Ok(Some(held)) if held < bundle.price_atomic => {
                return Err(CliError::rejected(
                    "insufficient_usdc",
                    format!(
                        "{payer} holds {} USDC, {} costs {} USDC",
                        usdc_display(held),
                        bundle.sku,
                        usdc_display(bundle.price_atomic)
                    ),
                ));
            }
            Ok(Some(_)) => {}
            Err(e) => warn(&format!(
                "could not read the USDC balance ({e}); continuing"
            )),
        }
    }
    let mut summary = format!(
        "Buy {}: {} {} tokens for {} USDC on {}\nPayer: {payer}",
        bundle.sku,
        bundle.units,
        bundle.kind.as_str(),
        usdc_display(bundle.price_atomic),
        plan.network_name
    );
    if !plan.endpoint.is_default {
        summary.push_str(&format!("\nHost: {}", plan.endpoint.host));
    }
    if !plan.yes && !confirm.confirm(&summary)? {
        return Err(CliError::refused("declined", "purchase cancelled"));
    }
    let sent = Instant::now();
    let reply = api.prepare(plan.sku, &payer.to_string()).await?;
    let memo = reply.memo.clone();
    let expectation = Expectation {
        network: plan.network,
        payer,
        amount: bundle.price_atomic,
        mainnet_treasury: plan.mainnet_treasury,
    };
    let mut tx =
        check_prepare(&reply, &expectation, sent.elapsed()).map_err(|e| e.with_memo(&memo))?;
    tx.sign_with_payer(plan.payer)
        .map_err(|e| CliError::refused("sign_failed", e.to_string()).with_memo(&memo))?;
    let transaction = tx
        .to_base64()
        .map_err(|e| CliError::refused("sign_failed", e.to_string()).with_memo(&memo))?;
    let payload = PaymentPayload {
        x402_version: X402_VERSION,
        accepted: reply.requirements.clone(),
        payload: SvmPayload { transaction },
        resource: None,
        extensions: Map::new(),
    };
    let header = encode_header(&payload)
        .map_err(|e| CliError::refused("sign_failed", e.to_string()).with_memo(&memo))?;
    let answer = match api.submit(plan.sku, &header).await {
        Ok(answer) => answer,
        Err(e) => {
            return Err(CliError::pending(
                &memo,
                format!("the payment was sent but no answer arrived ({e}); run receipt {memo}"),
            ));
        }
    };
    settle(api, answer, &memo, plan.network, plan.wait).await
}

async fn settle(
    api: &Api,
    answer: SubmitAnswer,
    memo: &str,
    network: Network,
    wait: Wait,
) -> Result<Value, CliError> {
    match (answer.status, answer.reply) {
        (202, _) => {
            let first = answer.retry_after;
            poll(api, memo, wait, first)
                .await
                .and_then(|reply| finish(reply, memo, network, None))
        }
        (status @ (200 | 409 | 410), Some(reply)) if reply.receipt.is_some() => {
            finish(reply, memo, network, Some(status))
        }
        (status @ 400..=499, _) => Err(http_error(status, &answer.body).with_memo(memo)),
        (503, Some(reply)) if reply.receipt.is_some() => {
            Err(http_error(503, &answer.body).with_memo(memo))
        }
        (status, _) => Err(CliError::pending(
            memo,
            format!(
                "the server answered HTTP {status} after the payment was sent, so it may still settle; run receipt {memo}"
            ),
        )
        .with_status(status)),
    }
}

async fn poll(
    api: &Api,
    memo: &str,
    wait: Wait,
    first: Option<Duration>,
) -> Result<TopupReply, CliError> {
    let start = Instant::now();
    let deadline = start
        .checked_add(wait.total)
        .unwrap_or_else(|| start + MAX_WAIT);
    let mut delay = wait.interval.or(first).unwrap_or(DEFAULT_RETRY_AFTER);
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(CliError::pending(
                memo,
                format!("the payment is still settling; run receipt {memo} later"),
            ));
        }
        tokio::time::sleep(delay.min(deadline - now)).await;
        match api.receipt(memo).await {
            Ok(reply) if reply.receipt.as_ref().is_some_and(TopupReceipt::is_final) => {
                return Ok(reply);
            }
            Ok(_) => {}
            Err(e) if e.exit == Exit::Transient => {}
            Err(e) => return Err(e.with_memo(memo)),
        }
        delay = wait.interval.unwrap_or(DEFAULT_RETRY_AFTER);
    }
}

fn finish(
    reply: TopupReply,
    memo: &str,
    network: Network,
    status: Option<u16>,
) -> Result<Value, CliError> {
    let Some(receipt) = reply.receipt.clone() else {
        return Err(CliError::transient(
            "unexpected_response",
            "the server answered without a receipt",
        )
        .with_memo(memo));
    };
    match receipt_exit(&receipt) {
        Exit::Ok => {
            let signature = receipt.signature.clone();
            let mut out = json!({
                "memo": receipt.memo,
                "signature": signature,
                "explorer_url": signature.as_deref().map(|s| network.explorer_tx_url(s)),
                "state": receipt.state,
                "receipt": receipt,
            });
            if let Some(balance) = reply.balance {
                out["balance"] = json!(balance);
            }
            Ok(out)
        }
        Exit::Pending => Err(CliError::pending(
            memo,
            format!("the payment did not settle yet and may still land; run receipt {memo} later"),
        )),
        _ => {
            let message = reply
                .error
                .clone()
                .or_else(|| receipt.error_reason.clone())
                .unwrap_or_else(|| format!("the top-up ended as {}", reply.code));
            let mut error = CliError::rejected(reply.code, message).with_memo(memo);
            if let Some(status) = status {
                error = error.with_status(status);
            }
            Err(error)
        }
    }
}

pub async fn receipt(api: &Api, memo: &str, wait: Wait) -> Result<(Exit, Value), CliError> {
    let first = api.receipt(memo).await?;
    let done = first.receipt.as_ref().is_some_and(TopupReceipt::is_final);
    let reply = if done || wait.total.is_zero() {
        first
    } else {
        match poll(api, memo, wait, None).await {
            Ok(reply) => reply,
            Err(e) if e.exit == Exit::Pending => api.receipt(memo).await?,
            Err(e) => return Err(e),
        }
    };
    let exit = reply
        .receipt
        .as_ref()
        .map(receipt_exit)
        .unwrap_or(Exit::Pending);
    let body = serde_json::to_value(&reply)
        .map_err(|e| CliError::transient("unexpected_response", e.to_string()))?;
    Ok((exit, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{ReceiptFailure, TokenKind};
    use chrono::Utc;

    fn receipt(state: ReceiptState, failure: Option<ReceiptFailure>) -> TopupReceipt {
        TopupReceipt {
            memo: "m1".into(),
            sku: "data_100".into(),
            kind: TokenKind::Data,
            units: 100,
            amount: 1_000_000,
            asset: Network::DEVNET_USDC.into(),
            network: Network::DEVNET_CAIP2.into(),
            payer: "payer".into(),
            state,
            signature: (state == ReceiptState::Settled).then(|| "sig".into()),
            pending_signature: None,
            failure,
            error_reason: failure.map(|_| "insufficient_funds".into()),
            created_at: Utc::now(),
            expires_at: Utc::now(),
            settled_at: None,
        }
    }

    fn reply(code: &str, receipt: TopupReceipt, balance: Option<i64>) -> TopupReply {
        TopupReply {
            code: code.into(),
            error: None,
            receipt: Some(receipt),
            balance,
        }
    }

    #[tokio::test]
    async fn an_enormous_wait_does_not_overflow_the_deadline() {
        let api = Api::new(
            url::Url::parse("http://127.0.0.1:9").unwrap(),
            Some("k".into()),
        )
        .unwrap();
        let wait = Wait {
            total: Duration::from_secs(u64::MAX),
            interval: Some(Duration::from_millis(1)),
        };
        let polled =
            tokio::time::timeout(Duration::from_millis(300), poll(&api, "m1", wait, None)).await;
        assert!(polled.is_err(), "poll should still be waiting");
    }

    #[test]
    fn receipt_states_map_to_exit_codes() {
        let cases = [
            (ReceiptState::Settled, None, Exit::Ok),
            (ReceiptState::Expired, None, Exit::Rejected),
            (
                ReceiptState::Failed,
                Some(ReceiptFailure::VerifyRejected),
                Exit::Rejected,
            ),
            (
                ReceiptState::Failed,
                Some(ReceiptFailure::NotLanded),
                Exit::Pending,
            ),
            (ReceiptState::Settling, None, Exit::Pending),
            (ReceiptState::Open, None, Exit::Pending),
        ];
        for (state, failure, exit) in cases {
            assert_eq!(receipt_exit(&receipt(state, failure)), exit, "{state:?}");
        }
    }

    #[test]
    fn settled_answers_carry_signature_explorer_link_and_balance() {
        let out = finish(
            reply("settled", receipt(ReceiptState::Settled, None), Some(1100)),
            "m1",
            Network::SolanaDevnet,
            Some(200),
        )
        .unwrap();
        assert_eq!(out["memo"], "m1");
        assert_eq!(out["signature"], "sig");
        assert_eq!(
            out["explorer_url"],
            "https://solscan.io/tx/sig?cluster=devnet"
        );
        assert_eq!(out["state"], "settled");
        assert_eq!(out["balance"], 1100);
        assert_eq!(out["receipt"]["sku"], "data_100");
    }

    #[test]
    fn final_failures_are_rejections_with_memo_and_status() {
        let e = finish(
            reply(
                "intent_failed",
                receipt(ReceiptState::Failed, Some(ReceiptFailure::VerifyRejected)),
                None,
            ),
            "m1",
            Network::SolanaDevnet,
            Some(409),
        )
        .unwrap_err();
        assert_eq!(e.exit, Exit::Rejected);
        assert_eq!(e.code(), "intent_failed");
        assert_eq!(e.body.message, "insufficient_funds");
        assert_eq!(e.body.http_status, Some(409));
        assert_eq!(e.body.memo.as_deref(), Some("m1"));
    }

    #[test]
    fn not_landed_failures_stay_pending() {
        let e = finish(
            reply(
                "intent_failed",
                receipt(ReceiptState::Failed, Some(ReceiptFailure::NotLanded)),
                None,
            ),
            "m1",
            Network::SolanaDevnet,
            Some(409),
        )
        .unwrap_err();
        assert_eq!(e.exit, Exit::Pending);
        assert_eq!(e.body.memo.as_deref(), Some("m1"));
    }
}
