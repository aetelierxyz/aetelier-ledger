use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use solana_address::Address;
use solana_hash::Hash;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::{VersionedMessage, v0};
use solana_signature::Signature;
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

use crate::error::{SubmissionReject, X402Error};

pub const TOKEN_PROGRAM: Address =
    Address::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const TOKEN_2022_PROGRAM: Address =
    Address::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnwqTM9GwX4Ks");
pub const ASSOCIATED_TOKEN_PROGRAM: Address =
    Address::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
pub const MEMO_PROGRAM: Address =
    Address::from_str_const("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
pub const COMPUTE_BUDGET_PROGRAM: Address =
    Address::from_str_const("ComputeBudget111111111111111111111111111111");

pub const USDC_DECIMALS: u8 = 6;

pub const DEFAULT_COMPUTE_UNIT_LIMIT: u32 = 20_000;
pub const DEFAULT_COMPUTE_UNIT_PRICE: u64 = 1;

pub fn associated_token_address(owner: &Address, mint: &Address) -> Address {
    associated_token_address_for_program(owner, mint, &TOKEN_PROGRAM)
}

pub fn associated_token_address_for_program(
    owner: &Address,
    mint: &Address,
    token_program: &Address,
) -> Address {
    Address::find_program_address(
        &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ASSOCIATED_TOKEN_PROGRAM,
    )
    .0
}

pub fn set_compute_unit_limit(units: u32) -> Instruction {
    let mut data = Vec::with_capacity(5);
    data.push(2u8);
    data.extend_from_slice(&units.to_le_bytes());
    Instruction::new_with_bytes(COMPUTE_BUDGET_PROGRAM, &data, vec![])
}

pub fn set_compute_unit_price(micro_lamports: u64) -> Instruction {
    let mut data = Vec::with_capacity(9);
    data.push(3u8);
    data.extend_from_slice(&micro_lamports.to_le_bytes());
    Instruction::new_with_bytes(COMPUTE_BUDGET_PROGRAM, &data, vec![])
}

pub fn transfer_checked(
    source: &Address,
    mint: &Address,
    destination: &Address,
    owner: &Address,
    amount: u64,
    decimals: u8,
) -> Instruction {
    let mut data = Vec::with_capacity(10);
    data.push(12u8);
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(decimals);
    Instruction::new_with_bytes(
        TOKEN_PROGRAM,
        &data,
        vec![
            AccountMeta::new(*source, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(*destination, false),
            AccountMeta::new_readonly(*owner, true),
        ],
    )
}

pub fn memo(text: &str) -> Instruction {
    Instruction::new_with_bytes(MEMO_PROGRAM, text.as_bytes(), vec![])
}

#[derive(Clone, Debug)]
pub struct BuildParams {
    pub fee_payer: Address,
    pub payer: Address,
    pub pay_to: Address,
    pub mint: Address,
    pub decimals: u8,
    pub amount: u64,
    pub memo: String,
    pub blockhash: Hash,
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
}

impl BuildParams {
    pub fn usdc(
        fee_payer: Address,
        payer: Address,
        pay_to: Address,
        mint: Address,
        amount: u64,
        memo: impl Into<String>,
        blockhash: Hash,
    ) -> Self {
        Self {
            fee_payer,
            payer,
            pay_to,
            mint,
            decimals: USDC_DECIMALS,
            amount,
            memo: memo.into(),
            blockhash,
            compute_unit_limit: DEFAULT_COMPUTE_UNIT_LIMIT,
            compute_unit_price: DEFAULT_COMPUTE_UNIT_PRICE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopupTx {
    pub tx: VersionedTransaction,
}

impl TopupTx {
    pub fn build(p: &BuildParams) -> Result<Self, X402Error> {
        if p.amount == 0 {
            return Err(X402Error::Build("amount must be > 0".into()));
        }
        if p.memo.is_empty() || p.memo.len() > 256 {
            return Err(X402Error::Build("memo must be 1..=256 bytes".into()));
        }
        if p.fee_payer == p.payer {
            return Err(X402Error::Build("fee payer must differ from payer".into()));
        }
        let source = associated_token_address(&p.payer, &p.mint);
        let destination = associated_token_address(&p.pay_to, &p.mint);
        let ixs = [
            set_compute_unit_limit(p.compute_unit_limit),
            set_compute_unit_price(p.compute_unit_price),
            transfer_checked(
                &source,
                &p.mint,
                &destination,
                &p.payer,
                p.amount,
                p.decimals,
            ),
            memo(&p.memo),
        ];
        let message = v0::Message::try_compile(&p.fee_payer, &ixs, &[], p.blockhash)
            .map_err(|e| X402Error::Build(format!("compile: {e}")))?;
        let n = message.header.num_required_signatures as usize;
        Ok(Self {
            tx: VersionedTransaction {
                signatures: vec![Signature::default(); n],
                message: VersionedMessage::V0(message),
            },
        })
    }

    pub fn message_bytes(&self) -> Result<Vec<u8>, X402Error> {
        bincode::serialize(&self.tx.message).map_err(|e| X402Error::Build(e.to_string()))
    }

    pub fn static_keys(&self) -> &[Address] {
        self.tx.message.static_account_keys()
    }

    pub fn num_required_signatures(&self) -> usize {
        self.tx.message.header().num_required_signatures as usize
    }

    pub fn signer_index(&self, key: &Address) -> Option<usize> {
        self.static_keys()
            .get(..self.num_required_signatures())?
            .iter()
            .position(|k| k == key)
    }

    pub fn sign_with_payer(&mut self, payer: &Keypair) -> Result<(), X402Error> {
        let idx = self
            .signer_index(&payer.pubkey())
            .ok_or_else(|| X402Error::Build("payer is not a required signer".into()))?;
        let sig = payer.sign_message(&self.message_bytes()?);
        let slot = self
            .tx
            .signatures
            .get_mut(idx)
            .ok_or_else(|| X402Error::Build("payer signature slot is missing".into()))?;
        *slot = sig;
        Ok(())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, X402Error> {
        bincode::serialize(&self.tx).map_err(|e| X402Error::Build(e.to_string()))
    }

    pub fn to_base64(&self) -> Result<String, X402Error> {
        Ok(B64.encode(self.to_bytes()?))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, X402Error> {
        let tx: VersionedTransaction =
            bincode::deserialize(bytes).map_err(|e| X402Error::Decode(e.to_string()))?;
        Ok(Self { tx })
    }

    pub fn from_base64(s: &str) -> Result<Self, X402Error> {
        let bytes = B64
            .decode(s.trim())
            .map_err(|e| X402Error::Decode(format!("transaction base64: {e}")))?;
        Self::from_bytes(&bytes)
    }

    pub fn from_submission(
        b64: &str,
        expected_message: &[u8],
        payer: &Address,
    ) -> Result<Self, X402Error> {
        let reject = X402Error::InvalidSubmission;
        let bytes = B64
            .decode(b64)
            .map_err(|_| reject(SubmissionReject::Base64))?;
        let tx: VersionedTransaction =
            bincode::deserialize(&bytes).map_err(|_| reject(SubmissionReject::Bincode))?;
        let topup = Self { tx };
        if topup.to_base64()? != b64 {
            return Err(reject(SubmissionReject::NonCanonical));
        }
        topup
            .tx
            .sanitize()
            .map_err(|_| reject(SubmissionReject::Sanitize))?;
        if topup.message_bytes()? != expected_message {
            return Err(reject(SubmissionReject::MessageMismatch));
        }
        if topup.tx.signatures.first() != Some(&Signature::default()) {
            return Err(reject(SubmissionReject::FeePayerSigned));
        }
        let signed = topup
            .signer_index(payer)
            .and_then(|idx| topup.tx.signatures.get(idx))
            .is_some_and(|sig| sig.verify(payer.as_ref(), expected_message));
        if !signed {
            return Err(reject(SubmissionReject::PayerSignature));
        }
        Ok(topup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr() -> Address {
        Keypair::new().pubkey()
    }

    fn params(fee_payer: Address, payer: Address) -> BuildParams {
        BuildParams::usdc(
            fee_payer,
            payer,
            addr(),
            Address::from_str_const("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"),
            1_000_000,
            "data_100:user:0123456789abcdef",
            Hash::new_from_array([7u8; 32]),
        )
    }

    #[test]
    fn ata_is_deterministic_and_off_curve() {
        let owner = addr();
        let mint = Address::from_str_const("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU");
        let a = associated_token_address(&owner, &mint);
        assert_eq!(a, associated_token_address(&owner, &mint));
        assert!(!a.is_on_curve(), "a PDA is never on the ed25519 curve");
        assert_ne!(
            a,
            associated_token_address_for_program(&owner, &mint, &TOKEN_2022_PROGRAM)
        );
    }

    #[test]
    fn layout_matches_spec_path_1() {
        let sponsor = addr();
        let payer_kp = Keypair::new();
        let p = params(sponsor, payer_kp.pubkey());
        let built = TopupTx::build(&p).unwrap();

        assert_eq!(built.num_required_signatures(), 2);
        assert_eq!(built.static_keys()[0], sponsor, "fee payer is account 0");
        assert_eq!(built.signer_index(&payer_kp.pubkey()), Some(1));
        assert!(
            built
                .tx
                .signatures
                .iter()
                .all(|s| *s == Signature::default())
        );

        let keys = built.static_keys();
        let ixs = built.tx.message.instructions();
        assert_eq!(ixs.len(), 4);
        let programs: Vec<Address> = ixs
            .iter()
            .map(|ix| keys[ix.program_id_index as usize])
            .collect();
        assert_eq!(
            programs,
            vec![
                COMPUTE_BUDGET_PROGRAM,
                COMPUTE_BUDGET_PROGRAM,
                TOKEN_PROGRAM,
                MEMO_PROGRAM
            ]
        );
        assert_eq!(ixs[0].data[0], 2);
        assert_eq!(
            u32::from_le_bytes(ixs[0].data[1..5].try_into().unwrap()),
            DEFAULT_COMPUTE_UNIT_LIMIT
        );
        assert_eq!(ixs[1].data[0], 3);
        assert_eq!(
            u64::from_le_bytes(ixs[1].data[1..9].try_into().unwrap()),
            DEFAULT_COMPUTE_UNIT_PRICE
        );
        assert_eq!(ixs[2].data[0], 12);
        assert_eq!(
            u64::from_le_bytes(ixs[2].data[1..9].try_into().unwrap()),
            1_000_000
        );
        assert_eq!(ixs[2].data[9], USDC_DECIMALS);
        let transfer_accounts: Vec<Address> =
            ixs[2].accounts.iter().map(|i| keys[*i as usize]).collect();
        assert_eq!(
            transfer_accounts[0],
            associated_token_address(&p.payer, &p.mint)
        );
        assert_eq!(transfer_accounts[1], p.mint);
        assert_eq!(
            transfer_accounts[2],
            associated_token_address(&p.pay_to, &p.mint)
        );
        assert_eq!(transfer_accounts[3], p.payer);
        assert_eq!(ixs[3].data, p.memo.as_bytes());

        for ix in ixs {
            assert!(
                !ix.accounts.contains(&0),
                "fee payer must not be an instruction account"
            );
            assert_ne!(ix.program_id_index, 0);
        }
    }

    #[test]
    fn payer_signature_fills_its_slot_and_verifies() {
        let sponsor = addr();
        let payer_kp = Keypair::new();
        let mut built = TopupTx::build(&params(sponsor, payer_kp.pubkey())).unwrap();
        built.sign_with_payer(&payer_kp).unwrap();
        assert_eq!(
            built.tx.signatures[0],
            Signature::default(),
            "sponsor slot empty"
        );
        let msg = built.message_bytes().unwrap();
        assert!(built.tx.signatures[1].verify(payer_kp.pubkey().as_ref(), &msg));

        let stranger = Keypair::new();
        assert!(matches!(
            built.sign_with_payer(&stranger),
            Err(X402Error::Build(_))
        ));
    }

    #[test]
    fn base64_round_trip_preserves_partial_signature() {
        let payer_kp = Keypair::new();
        let mut built = TopupTx::build(&params(addr(), payer_kp.pubkey())).unwrap();
        built.sign_with_payer(&payer_kp).unwrap();
        let b64 = built.to_base64().unwrap();
        let back = TopupTx::from_base64(&b64).unwrap();
        assert_eq!(back, built);
        assert!(TopupTx::from_base64("!!").is_err());
    }

    const MEASURED_CU_WITH_32_BYTE_MEMO: u32 = 13_180;
    const CDP_MAINNET_SETTLED_LIMIT: u32 = 60_000;
    const CDP_MAINNET_SETTLED_PRICE: u64 = 100_000;

    #[test]
    fn compute_budget_covers_a_32_byte_memo_within_what_cdp_settles() {
        const _: () = assert!(
            DEFAULT_COMPUTE_UNIT_LIMIT
                >= MEASURED_CU_WITH_32_BYTE_MEMO + MEASURED_CU_WITH_32_BYTE_MEMO / 2,
            "DEFAULT_COMPUTE_UNIT_LIMIT must stay >= 1.5 x measured usage with a 32-byte memo"
        );
        const _: () = assert!(DEFAULT_COMPUTE_UNIT_LIMIT <= CDP_MAINNET_SETTLED_LIMIT);
        const _: () = assert!(DEFAULT_COMPUTE_UNIT_PRICE <= CDP_MAINNET_SETTLED_PRICE);
        let built = TopupTx::build(&params(addr(), addr())).unwrap();
        let ix = &built.tx.message.instructions()[0];
        assert_eq!(
            u32::from_le_bytes(ix.data[1..5].try_into().unwrap()),
            DEFAULT_COMPUTE_UNIT_LIMIT
        );
    }

    #[test]
    fn build_rejects_bad_inputs() {
        let payer = addr();
        let mut p = params(addr(), payer);
        p.amount = 0;
        assert!(TopupTx::build(&p).is_err());
        let mut p = params(addr(), payer);
        p.memo = "x".repeat(257);
        assert!(TopupTx::build(&p).is_err());
        let mut p = params(addr(), payer);
        p.memo = "x".repeat(256);
        assert!(TopupTx::build(&p).is_ok());
        let mut p = params(addr(), payer);
        p.memo = String::new();
        assert!(TopupTx::build(&p).is_err());
        let p = params(payer, payer);
        assert!(TopupTx::build(&p).is_err());
    }

    struct Submission {
        signed: TopupTx,
        payer: Keypair,
        message: Vec<u8>,
    }

    fn submission() -> Submission {
        let payer = Keypair::new();
        let mut signed = TopupTx::build(&params(addr(), payer.pubkey())).unwrap();
        signed.sign_with_payer(&payer).unwrap();
        let message = signed.message_bytes().unwrap();
        Submission {
            signed,
            payer,
            message,
        }
    }

    fn assert_rejected(result: Result<TopupTx, X402Error>, reason: SubmissionReject) {
        match result {
            Err(X402Error::InvalidSubmission(got)) => assert_eq!(got, reason),
            other => panic!("expected {reason:?}, got {other:?}"),
        }
    }

    #[test]
    fn submission_with_the_stored_message_and_payer_signature_is_accepted() {
        let s = submission();
        let b64 = s.signed.to_base64().unwrap();
        let accepted = TopupTx::from_submission(&b64, &s.message, &s.payer.pubkey()).unwrap();
        assert_eq!(accepted, s.signed);
    }

    #[test]
    fn submission_rejects_undecodable_input() {
        let s = submission();
        assert_rejected(
            TopupTx::from_submission("!!", &s.message, &s.payer.pubkey()),
            SubmissionReject::Base64,
        );
        assert_rejected(
            TopupTx::from_submission(&B64.encode([1u8, 2, 3]), &s.message, &s.payer.pubkey()),
            SubmissionReject::Bincode,
        );
    }

    #[test]
    fn submission_rejects_trailing_bytes_that_the_general_decoder_tolerates() {
        let s = submission();
        let mut raw = s.signed.to_bytes().unwrap();
        raw.extend_from_slice(&[0, 0, 0]);
        let padded = B64.encode(&raw);
        assert_eq!(TopupTx::from_base64(&padded).unwrap(), s.signed);
        assert_rejected(
            TopupTx::from_submission(&padded, &s.message, &s.payer.pubkey()),
            SubmissionReject::NonCanonical,
        );
    }

    #[test]
    fn submission_rejects_surrounding_whitespace() {
        let s = submission();
        let padded = format!(" {}\n", s.signed.to_base64().unwrap());
        assert_eq!(TopupTx::from_base64(&padded).unwrap(), s.signed);
        assert_rejected(
            TopupTx::from_submission(&padded, &s.message, &s.payer.pubkey()),
            SubmissionReject::Base64,
        );
    }

    #[test]
    fn submission_rejects_a_signature_count_the_message_does_not_declare() {
        let mut s = submission();
        s.signed.tx.signatures.push(Signature::default());
        let b64 = s.signed.to_base64().unwrap();
        assert_rejected(
            TopupTx::from_submission(&b64, &s.message, &s.payer.pubkey()),
            SubmissionReject::Sanitize,
        );
    }

    #[test]
    fn submission_rejects_another_message_whether_or_not_it_was_re_signed() {
        let s = submission();
        let mut tampered = TopupTx::build(&params(addr(), s.payer.pubkey())).unwrap();
        let carried_over = s.signed.tx.signatures.clone();
        tampered.tx.signatures = carried_over;
        assert_rejected(
            TopupTx::from_submission(
                &tampered.to_base64().unwrap(),
                &s.message,
                &s.payer.pubkey(),
            ),
            SubmissionReject::MessageMismatch,
        );
        tampered.sign_with_payer(&s.payer).unwrap();
        assert_rejected(
            TopupTx::from_submission(
                &tampered.to_base64().unwrap(),
                &s.message,
                &s.payer.pubkey(),
            ),
            SubmissionReject::MessageMismatch,
        );
    }

    #[test]
    fn submission_rejects_a_filled_fee_payer_slot() {
        let mut s = submission();
        s.signed.tx.signatures[0] = Signature::from([9u8; 64]);
        let b64 = s.signed.to_base64().unwrap();
        assert_rejected(
            TopupTx::from_submission(&b64, &s.message, &s.payer.pubkey()),
            SubmissionReject::FeePayerSigned,
        );
    }

    #[test]
    fn submission_rejects_a_missing_or_foreign_payer_signature() {
        let mut s = submission();
        s.signed.tx.signatures[1] = Signature::default();
        assert_rejected(
            TopupTx::from_submission(
                &s.signed.to_base64().unwrap(),
                &s.message,
                &s.payer.pubkey(),
            ),
            SubmissionReject::PayerSignature,
        );
        s.signed.tx.signatures[1] = Keypair::new().sign_message(&s.message);
        assert_rejected(
            TopupTx::from_submission(
                &s.signed.to_base64().unwrap(),
                &s.message,
                &s.payer.pubkey(),
            ),
            SubmissionReject::PayerSignature,
        );
        let s = submission();
        assert_rejected(
            TopupTx::from_submission(
                &s.signed.to_base64().unwrap(),
                &s.message,
                &Keypair::new().pubkey(),
            ),
            SubmissionReject::PayerSignature,
        );
    }

    fn crafted(required_signatures: u8, signatures: Vec<Signature>) -> Vec<u8> {
        let payer = Keypair::new();
        let mut tx = TopupTx::build(&params(addr(), payer.pubkey())).unwrap().tx;
        if let VersionedMessage::V0(message) = &mut tx.message {
            message.header.num_required_signatures = required_signatures;
        }
        tx.signatures = signatures;
        bincode::serialize(&tx).unwrap()
    }

    #[test]
    fn non_signer_static_key_has_no_signer_index() {
        let built = TopupTx::build(&params(addr(), addr())).unwrap();
        let non_signer = built.static_keys()[2];
        assert_eq!(built.signer_index(&non_signer), None);
    }

    #[test]
    fn signer_lookups_do_not_panic_on_inconsistent_headers() {
        let stranger = Keypair::new();
        let oversized = TopupTx::from_bytes(&crafted(200, vec![])).unwrap();
        assert_eq!(oversized.signer_index(&stranger.pubkey()), None);
        let mut oversized_signing = oversized.clone();
        assert!(oversized_signing.sign_with_payer(&stranger).is_err());
        assert_rejected(
            TopupTx::from_submission(&oversized.to_base64().unwrap(), &[], &stranger.pubkey()),
            SubmissionReject::Sanitize,
        );

        let payer = Keypair::new();
        let mut unsigned = TopupTx::build(&params(addr(), payer.pubkey())).unwrap();
        unsigned.tx.signatures = vec![];
        let bytes = bincode::serialize(&unsigned.tx).unwrap();
        let mut empty = TopupTx::from_bytes(&bytes).unwrap();
        assert_eq!(empty.signer_index(&payer.pubkey()), Some(1));
        assert!(matches!(
            empty.sign_with_payer(&payer),
            Err(X402Error::Build(_))
        ));
        assert_rejected(
            TopupTx::from_submission(&empty.to_base64().unwrap(), &[], &payer.pubkey()),
            SubmissionReject::Sanitize,
        );
    }
}

pub const SYSTEM_PROGRAM: Address = Address::from_str_const("11111111111111111111111111111111");

pub fn create_associated_token_account_idempotent(
    funder: &Address,
    owner: &Address,
    mint: &Address,
) -> Instruction {
    let ata = associated_token_address(owner, mint);
    Instruction::new_with_bytes(
        ASSOCIATED_TOKEN_PROGRAM,
        &[1u8],
        vec![
            AccountMeta::new(*funder, true),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM, false),
        ],
    )
}

pub fn sign_single(
    signer: &Keypair,
    instructions: &[Instruction],
    blockhash: Hash,
) -> Result<String, X402Error> {
    let message = v0::Message::try_compile(&signer.pubkey(), instructions, &[], blockhash)
        .map_err(|e| X402Error::Build(format!("compile: {e}")))?;
    let message = VersionedMessage::V0(message);
    let bytes = bincode::serialize(&message).map_err(|e| X402Error::Build(e.to_string()))?;
    let tx = VersionedTransaction {
        signatures: vec![signer.sign_message(&bytes)],
        message,
    };
    Ok(B64.encode(bincode::serialize(&tx).map_err(|e| X402Error::Build(e.to_string()))?))
}

#[cfg(test)]
mod setup_tests {
    use super::*;

    #[test]
    fn create_ata_idempotent_layout() {
        let funder = Keypair::new().pubkey();
        let owner = Keypair::new().pubkey();
        let mint = Address::from_str_const("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU");
        let ix = create_associated_token_account_idempotent(&funder, &owner, &mint);
        assert_eq!(ix.program_id, ASSOCIATED_TOKEN_PROGRAM);
        assert_eq!(ix.data, vec![1u8]);
        assert_eq!(ix.accounts.len(), 6);
        assert!(ix.accounts[0].is_signer && ix.accounts[0].is_writable);
        assert_eq!(
            ix.accounts[1].pubkey,
            associated_token_address(&owner, &mint)
        );
        assert!(ix.accounts[1].is_writable && !ix.accounts[1].is_signer);
        assert_eq!(ix.accounts[4].pubkey, SYSTEM_PROGRAM);
        assert_eq!(ix.accounts[5].pubkey, TOKEN_PROGRAM);
    }

    #[test]
    fn sign_single_produces_one_verifying_signature() {
        let kp = Keypair::new();
        let ix = memo("setup");
        let b64 = sign_single(&kp, &[ix], Hash::new_from_array([1u8; 32])).unwrap();
        let tx = TopupTx::from_base64(&b64).unwrap();
        assert_eq!(tx.num_required_signatures(), 1);
        assert_eq!(tx.static_keys()[0], kp.pubkey());
        let msg = tx.message_bytes().unwrap();
        assert!(tx.tx.signatures[0].verify(kp.pubkey().as_ref(), &msg));
    }
}

#[cfg(test)]
mod golden_tests {
    use serde_json::Value;

    use super::*;
    use crate::network::Network;

    fn settled_result() -> Value {
        serde_json::from_str(include_str!("../testdata/settled_transaction.json")).unwrap()
    }

    fn settled_message() -> Vec<u8> {
        let b64 = settled_result()["transaction"][0]
            .as_str()
            .unwrap()
            .to_string();
        TopupTx::from_base64(&b64).unwrap().message_bytes().unwrap()
    }

    #[test]
    fn captured_settlement_rebuilds_byte_for_byte_from_its_inputs() {
        let b64 = settled_result()["transaction"][0]
            .as_str()
            .unwrap()
            .to_string();
        let landed = TopupTx::from_base64(&b64).unwrap();
        assert_eq!(landed.to_base64().unwrap(), b64);
        let rebuilt = TopupTx::build(&BuildParams {
            fee_payer: Address::from_str_const("BFK9TLC3edb13K6v4YyH3DwPb5DSUpkWvb7XnqCL9b4F"),
            payer: Address::from_str_const("7GY76sXRxXMebLFvD2KX8NS1mVSGw3Zs9PSPh4qgyGef"),
            pay_to: Address::from_str_const("G7t2qpRF4kKZ1AqQZyT3ZVjERb2njQQY7JxHE2yBUGL7"),
            mint: Address::from_str_const(Network::DEVNET_USDC),
            decimals: 6,
            amount: 10_000,
            memo: "data_100:7GY76sXRxXMebLFvD2KX8NS1mVSGw3Zs9PSPh4qgyGef:331469ddc68c4665a5df13b97b2f437b"
                .into(),
            blockhash: *landed.tx.message.recent_blockhash(),
            compute_unit_limit: 200_000,
            compute_unit_price: 1,
        })
        .unwrap();
        assert_eq!(rebuilt.message_bytes().unwrap(), settled_message());
    }
}
