use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    Data,
    Compute,
    Networking,
}

impl TokenKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            TokenKind::Data => "data",
            TokenKind::Compute => "compute",
            TokenKind::Networking => "networking",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleEntry {
    pub sku: String,
    pub kind: TokenKind,
    pub units: i64,
    pub price_atomic: u64,
    pub price_usdc: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub bundles: Vec<BundleEntry>,
}

impl Catalog {
    pub fn bundle(&self, sku: &str) -> Option<&BundleEntry> {
        self.bundles.iter().find(|b| b.sku == sku)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptState {
    Open,
    Expired,
    Settling,
    Settled,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptFailure {
    VerifyRejected,
    NotLanded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopupReceipt {
    pub memo: String,
    pub sku: String,
    pub kind: TokenKind,
    pub units: i64,
    pub amount: u64,
    pub asset: String,
    pub network: String,
    pub payer: String,
    pub state: ReceiptState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ReceiptFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled_at: Option<DateTime<Utc>>,
}

impl TopupReceipt {
    pub fn is_final(&self) -> bool {
        match self.state {
            ReceiptState::Expired | ReceiptState::Settled => true,
            ReceiptState::Failed => self.failure != Some(ReceiptFailure::NotLanded),
            ReceiptState::Open | ReceiptState::Settling => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopupReply {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<TopupReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn assert_wire<T>(value: &T, expected: Value)
    where
        T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let encoded = serde_json::to_value(value).unwrap();
        assert_eq!(encoded, expected);
        let decoded: T = serde_json::from_value(expected).unwrap();
        assert_eq!(&decoded, value);
    }

    fn settled_receipt() -> TopupReceipt {
        TopupReceipt {
            memo: "m1".into(),
            sku: "data_100".into(),
            kind: TokenKind::Data,
            units: 100,
            amount: 1_000_000,
            asset: "mint".into(),
            network: "solana:net".into(),
            payer: "payer".into(),
            state: ReceiptState::Settled,
            signature: Some("sig".into()),
            pending_signature: None,
            failure: None,
            error_reason: None,
            created_at: at("2026-11-03T10:00:00Z"),
            expires_at: at("2026-11-03T10:00:25Z"),
            settled_at: Some(at("2026-11-03T10:00:05Z")),
        }
    }

    #[test]
    fn bundle_entry_wire() {
        assert_wire(
            &BundleEntry {
                sku: "data_100".into(),
                kind: TokenKind::Data,
                units: 100,
                price_atomic: 1_000_000,
                price_usdc: "1.00".into(),
            },
            json!({
                "sku": "data_100",
                "kind": "data",
                "units": 100,
                "price_atomic": 1_000_000,
                "price_usdc": "1.00"
            }),
        );
    }

    #[test]
    fn catalog_reads_only_bundles_and_finds_them_by_sku() {
        let catalog: Catalog = serde_json::from_value(json!({
            "tokens_per_usd": 100,
            "bundles": [{
                "sku": "compute_1000",
                "kind": "compute",
                "units": 1000,
                "price_atomic": 10_000_000,
                "price_usdc": "10.0"
            }],
            "plans": [],
            "packs": []
        }))
        .unwrap();
        assert_eq!(
            catalog.bundle("compute_1000").unwrap().price_atomic,
            10_000_000
        );
        assert!(catalog.bundle("data_100").is_none());
    }

    #[test]
    fn settled_reply_wire() {
        assert_wire(
            &TopupReply {
                code: "settled".into(),
                error: None,
                receipt: Some(settled_receipt()),
                balance: Some(1100),
            },
            json!({
                "code": "settled",
                "receipt": {
                    "memo": "m1",
                    "sku": "data_100",
                    "kind": "data",
                    "units": 100,
                    "amount": 1_000_000,
                    "asset": "mint",
                    "network": "solana:net",
                    "payer": "payer",
                    "state": "settled",
                    "signature": "sig",
                    "created_at": "2026-11-03T10:00:00Z",
                    "expires_at": "2026-11-03T10:00:25Z",
                    "settled_at": "2026-11-03T10:00:05Z"
                },
                "balance": 1100
            }),
        );
    }

    #[test]
    fn failed_receipt_wire() {
        let receipt = TopupReceipt {
            state: ReceiptState::Failed,
            signature: None,
            pending_signature: Some("pending".into()),
            failure: Some(ReceiptFailure::VerifyRejected),
            error_reason: Some("insufficient_funds".into()),
            settled_at: None,
            ..settled_receipt()
        };
        let encoded = serde_json::to_value(&receipt).unwrap();
        assert_eq!(encoded["state"], json!("failed"));
        assert_eq!(encoded["failure"], json!("verify_rejected"));
        assert_eq!(encoded["pending_signature"], json!("pending"));
        assert_eq!(encoded["error_reason"], json!("insufficient_funds"));
        assert!(encoded.get("signature").is_none());
        assert!(encoded.get("settled_at").is_none());
        let decoded: TopupReceipt = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, receipt);
        assert_eq!(
            serde_json::to_value(ReceiptFailure::NotLanded).unwrap(),
            json!("not_landed")
        );
    }

    #[test]
    fn bare_reply_wire() {
        assert_wire(
            &TopupReply {
                code: "memo_not_found".into(),
                error: None,
                receipt: None,
                balance: None,
            },
            json!({"code": "memo_not_found"}),
        );
        assert_wire(
            &TopupReply {
                code: "topup_unavailable".into(),
                error: Some("try again".into()),
                receipt: None,
                balance: None,
            },
            json!({"code": "topup_unavailable", "error": "try again"}),
        );
    }

    #[test]
    fn receipt_state_and_token_kind_wire_values() {
        let cases = [
            (ReceiptState::Open, "open"),
            (ReceiptState::Expired, "expired"),
            (ReceiptState::Settling, "settling"),
            (ReceiptState::Settled, "settled"),
            (ReceiptState::Failed, "failed"),
        ];
        for (state, wire) in cases {
            assert_eq!(serde_json::to_value(state).unwrap(), json!(wire));
            let decoded: ReceiptState = serde_json::from_value(json!(wire)).unwrap();
            assert_eq!(decoded, state);
        }
        for kind in [TokenKind::Data, TokenKind::Compute, TokenKind::Networking] {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
        }
    }

    #[test]
    fn not_landed_failure_is_not_final_because_it_can_still_settle() {
        let with = |state, failure| TopupReceipt {
            state,
            failure,
            ..settled_receipt()
        };
        assert!(with(ReceiptState::Settled, None).is_final());
        assert!(with(ReceiptState::Expired, None).is_final());
        assert!(with(ReceiptState::Failed, Some(ReceiptFailure::VerifyRejected)).is_final());
        assert!(!with(ReceiptState::Failed, Some(ReceiptFailure::NotLanded)).is_final());
        assert!(!with(ReceiptState::Settling, None).is_final());
        assert!(!with(ReceiptState::Open, None).is_final());
    }
}
