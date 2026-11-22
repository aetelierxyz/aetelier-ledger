use std::time::Duration;

use aetelier_ledger_x402::{PrepareBody, PrepareReply};
use reqwest::header::{HeaderMap, RETRY_AFTER};
use reqwest::{RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use crate::error::{CliError, Exit};
use crate::wire::TopupReply;

pub const USER_AGENT: &str = concat!("aetelier-ledger-cli/", env!("CARGO_PKG_VERSION"));
pub const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(5);
const READ_ATTEMPTS: u32 = 3;
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_MESSAGE: usize = 300;

pub struct Api {
    http: reqwest::Client,
    base: Url,
    api_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SubmitAnswer {
    pub status: u16,
    pub reply: Option<TopupReply>,
    pub retry_after: Option<Duration>,
    pub body: Vec<u8>,
}

impl Api {
    pub fn new(base: Url, api_key: Option<String>) -> Result<Self, CliError> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| CliError::usage("http_client", e.to_string()))?;
        Ok(Self {
            http,
            base,
            api_key,
        })
    }

    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    pub fn require_api_key(&self) -> Result<(), CliError> {
        self.key().map(|_| ())
    }

    fn key(&self) -> Result<&str, CliError> {
        self.api_key.as_deref().ok_or_else(|| {
            CliError::usage(
                "missing_api_key",
                "set AETELIER_API_KEY to an API key created under Settings, API keys",
            )
        })
    }

    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty().extend(segments);
        }
        url
    }

    fn authed(&self, request: RequestBuilder) -> Result<RequestBuilder, CliError> {
        Ok(request.bearer_auth(self.key()?))
    }

    async fn read<T: DeserializeOwned>(&self, url: Url, auth: bool) -> Result<T, CliError> {
        let mut attempt = 1;
        loop {
            let mut request = self.http.get(url.clone()).timeout(REQUEST_TIMEOUT);
            if auth {
                request = self.authed(request)?;
            }
            let response = request.send().await.map_err(transport)?;
            let status = response.status();
            let wait = retry_after(response.headers());
            let bytes = response.bytes().await.map_err(transport)?;
            if status.is_success() {
                return decode(&bytes, status);
            }
            if matches!(status.as_u16(), 429 | 503) && attempt < READ_ATTEMPTS {
                attempt += 1;
                tokio::time::sleep(wait.unwrap_or(DEFAULT_RETRY_AFTER).min(MAX_RETRY_AFTER)).await;
                continue;
            }
            return Err(http_error(status.as_u16(), &bytes));
        }
    }

    pub async fn catalog<T: DeserializeOwned>(&self) -> Result<T, CliError> {
        self.read(self.url(&["api", "payments", "catalog"]), false)
            .await
    }

    pub async fn status(&self) -> Result<Value, CliError> {
        self.read(self.url(&["payments", "topup", "status"]), false)
            .await
    }

    pub async fn entitlements(&self) -> Result<Value, CliError> {
        self.read(self.url(&["api", "payments", "entitlements"]), true)
            .await
    }

    pub async fn ledger(&self, limit: Option<i64>, offset: Option<i64>) -> Result<Value, CliError> {
        let mut url = self.url(&["api", "payments", "ledger"]);
        {
            let mut query = url.query_pairs_mut();
            if let Some(limit) = limit {
                query.append_pair("limit", &limit.to_string());
            }
            if let Some(offset) = offset {
                query.append_pair("offset", &offset.to_string());
            }
        }
        if url.query() == Some("") {
            url.set_query(None);
        }
        self.read(url, true).await
    }

    pub async fn receipt(&self, memo: &str) -> Result<TopupReply, CliError> {
        self.read(self.url(&["api", "payments", "topups", memo]), true)
            .await
    }

    pub async fn prepare(&self, sku: &str, payer: &str) -> Result<PrepareReply, CliError> {
        let request = self
            .http
            .post(self.url(&["api", "payments", "topup", sku, "prepare"]))
            .timeout(REQUEST_TIMEOUT)
            .json(&PrepareBody {
                payer: payer.to_string(),
            });
        let response = self.authed(request)?.send().await.map_err(transport)?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(transport)?;
        if status == StatusCode::PAYMENT_REQUIRED {
            return decode(&bytes, status);
        }
        Err(http_error(status.as_u16(), &bytes))
    }

    pub async fn submit(&self, sku: &str, payment_signature: &str) -> Result<SubmitAnswer, String> {
        let request = self
            .http
            .post(self.url(&["api", "payments", "topup", sku]))
            .timeout(SUBMIT_TIMEOUT)
            .header("payment-signature", payment_signature);
        let request = self.authed(request).map_err(|e| e.to_string())?;
        let response = request.send().await.map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let retry_after = retry_after(response.headers());
        let body = response.bytes().await.map_err(|e| e.to_string())?.to_vec();
        let reply = serde_json::from_slice::<TopupReply>(&body).ok();
        Ok(SubmitAnswer {
            status,
            reply,
            retry_after,
            body,
        })
    }
}

