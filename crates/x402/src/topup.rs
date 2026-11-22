use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::PaymentRequirements;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareBody {
    pub payer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareReply {
    pub code: String,
    pub memo: String,
    pub transaction: String,
    pub expires_at: DateTime<Utc>,
    pub expires_in_ms: i64,
    pub last_valid_block_height: u64,
    pub requirements: PaymentRequirements,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    #[test]
    fn prepare_body_wire() {
        let body = PrepareBody {
            payer: "payer".into(),
        };
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            json!({"payer": "payer"})
        );
        let decoded: PrepareBody = serde_json::from_value(json!({"payer": "payer"})).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn prepare_reply_wire() {
        let mut extra = Map::new();
        extra.insert("feePayer".into(), json!("fee"));
        extra.insert("memo".into(), json!("m1"));
        let reply = PrepareReply {
            code: "payment_required".into(),
            memo: "m1".into(),
            transaction: "dHg=".into(),
            expires_at: DateTime::parse_from_rfc3339("2026-11-03T10:00:25Z")
                .unwrap()
                .with_timezone(&Utc),
            expires_in_ms: 24_000,
            last_valid_block_height: 42,
            requirements: PaymentRequirements {
                scheme: "exact".into(),
                network: "solana:net".into(),
                amount: "1000000".into(),
                asset: "mint".into(),
                pay_to: "treasury".into(),
                max_timeout_seconds: 25,
                extra,
            },
        };
        let expected = json!({
            "code": "payment_required",
            "memo": "m1",
            "transaction": "dHg=",
            "expires_at": "2026-11-03T10:00:25Z",
            "expires_in_ms": 24_000,
            "last_valid_block_height": 42,
            "requirements": {
                "scheme": "exact",
                "network": "solana:net",
                "amount": "1000000",
                "asset": "mint",
                "payTo": "treasury",
                "maxTimeoutSeconds": 25,
                "extra": {"feePayer": "fee", "memo": "m1"}
            }
        });
        assert_eq!(serde_json::to_value(&reply).unwrap(), expected);
        let decoded: PrepareReply = serde_json::from_value(expected).unwrap();
        assert_eq!(decoded, reply);
        assert_eq!(decoded.requirements.fee_payer(), Some("fee"));
        assert_eq!(decoded.requirements.memo(), Some("m1"));
    }
}
