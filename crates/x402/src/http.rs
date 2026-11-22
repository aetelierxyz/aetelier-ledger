use std::time::Duration;

use reqwest::Url;
use reqwest::redirect::Policy;
use url::Host;

use crate::error::X402Error;

pub fn http_client(timeout: Duration) -> Result<reqwest::Client, X402Error> {
    Ok(reqwest::Client::builder()
        .timeout(timeout)
        .redirect(Policy::none())
        .build()?)
}

fn parse_endpoint(name: &str, value: &str) -> Result<Url, X402Error> {
    let url = Url::parse(value.trim())
        .map_err(|_| X402Error::Config(format!("{name} is not a valid URL")))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(X402Error::Config(format!(
            "{name} must not contain credentials"
        )));
    }
    Ok(url)
}

pub fn https_endpoint(name: &str, value: &str) -> Result<Url, X402Error> {
    let url = parse_endpoint(name, value)?;
    if url.scheme() != "https" {
        return Err(X402Error::Config(format!("{name} must use https")));
    }
    Ok(url)
}

pub fn client_endpoint(name: &str, value: &str) -> Result<Url, X402Error> {
    let url = parse_endpoint(name, value)?;
    let loopback = match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        Some(Host::Domain(domain)) => domain == "localhost",
        None => false,
    };
    if url.scheme() == "https" || (url.scheme() == "http" && loopback) {
        Ok(url)
    } else {
        Err(X402Error::Config(format!(
            "{name} must use https, or http on a loopback host"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALWAYS_REJECTED: &[&str] = &[
        "https://api.cdp.coinbase.com@evil.com/",
        "https://user:pass@api.cdp.coinbase.com/",
        "https://:abc123@h.example/",
        "http://127.0.0.1@evil.com",
        "http://evil.example#@127.0.0.1",
        "http://localhost.evil.example",
        "http://foo.localhost/",
        "http://127.0.0.1.evil.example",
        "http://0.0.0.0/",
        "http://[::ffff:127.0.0.1]/",
        "httpsx://h",
        "https://",
        "not a url",
    ];

    #[test]
    fn hostile_urls_are_rejected_by_both_rules() {
        for url in ALWAYS_REJECTED {
            assert!(client_endpoint("u", url).is_err(), "client accepted {url}");
            assert!(https_endpoint("u", url).is_err(), "config accepted {url}");
        }
    }

    #[test]
    fn client_rule_allows_https_and_loopback_http_only() {
        for url in [
            "https://api.cdp.coinbase.com/platform/v2/x402",
            "http://127.0.0.1:8080",
            "http://[::1]:8899",
            "http://localhost:8080/x402",
        ] {
            assert!(client_endpoint("u", url).is_ok(), "client refused {url}");
        }
        assert!(client_endpoint("u", "http://api.cdp.coinbase.com").is_err());
    }

    #[test]
    fn config_rule_allows_https_only() {
        assert!(https_endpoint("u", "https://api.devnet.solana.com").is_ok());
        for url in [
            "http://127.0.0.1:8080",
            "http://localhost:8080",
            "http://api.devnet.solana.com",
        ] {
            assert!(https_endpoint("u", url).is_err(), "config accepted {url}");
        }
    }

    #[test]
    fn endpoint_errors_name_the_variable_and_omit_the_value() {
        let err = https_endpoint("X402_RPC_URL", "http://rpc.example/?api-marker=abc123")
            .unwrap_err()
            .to_string();
        assert!(err.contains("X402_RPC_URL") && !err.contains("abc123"));
        let err = client_endpoint("rpc url", "ftp://rpc.example/?api-marker=abc123")
            .unwrap_err()
            .to_string();
        assert!(!err.contains("abc123"));
    }

    #[test]
    fn unusable_endpoints_are_named_without_echoing_the_value() {
        for value in ["https://[abc123", "https://user:abc123@rpc.example/"] {
            for err in [
                https_endpoint("X402_RPC_URL", value).unwrap_err(),
                client_endpoint("X402_RPC_URL", value).unwrap_err(),
            ] {
                let err = err.to_string();
                assert!(
                    err.contains("X402_RPC_URL") && !err.contains("abc123"),
                    "{err}"
                );
            }
        }
    }

    #[test]
    fn clients_apply_the_timeout_and_do_not_follow_redirects() {
        let client = http_client(Duration::from_secs(7)).unwrap();
        let debug = format!("{client:?}");
        assert!(debug.contains("Policy(None)"), "{debug}");
        assert!(debug.contains("TotalTimeout: 7s"), "{debug}");
    }
}
