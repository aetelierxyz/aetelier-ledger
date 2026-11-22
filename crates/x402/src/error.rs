use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmissionReject {
    Base64,
    Bincode,
    NonCanonical,
    Sanitize,
    MessageMismatch,
    FeePayerSigned,
    PayerSignature,
}

#[derive(Debug, Error)]
pub enum X402Error {
    #[error("configuration: {0}")]
    Config(String),
    #[error("transport: {0}")]
    Transport(String),
    #[error("facilitator returned HTTP {status}: {body}")]
    FacilitatorStatus { status: u16, body: String },
    #[error("rpc error: {0}")]
    Rpc(String),
    #[error("decode: {0}")]
    Decode(String),
    #[error("transaction build: {0}")]
    Build(String),
    #[error("network mismatch: expected `{expected}`, got `{got}`")]
    NetworkMismatch { expected: String, got: String },
    #[error("settlement not confirmed: {0}")]
    SettleUnconfirmed(String),
    #[error("invalid submission: {0:?}")]
    InvalidSubmission(SubmissionReject),
    #[error("unsupported transaction version: {0}")]
    UnsupportedTxVersion(String),
    #[error("landing scan limit: {0}")]
    LandingScanLimit(String),
}

impl From<reqwest::Error> for X402Error {
    fn from(e: reqwest::Error) -> Self {
        X402Error::Transport(e.without_url().to_string())
    }
}

impl From<serde_json::Error> for X402Error {
    fn from(e: serde_json::Error) -> Self {
        X402Error::Decode(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn transport_errors_do_not_carry_the_request_url() {
        let err = reqwest::Client::new()
            .get("http://127.0.0.1:9/?api-marker=abc123")
            .timeout(std::time::Duration::from_nanos(1))
            .send()
            .await
            .unwrap_err();
        assert!(err.to_string().contains("abc123"));
        assert!(!X402Error::from(err).to_string().contains("abc123"));
    }
}
