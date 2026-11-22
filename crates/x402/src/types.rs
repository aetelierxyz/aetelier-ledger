use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use solana_signature::Signature;

use crate::error::X402Error;
use crate::network::Network;

pub const X402_VERSION: u8 = 2;

pub const HEADER_PAYMENT_REQUIRED: &str = "PAYMENT-REQUIRED";
pub const HEADER_PAYMENT_SIGNATURE: &str = "PAYMENT-SIGNATURE";
pub const HEADER_PAYMENT_RESPONSE: &str = "PAYMENT-RESPONSE";

pub const SCHEME_EXACT: &str = "exact";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInfo {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    pub scheme: String,
    pub network: String,
    pub amount: String,
    pub asset: String,
    pub pay_to: String,
    pub max_timeout_seconds: u64,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl PaymentRequirements {
    pub fn fee_payer(&self) -> Option<&str> {
        self.extra.get("feePayer").and_then(Value::as_str)
    }

    pub fn memo(&self) -> Option<&str> {
        self.extra.get("memo").and_then(Value::as_str)
    }

    pub fn amount_u64(&self) -> Result<u64, X402Error> {
        self.amount
            .parse::<u64>()
            .map_err(|e| X402Error::Decode(format!("amount `{}`: {e}", self.amount)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequired {
    pub x402_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub resource: ResourceInfo,
    pub accepts: Vec<PaymentRequirements>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub extensions: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SvmPayload {
    pub transaction: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    pub x402_version: u8,
    pub accepted: PaymentRequirements,
    pub payload: SvmPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceInfo>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub extensions: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FacilitatorRequest {
    pub x402_version: u8,
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    pub is_valid: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
    #[serde(default)]
    pub transaction: String,
    #[serde(default)]
    pub network: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<Value>,
}

impl SettleResponse {
    pub const PENDING: &'static str = "settlement_pending";

    pub fn is_pending(&self) -> bool {
        self.error_reason.as_deref() == Some(Self::PENDING)
    }

    pub fn settled_signature(&self, network: Network) -> Result<Signature, X402Error> {
        if !self.success {
            return Err(X402Error::SettleUnconfirmed(format!(
                "success is false: {}",
                self.error_reason.as_deref().unwrap_or("no reason given")
            )));
        }
        if self.network != network.caip2() {
            return Err(X402Error::NetworkMismatch {
                expected: network.caip2().to_string(),
                got: self.network.clone(),
            });
        }
        if self.transaction.is_empty() {
            return Err(X402Error::SettleUnconfirmed("transaction is empty".into()));
        }
        let signature = Signature::from_str(&self.transaction)
            .map_err(|_| X402Error::SettleUnconfirmed("transaction is not a signature".into()))?;
        if signature == Signature::default() {
            return Err(X402Error::SettleUnconfirmed(
                "transaction is the default signature".into(),
            ));
        }
        Ok(signature)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedKind {
    pub x402_version: u8,
    pub scheme: String,
    pub network: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl SupportedKind {
    pub fn fee_payer(&self) -> Option<&str> {
        self.extra.get("feePayer").and_then(Value::as_str)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedResponse {
    pub kinds: Vec<SupportedKind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<Value>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub signers: Map<String, Value>,
}

impl SupportedResponse {
    pub fn exact_kind(&self, network: &str) -> Option<&SupportedKind> {
        self.kinds.iter().find(|k| {
            k.x402_version == X402_VERSION && k.scheme == SCHEME_EXACT && k.network == network
        })
    }
}

pub fn encode_header<T: Serialize>(value: &T) -> Result<String, X402Error> {
    Ok(B64.encode(serde_json::to_vec(value)?))
}

pub fn decode_header<T: for<'de> Deserialize<'de>>(value: &str) -> Result<T, X402Error> {
    let raw = B64
        .decode(value.trim())
        .map_err(|e| X402Error::Decode(format!("header base64: {e}")))?;
    Ok(serde_json::from_slice(&raw)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> PaymentRequirements {
        let mut extra = Map::new();
        extra.insert("feePayer".into(), Value::String("FeePayer111".into()));
        extra.insert("memo".into(), Value::String("data_100:u1:abcd".into()));
        PaymentRequirements {
            scheme: SCHEME_EXACT.into(),
            network: "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1".into(),
            amount: "1000000".into(),
            asset: "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU".into(),
            pay_to: "PayTo111".into(),
            max_timeout_seconds: 60,
            extra,
        }
    }

    #[test]
    fn requirements_serialize_with_spec_field_names() {
        let v = serde_json::to_value(req()).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        for k in [
            "scheme",
            "network",
            "amount",
            "asset",
            "payTo",
            "maxTimeoutSeconds",
            "extra",
        ] {
            assert!(keys.contains(&k), "missing `{k}` in {keys:?}");
        }
        assert_eq!(v["extra"]["feePayer"], "FeePayer111");
        assert_eq!(req().fee_payer(), Some("FeePayer111"));
        assert_eq!(req().memo(), Some("data_100:u1:abcd"));
        assert_eq!(req().amount_u64().unwrap(), 1_000_000);
    }

    #[test]
    fn payment_required_header_round_trips() {
        let pr = PaymentRequired {
            x402_version: X402_VERSION,
            error: None,
            resource: ResourceInfo {
                url: "https://aetelier.xyz/api/payments/topup/data_100".into(),
                description: Some("100 data units".into()),
                mime_type: Some("application/json".into()),
            },
            accepts: vec![req()],
            extensions: Map::new(),
        };
        let header = encode_header(&pr).unwrap();
        let back: PaymentRequired = decode_header(&header).unwrap();
        assert_eq!(back, pr);
        let json = serde_json::to_value(&pr).unwrap();
        assert_eq!(json["x402Version"], 2);
        assert!(json.get("error").is_none(), "None error must be omitted");
        assert_eq!(json["resource"]["mimeType"], "application/json");
    }

    #[test]
    fn payment_payload_carries_accepted_and_transaction() {
        let pp = PaymentPayload {
            x402_version: X402_VERSION,
            accepted: req(),
            payload: SvmPayload {
                transaction: "AQID".into(),
            },
            resource: None,
            extensions: Map::new(),
        };
        let json = serde_json::to_value(&pp).unwrap();
        assert_eq!(json["x402Version"], 2);
        assert_eq!(json["accepted"]["payTo"], "PayTo111");
        assert_eq!(json["payload"]["transaction"], "AQID");
        assert!(json.get("scheme").is_none(), "v2 removed top-level scheme");
        assert!(
            json.get("network").is_none(),
            "v2 removed top-level network"
        );
        let back: PaymentPayload = decode_header(&encode_header(&pp).unwrap()).unwrap();
        assert_eq!(back, pp);
    }

    fn confirmed_settle() -> (SettleResponse, Signature) {
        let signature = Signature::from([7u8; 64]);
        let response = SettleResponse {
            success: true,
            transaction: signature.to_string(),
            network: Network::DEVNET_CAIP2.into(),
            ..Default::default()
        };
        (response, signature)
    }

    fn unconfirmed_message(response: &SettleResponse) -> String {
        match response.settled_signature(Network::SolanaDevnet) {
            Err(X402Error::SettleUnconfirmed(message)) => message,
            other => panic!("expected an unconfirmed settlement, got {other:?}"),
        }
    }

    #[test]
    fn settled_signature_returns_the_exact_signature_on_success() {
        let (response, signature) = confirmed_settle();
        assert_eq!(
            response.settled_signature(Network::SolanaDevnet).unwrap(),
            signature
        );
    }

    #[test]
    fn settled_signature_rejects_another_network() {
        let (response, _) = confirmed_settle();
        match response.settled_signature(Network::SolanaMainnet) {
            Err(X402Error::NetworkMismatch { expected, got }) => {
                assert_eq!(expected, Network::MAINNET_CAIP2);
                assert_eq!(got, Network::DEVNET_CAIP2);
            }
            other => panic!("expected a network mismatch, got {other:?}"),
        }
    }

    #[test]
    fn settled_signature_rejects_an_empty_network() {
        let (mut response, _) = confirmed_settle();
        response.network = String::new();
        assert!(matches!(
            response.settled_signature(Network::SolanaDevnet),
            Err(X402Error::NetworkMismatch { .. })
        ));
    }

    #[test]
    fn settled_signature_treats_unsuccessful_responses_as_unconfirmed() {
        let (mut response, _) = confirmed_settle();
        response.success = false;
        response.error_reason = Some(SettleResponse::PENDING.into());
        assert!(unconfirmed_message(&response).contains(SettleResponse::PENDING));

        response.network = String::new();
        assert!(unconfirmed_message(&response).contains(SettleResponse::PENDING));
    }

    #[test]
    fn settled_signature_rejects_unusable_transaction_values() {
        let (mut response, _) = confirmed_settle();
        response.transaction = String::new();
        assert!(unconfirmed_message(&response).contains("empty"));

        response.transaction = "5Sig".into();
        assert!(unconfirmed_message(&response).contains("not a signature"));

        response.transaction = Signature::default().to_string();
        assert!(unconfirmed_message(&response).contains("default"));
    }

    #[test]
    fn exact_kind_ignores_other_schemes() {
        let supported: SupportedResponse = serde_json::from_str(
            r#"{"kinds":[{"x402Version":2,"scheme":"upto","network":"solana:x"}]}"#,
        )
        .unwrap();
        assert!(supported.exact_kind("solana:x").is_none());
    }

    #[test]
    fn facilitator_responses_parse_cdp_shapes() {
        let v: VerifyResponse = serde_json::from_str(
            r#"{"isValid":false,"invalidReason":"insufficient_funds","invalidMessage":"x","payer":"P"}"#,
        )
        .unwrap();
        assert!(!v.is_valid);
        assert_eq!(v.invalid_reason.as_deref(), Some("insufficient_funds"));

        let s: SettleResponse = serde_json::from_str(
            r#"{"success":true,"transaction":"5Sig","network":"solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1","payer":"P","amount":"1000000"}"#,
        )
        .unwrap();
        assert!(s.success && !s.is_pending());
        assert_eq!(s.transaction, "5Sig");

        let p: SettleResponse = serde_json::from_str(
            r#"{"success":false,"errorReason":"settlement_pending","transaction":"5Sig","network":"solana:x"}"#,
        )
        .unwrap();
        assert!(p.is_pending());

        let sup: SupportedResponse = serde_json::from_str(
            r#"{"kinds":[{"x402Version":1,"scheme":"exact","network":"solana-devnet"},
                        {"x402Version":2,"scheme":"exact","network":"solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1","extra":{"feePayer":"BENrLoUbndxoNMUS5JXApGMtNykLjFXXixMtpDwDR9SP"}}],
               "extensions":[],"signers":{"solana:*":["BENrLoUbndxoNMUS5JXApGMtNykLjFXXixMtpDwDR9SP"]}}"#,
        )
        .unwrap();
        let kind = sup
            .exact_kind("solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1")
            .expect("v2 devnet kind");
        assert_eq!(
            kind.fee_payer(),
            Some("BENrLoUbndxoNMUS5JXApGMtNykLjFXXixMtpDwDR9SP")
        );
        assert!(
            sup.exact_kind("solana-devnet").is_none(),
            "v1 kind must not match"
        );
    }
}
