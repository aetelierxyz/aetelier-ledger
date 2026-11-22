use std::time::Duration;

use reqwest::Url;
use serde_json::{Value, json};
use solana_address::Address;

use crate::error::X402Error;
use crate::http::{client_endpoint, http_client};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
const UNSUPPORTED_TX_VERSION_CODE: i64 = -32015;

#[derive(Clone)]
pub struct SolanaRpc {
    http: reqwest::Client,
    url: Url,
}

impl std::fmt::Debug for SolanaRpc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaRpc")
            .field("url", &"<redacted>")
            .finish()
    }
}

pub(crate) fn interpret_rpc(method: &str, status: u16, body: &str) -> Result<Value, X402Error> {
    let parsed = serde_json::from_str::<Value>(body);
    if let Some(err) = parsed.as_ref().ok().and_then(|v| v.get("error")) {
        if err["code"].as_i64() == Some(UNSUPPORTED_TX_VERSION_CODE) {
            return Err(X402Error::UnsupportedTxVersion(format!("{method}: {err}")));
        }
        return Err(X402Error::Rpc(format!("{method}: {err}")));
    }
    if !(200..300).contains(&status) {
        return Err(X402Error::Rpc(format!("{method}: HTTP {status}: {body}")));
    }
    let value = parsed.map_err(|e| X402Error::Rpc(format!("{method}: bad json: {e}")))?;
    value
        .get("result")
        .cloned()
        .ok_or_else(|| X402Error::Rpc(format!("{method}: no result")))
}

impl SolanaRpc {
    pub fn new(url: &str) -> Result<Self, X402Error> {
        Ok(Self {
            http: http_client(DEFAULT_TIMEOUT)?,
            url: client_endpoint("rpc url", url)?,
        })
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, X402Error> {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let resp = self.http.post(self.url.clone()).json(&body).send().await?;
        let status = resp.status().as_u16();
        let text = resp.text().await?;
        interpret_rpc(method, status, &text)
    }

    pub async fn token_balance(&self, token_account: &Address) -> Result<Option<u64>, X402Error> {
        match self
            .call(
                "getTokenAccountBalance",
                json!([token_account.to_string(), {"commitment": "confirmed"}]),
            )
            .await
        {
            Ok(r) => {
                let amount = r["value"]["amount"]
                    .as_str()
                    .ok_or_else(|| X402Error::Rpc("getTokenAccountBalance: no amount".into()))?;
                amount
                    .parse::<u64>()
                    .map(Some)
                    .map_err(|e| X402Error::Rpc(format!("amount `{amount}`: {e}")))
            }
            Err(X402Error::Rpc(msg)) if msg.contains("could not find account") => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpret_rpc_fails_closed() {
        assert_eq!(
            interpret_rpc("m", 200, r#"{"result":null}"#).unwrap(),
            Value::Null
        );
        for status in [200u16, 201, 299] {
            assert_eq!(
                interpret_rpc("m", status, r#"{"result":5}"#).unwrap(),
                json!(5)
            );
        }
        assert!(matches!(
            interpret_rpc("m", 200, r#"{"jsonrpc":"2.0"}"#),
            Err(X402Error::Rpc(_))
        ));
        assert!(matches!(
            interpret_rpc("m", 200, "not json"),
            Err(X402Error::Rpc(_))
        ));
        for status in [300u16, 429, 500] {
            assert!(matches!(
                interpret_rpc("m", status, r#"{"result":5}"#),
                Err(X402Error::Rpc(_))
            ));
        }
        assert!(matches!(
            interpret_rpc(
                "m",
                200,
                r#"{"error":{"code":-32016,"message":"Minimum context slot has not been reached"}}"#
            ),
            Err(X402Error::Rpc(_))
        ));
    }

    #[test]
    fn error_beside_a_result_is_an_error() {
        let body = r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"could not find account"},"result":5,"id":1}"#;
        assert!(matches!(
            interpret_rpc("m", 200, body),
            Err(X402Error::Rpc(msg)) if msg.contains("could not find account")
        ));
    }

    #[test]
    fn interpret_rpc_types_the_unsupported_version_error() {
        let body = r#"{"jsonrpc":"2.0","error":{"code":-32015,"message":"Transaction version (1) is not supported by the requesting client."},"id":1}"#;
        assert!(matches!(
            interpret_rpc("getTransaction", 200, body),
            Err(X402Error::UnsupportedTxVersion(_))
        ));
    }

    #[test]
    fn rpc_endpoint_rule_and_redacted_debug() {
        assert!(SolanaRpc::new("http://rpc.example").is_err());
        assert!(SolanaRpc::new("http://127.0.0.1:8899").is_ok());
        let rpc = SolanaRpc::new("https://rpc.example/?api-marker=abc123").unwrap();
        assert!(!format!("{rpc:?}").contains("abc123"));
        assert!(format!("{:?}", rpc.http).contains("Policy(None)"));
    }
}