pub fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

fn transport(e: reqwest::Error) -> CliError {
    let code = if e.is_timeout() {
        "timeout"
    } else {
        "network_error"
    };
    CliError::transient(code, e.to_string())
}

fn decode<T: DeserializeOwned>(bytes: &[u8], status: StatusCode) -> Result<T, CliError> {
    serde_json::from_slice(bytes).map_err(|e| {
        CliError::transient(
            "unexpected_response",
            format!("could not decode the server's answer: {e}"),
        )
        .with_status(status.as_u16())
    })
}

pub fn exit_for_status(status: u16) -> Exit {
    match status {
        401 | 403 => Exit::Auth,
        404 => Exit::Usage,
        400 | 402 | 409 | 410 | 422 => Exit::Rejected,
        _ => Exit::Transient,
    }
}

fn default_code(status: u16) -> String {
    match status {
        401 => "unauthorized".into(),
        403 => "forbidden".into(),
        404 => "not_found".into(),
        429 => "rate_limited".into(),
        500..=599 => "server_error".into(),
        other => format!("http_{other}"),
    }
}

pub fn http_error(status: u16, body: &[u8]) -> CliError {
    let parsed = serde_json::from_slice::<Value>(body).ok();
    let field = |name: &str| {
        parsed
            .as_ref()
            .and_then(|v| v.get(name))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let code = field("code").unwrap_or_else(|| default_code(status));
    let text: String = String::from_utf8_lossy(body)
        .trim()
        .chars()
        .take(MAX_MESSAGE)
        .collect();
    let message = field("error")
        .or_else(|| (parsed.is_none() && !text.is_empty()).then_some(text))
        .unwrap_or_else(|| format!("the server answered HTTP {status}"));
    CliError::new(exit_for_status(status), code, message).with_status(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_exit_categories() {
        for (status, exit) in [
            (401, Exit::Auth),
            (403, Exit::Auth),
            (404, Exit::Usage),
            (400, Exit::Rejected),
            (409, Exit::Rejected),
            (410, Exit::Rejected),
            (429, Exit::Transient),
            (500, Exit::Transient),
            (503, Exit::Transient),
        ] {
            assert_eq!(exit_for_status(status), exit, "{status}");
        }
    }

    #[test]
    fn json_error_bodies_keep_the_server_code_and_message() {
        let e = http_error(
            403,
            br#"{"code":"topup_not_allowed","error":"limited to listed accounts"}"#,
        );
        assert_eq!(e.exit, Exit::Auth);
        assert_eq!(e.code(), "topup_not_allowed");
        assert_eq!(e.body.message, "limited to listed accounts");
        assert_eq!(e.body.http_status, Some(403));
        let e = http_error(404, br#"{"code":"memo_not_found"}"#);
        assert_eq!(e.code(), "memo_not_found");
        assert_eq!(e.body.message, "the server answered HTTP 404");
    }

    #[test]
    fn plain_text_and_empty_bodies_fall_back_to_status_codes() {
        let e = http_error(500, b"ledger read: connection refused");
        assert_eq!(e.code(), "server_error");
        assert_eq!(e.body.message, "ledger read: connection refused");
        let e = http_error(401, b"");
        assert_eq!(e.code(), "unauthorized");
        let long = vec![b'x'; 2000];
        assert_eq!(http_error(502, &long).body.message.len(), MAX_MESSAGE);
    }

    #[test]
    fn retry_after_reads_whole_seconds_only() {
        let mut headers = HeaderMap::new();
        assert_eq!(retry_after(&headers), None);
        headers.insert(RETRY_AFTER, "5".parse().unwrap());
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(5)));
        headers.insert(
            RETRY_AFTER,
            "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after(&headers), None);
    }

    #[test]
    fn urls_append_to_the_base_path_and_escape_segments() {
        let api = Api::new(Url::parse("https://host.example/base/").unwrap(), None).unwrap();
        assert_eq!(
            api.url(&["api", "payments", "topups", "a/b c"]).as_str(),
            "https://host.example/base/api/payments/topups/a%2Fb%20c"
        );
        let api = Api::new(Url::parse("https://host.example").unwrap(), None).unwrap();
        assert_eq!(
            api.url(&["payments", "topup", "status"]).as_str(),
            "https://host.example/payments/topup/status"
        );
        assert_eq!(api.require_api_key().unwrap_err().code(), "missing_api_key");
    }
}
