use std::str::FromStr;
use std::time::Duration;

use crate::error::CliError;
use aetelier_ledger_x402::{Address, BuildParams, Network, PrepareReply, TopupTx};

pub const SCHEME_EXACT: &str = "exact";

#[derive(Clone, Debug)]
pub struct Expectation {
    pub network: Network,
    pub payer: Address,
    pub amount: u64,
    pub mainnet_treasury: Option<Address>,
}

fn refuse(code: &str, message: impl Into<String>) -> CliError {
    CliError::refused(code, message)
}

fn address(field: &str, text: &str) -> Result<Address, CliError> {
    Address::from_str(text).map_err(|_| {
        refuse(
            "invalid_requirements",
            format!("{field} `{text}` is not an address"),
        )
    })
}

pub fn check_prepare(
    reply: &PrepareReply,
    expect: &Expectation,
    elapsed: Duration,
) -> Result<TopupTx, CliError> {
    let req = &reply.requirements;
    if req.scheme != SCHEME_EXACT {
        return Err(refuse(
            "scheme_mismatch",
            format!(
                "expected scheme `exact`, the server asked for `{}`",
                req.scheme
            ),
        ));
    }
    if req.network != expect.network.caip2() {
        return Err(refuse(
            "network_mismatch",
            format!(
                "you chose {}, the server asked for payment on {}",
                expect.network.caip2(),
                req.network
            ),
        ));
    }
    if req.asset != expect.network.usdc_mint() {
        return Err(refuse(
            "asset_mismatch",
            format!(
                "the server asked for token {}, not USDC ({})",
                req.asset,
                expect.network.usdc_mint()
            ),
        ));
    }
    let pay_to = address("payTo", &req.pay_to)?;
    if expect.network == Network::SolanaMainnet {
        match expect.mainnet_treasury {
            Some(treasury) if treasury == pay_to => {}
            Some(treasury) => {
                return Err(refuse(
                    "pay_to_mismatch",
                    format!(
                        "the server asked to pay {pay_to}, the Aetelier treasury is {treasury}"
                    ),
                ));
            }
            None => {
                return Err(refuse(
                    "mainnet_not_enabled",
                    "this build has no mainnet treasury address",
                ));
            }
        }
    }
    let amount: u64 = req.amount.parse().map_err(|_| {
        refuse(
            "invalid_requirements",
            format!("amount `{}` is not a whole number", req.amount),
        )
    })?;
    if amount != expect.amount {
        return Err(refuse(
            "amount_mismatch",
            format!(
                "you confirmed {} atomic USDC, the server asked for {amount}",
                expect.amount
            ),
        ));
    }
    if reply.memo.is_empty() || req.memo() != Some(reply.memo.as_str()) {
        return Err(refuse(
            "memo_mismatch",
            "the memo in the requirements differs from the prepared memo",
        ));
    }
    let Some(fee_payer) = req.fee_payer() else {
        return Err(refuse(
            "fee_payer_missing",
            "the requirements name no fee payer",
        ));
    };
    let fee_payer = address("feePayer", fee_payer)?;
    if fee_payer == expect.payer {
        return Err(refuse(
            "fee_payer_is_payer",
            "the fee payer must not be your payer address",
        ));
    }
    if i128::from(reply.expires_in_ms) <= elapsed.as_millis() as i128 {
        return Err(refuse(
            "prepare_expired",
            "the prepared payment expired before it could be signed",
        ));
    }
    let received = TopupTx::from_base64(&reply.transaction)
        .map_err(|e| refuse("transaction_undecodable", e.to_string()))?;
    if received.to_base64().ok().as_deref() != Some(reply.transaction.as_str()) {
        return Err(refuse(
            "transaction_noncanonical",
            "the prepared transaction is not canonically encoded",
        ));
    }
    let mint = address("asset", &req.asset)?;
    let blockhash = *received.tx.message.recent_blockhash();
    let rebuilt = TopupTx::build(&BuildParams::usdc(
        fee_payer,
        expect.payer,
        pay_to,
        mint,
        amount,
        reply.memo.clone(),
        blockhash,
    ))
    .map_err(|e| refuse("transaction_mismatch", e.to_string()))?;
    if rebuilt != received {
        return Err(refuse(
            "transaction_mismatch",
            "the prepared transaction is not the USDC transfer described by the requirements",
        ));
    }
    Ok(received)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use aetelier_ledger_x402::tx::{
        associated_token_address, memo, set_compute_unit_limit, set_compute_unit_price,
        transfer_checked,
    };
    use aetelier_ledger_x402::{Hash, PaymentRequirements, Signature};
    use chrono::{DateTime, Duration as Span, Utc};
    use serde_json::{Map, json};
    use solana_instruction::Instruction;
    use solana_keypair::Keypair;
    use solana_message::{VersionedMessage, v0};
    use solana_signer::Signer;
    use solana_transaction::versioned::VersionedTransaction;

    type ReplyEdit = Box<dyn Fn(&mut PrepareReply)>;
    type ParamsEdit = Box<dyn Fn(&mut BuildParams)>;

    const BLOCKHASH: [u8; 32] = [3; 32];
    const FRESH: Duration = Duration::ZERO;
    const MEMO: &str = "aetelier-topup-m1";

    pub struct Fixture {
        pub payer: Keypair,
        pub fee_payer: Address,
        pub pay_to: Address,
        pub mint: Address,
        pub reply: PrepareReply,
    }

    pub fn fixture(network: Network, amount: u64) -> Fixture {
        let payer = Keypair::new();
        let fee_payer = Keypair::new().pubkey();
        let pay_to = Keypair::new().pubkey();
        let mint = Address::from_str(network.usdc_mint()).unwrap();
        let tx = TopupTx::build(&BuildParams::usdc(
            fee_payer,
            payer.pubkey(),
            pay_to,
            mint,
            amount,
            MEMO,
            Hash::new_from_array(BLOCKHASH),
        ))
        .unwrap();
        let mut extra = Map::new();
        extra.insert("feePayer".into(), json!(fee_payer.to_string()));
        extra.insert("memo".into(), json!(MEMO));
        let now: DateTime<Utc> = Utc::now();
        let reply = PrepareReply {
            code: "payment_required".into(),
            memo: MEMO.into(),
            transaction: tx.to_base64().unwrap(),
            expires_at: now + Span::seconds(25),
            expires_in_ms: 25_000,
            last_valid_block_height: 1,
            requirements: PaymentRequirements {
                scheme: "exact".into(),
                network: network.caip2().into(),
                amount: amount.to_string(),
                asset: network.usdc_mint().into(),
                pay_to: pay_to.to_string(),
                max_timeout_seconds: 25,
                extra,
            },
        };
        Fixture {
            payer,
            fee_payer,
            pay_to,
            mint,
            reply,
        }
    }

    fn expect(f: &Fixture, network: Network, treasury: Option<Address>) -> Expectation {
        Expectation {
            network,
            payer: f.payer.pubkey(),
            amount: 1_000_000,
            mainnet_treasury: treasury,
        }
    }

    fn refusal(reply: &PrepareReply, e: &Expectation, elapsed: Duration) -> String {
        let err = check_prepare(reply, e, elapsed).unwrap_err();
        assert_eq!(err.exit, crate::error::Exit::Refused);
        err.code().to_string()
    }

    fn params(f: &Fixture) -> BuildParams {
        BuildParams::usdc(
            f.fee_payer,
            f.payer.pubkey(),
            f.pay_to,
            f.mint,
            1_000_000,
            MEMO,
            Hash::new_from_array(BLOCKHASH),
        )
    }

    fn unsigned(fee_payer: &Address, instructions: &[Instruction]) -> String {
        let message = v0::Message::try_compile(
            fee_payer,
            instructions,
            &[],
            Hash::new_from_array(BLOCKHASH),
        )
        .unwrap();
        let signatures =
            vec![Signature::default(); message.header.num_required_signatures as usize];
        TopupTx {
            tx: VersionedTransaction {
                signatures,
                message: VersionedMessage::V0(message),
            },
        }
        .to_base64()
        .unwrap()
    }

    fn honest_instructions(f: &Fixture) -> Vec<Instruction> {
        let payer = f.payer.pubkey();
        vec![
            set_compute_unit_limit(20_000),
            set_compute_unit_price(1),
            transfer_checked(
                &associated_token_address(&payer, &f.mint),
                &f.mint,
                &associated_token_address(&f.pay_to, &f.mint),
                &payer,
                1_000_000,
                6,
            ),
            memo(MEMO),
        ]
    }

    #[test]
    fn an_honest_devnet_prepare_passes_and_signs_for_submission() {
        let f = fixture(Network::SolanaDevnet, 1_000_000);
        let mut tx =
            check_prepare(&f.reply, &expect(&f, Network::SolanaDevnet, None), FRESH).unwrap();
        tx.sign_with_payer(&f.payer).unwrap();
        let message = tx.message_bytes().unwrap();
        TopupTx::from_submission(&tx.to_base64().unwrap(), &message, &f.payer.pubkey()).unwrap();
    }

    #[test]
    fn the_hand_built_honest_transaction_matches_the_crate_builder() {
        let f = fixture(Network::SolanaDevnet, 1_000_000);
        assert_eq!(
            unsigned(&f.fee_payer, &honest_instructions(&f)),
            f.reply.transaction
        );
    }

    #[test]
    fn mainnet_requires_the_pinned_treasury() {
        let f = fixture(Network::SolanaMainnet, 1_000_000);
        let pinned = expect(&f, Network::SolanaMainnet, Some(f.pay_to));
        assert!(check_prepare(&f.reply, &pinned, FRESH).is_ok());
        let other = expect(&f, Network::SolanaMainnet, Some(Keypair::new().pubkey()));
        assert_eq!(refusal(&f.reply, &other, FRESH), "pay_to_mismatch");
        let unpinned = expect(&f, Network::SolanaMainnet, None);
        assert_eq!(refusal(&f.reply, &unpinned, FRESH), "mainnet_not_enabled");
    }

    #[test]
    fn devnet_accepts_any_pay_to() {
        let f = fixture(Network::SolanaDevnet, 1_000_000);
        let e = expect(&f, Network::SolanaDevnet, Some(Keypair::new().pubkey()));
        assert!(check_prepare(&f.reply, &e, FRESH).is_ok());
    }

    #[test]
    fn requirement_tampering_is_refused() {
        let f = fixture(Network::SolanaDevnet, 1_000_000);
        let e = expect(&f, Network::SolanaDevnet, None);
        let payer = f.payer.pubkey().to_string();
        let cases: Vec<(&str, ReplyEdit)> = vec![
            (
                "scheme_mismatch",
                Box::new(|r| r.requirements.scheme = "upto".into()),
            ),
            (
                "network_mismatch",
                Box::new(|r| r.requirements.network = Network::MAINNET_CAIP2.into()),
            ),
            (
                "asset_mismatch",
                Box::new(|r| r.requirements.asset = Network::MAINNET_USDC.into()),
            ),
            (
                "amount_mismatch",
                Box::new(|r| r.requirements.amount = "1000001".into()),
            ),
            (
                "invalid_requirements",
                Box::new(|r| r.requirements.amount = "1.0".into()),
            ),
            (
                "invalid_requirements",
                Box::new(|r| r.requirements.pay_to = "x".into()),
            ),
            ("memo_mismatch", Box::new(|r| r.memo = "other".into())),
            (
                "memo_mismatch",
                Box::new(|r| {
                    r.requirements.extra.remove("memo");
                }),
            ),
            (
                "fee_payer_missing",
                Box::new(|r| {
                    r.requirements.extra.remove("feePayer");
                }),
            ),
            (
                "fee_payer_is_payer",
                Box::new(move |r| {
                    r.requirements
                        .extra
                        .insert("feePayer".into(), json!(payer.clone()));
                }),
            ),
            ("prepare_expired", Box::new(|r| r.expires_in_ms = 0)),
            ("prepare_expired", Box::new(|r| r.expires_in_ms = -5)),
            (
                "transaction_undecodable",
                Box::new(|r| r.transaction = "@@".into()),
            ),
        ];
        for (code, edit) in cases {
            let mut reply = f.reply.clone();
            edit(&mut reply);
            assert_eq!(refusal(&reply, &e, FRESH), code);
        }
    }

    #[test]
    fn transaction_tampering_is_refused() {
        let f = fixture(Network::SolanaDevnet, 1_000_000);
        let e = expect(&f, Network::SolanaDevnet, None);
        let other = Keypair::new().pubkey();
        let edits: Vec<ParamsEdit> = vec![
            Box::new(|p| p.amount = 2_000_000),
            Box::new(move |p| p.pay_to = other),
            Box::new(|p| p.memo = "aetelier-topup-m2".into()),
            Box::new(|p| p.compute_unit_price = 1_000_000),
            Box::new(|p| p.compute_unit_limit = 200_000),
            Box::new(|p| p.compute_unit_limit = 1_400_000),
            Box::new(move |p| p.fee_payer = other),
        ];
        for edit in edits {
            let mut p = params(&f);
            edit(&mut p);
            let mut reply = f.reply.clone();
            reply.transaction = TopupTx::build(&p).unwrap().to_base64().unwrap();
            let code = refusal(&reply, &e, FRESH);
            assert!(code == "transaction_mismatch", "{code}");
        }
    }

    #[test]
    fn the_signing_window_is_measured_from_the_request_not_the_clocks() {
        let mut f = fixture(Network::SolanaDevnet, 1_000_000);
        let e = expect(&f, Network::SolanaDevnet, None);
        f.reply.expires_at = Utc::now() - Span::hours(1);
        assert!(check_prepare(&f.reply, &e, Duration::from_millis(24_999)).is_ok());
        assert_eq!(
            refusal(&f.reply, &e, Duration::from_millis(25_000)),
            "prepare_expired"
        );
    }

    #[test]
    fn flipping_any_single_byte_is_refused_unless_only_the_blockhash_moved() {
        use base64::Engine;
        use base64::engine::general_purpose::STANDARD;
        let f = fixture(Network::SolanaDevnet, 1_000_000);
        let e = expect(&f, Network::SolanaDevnet, None);
        let original = TopupTx::from_base64(&f.reply.transaction).unwrap();
        let bytes = STANDARD.decode(&f.reply.transaction).unwrap();
        let mut refused = 0;
        for i in 0..bytes.len() {
            let mut flipped = bytes.clone();
            flipped[i] ^= 0x01;
            let mut reply = f.reply.clone();
            reply.transaction = STANDARD.encode(&flipped);
            match check_prepare(&reply, &e, FRESH) {
                Err(err) => {
                    assert!(
                        matches!(
                            err.code(),
                            "transaction_mismatch"
                                | "transaction_undecodable"
                                | "transaction_noncanonical"
                        ),
                        "byte {i}: {}",
                        err.code()
                    );
                    refused += 1;
                }
                Ok(tx) => {
                    let (got, want) = (&tx.tx.message, &original.tx.message);
                    assert_ne!(got.recent_blockhash(), want.recent_blockhash(), "byte {i}");
                    assert_eq!(got.header(), want.header(), "byte {i}");
                    assert_eq!(
                        got.static_account_keys(),
                        want.static_account_keys(),
                        "byte {i}"
                    );
                    assert_eq!(got.instructions(), want.instructions(), "byte {i}");
                    assert_eq!(tx.tx.signatures, original.tx.signatures, "byte {i}");
                }
            }
        }
        assert_eq!(refused, bytes.len() - 32);
    }

    #[test]
    fn the_server_chosen_blockhash_is_taken_from_the_transaction() {
        let f = fixture(Network::SolanaDevnet, 1_000_000);
        let mut p = params(&f);
        p.blockhash = Hash::new_from_array([4; 32]);
        let mut reply = f.reply.clone();
        reply.transaction = TopupTx::build(&p).unwrap().to_base64().unwrap();
        let tx = check_prepare(&reply, &expect(&f, Network::SolanaDevnet, None), FRESH).unwrap();
        assert_eq!(
            *tx.tx.message.recent_blockhash(),
            Hash::new_from_array([4; 32])
        );
    }

    #[test]
    fn extra_missing_or_presigned_parts_are_refused() {
        let f = fixture(Network::SolanaDevnet, 1_000_000);
        let e = expect(&f, Network::SolanaDevnet, None);

        let mut extra = honest_instructions(&f);
        extra.push(memo("second memo"));
        let mut reply = f.reply.clone();
        reply.transaction = unsigned(&f.fee_payer, &extra);
        assert_eq!(refusal(&reply, &e, FRESH), "transaction_mismatch");

        let mut missing = honest_instructions(&f);
        missing.pop();
        let mut reply = f.reply.clone();
        reply.transaction = unsigned(&f.fee_payer, &missing);
        assert_eq!(refusal(&reply, &e, FRESH), "transaction_mismatch");

        let mut tx = TopupTx::from_base64(&f.reply.transaction).unwrap();
        tx.sign_with_payer(&f.payer).unwrap();
        let mut reply = f.reply.clone();
        reply.transaction = tx.to_base64().unwrap();
        assert_eq!(refusal(&reply, &e, FRESH), "transaction_mismatch");
    }
}
