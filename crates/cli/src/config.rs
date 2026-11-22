use std::str::FromStr;

use aetelier_ledger_x402::{Address, Network};
use clap::ValueEnum;
use url::{Host, Url};

use crate::error::CliError;

pub const DEFAULT_URL: &str = "https://aetelier.xyz";
pub const MAINNET_TREASURY: Option<&str> = None;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum NetworkName {
    Devnet,
    Mainnet,
}

impl NetworkName {
    pub fn network(self) -> Network {
        match self {
            NetworkName::Devnet => Network::SolanaDevnet,
            NetworkName::Mainnet => Network::SolanaMainnet,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            NetworkName::Devnet => "devnet",
            NetworkName::Mainnet => "mainnet",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub url: Url,
    pub host: String,
    pub is_default: bool,
}

pub fn endpoint(raw: &str) -> Result<Endpoint, CliError> {
    let text = raw.trim();
    let invalid = |why: String| CliError::usage("invalid_url", format!("--url {text}: {why}"));
    let url = Url::parse(text).map_err(|e| invalid(e.to_string()))?;
    let loopback = match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        Some(Host::Domain(domain)) => domain == "localhost",
        None => false,
    };
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback)) {
        return Err(invalid("use https, or http on a loopback host".into()));
    }
    if url.query().is_some() || url.fragment().is_some() || !url.username().is_empty() {
        return Err(invalid(
            "give only scheme, host, optional port and path".into(),
        ));
    }
    let Some(host) = url.host_str() else {
        return Err(invalid("missing host".into()));
    };
    let host = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    let is_default = url.as_str().trim_end_matches('/') == DEFAULT_URL;
    Ok(Endpoint {
        url,
        host,
        is_default,
    })
}

pub fn api_key(raw: Option<String>) -> Option<String> {
    raw.map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

pub fn pinned_treasury(raw: Option<&str>) -> Result<Option<Address>, CliError> {
    raw.map(|text| {
        Address::from_str(text)
            .map_err(|e| CliError::usage("invalid_treasury", format!("{text}: {e}")))
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_url_is_recognised_with_or_without_trailing_slash() {
        for raw in [
            "https://aetelier.xyz",
            "https://aetelier.xyz/",
            " https://aetelier.xyz ",
        ] {
            let e = endpoint(raw).unwrap();
            assert!(e.is_default, "{raw}");
            assert_eq!(e.host, "aetelier.xyz");
        }
    }

    #[test]
    fn other_hosts_keep_their_port_and_are_not_default() {
        let e = endpoint("http://127.0.0.1:8090").unwrap();
        assert!(!e.is_default);
        assert_eq!(e.host, "127.0.0.1:8090");
        let e = endpoint("https://aetelier.xyz.evil.example").unwrap();
        assert!(!e.is_default);
        assert!(endpoint("http://localhost:3000").is_ok());
        assert!(endpoint("http://[::1]:8090").is_ok());
    }

    #[test]
    fn unusable_urls_are_usage_errors() {
        for raw in [
            "aetelier.xyz",
            "ftp://aetelier.xyz",
            "http://aetelier.xyz",
            "http://10.0.0.5:8090",
            "https://aetelier.xyz/?x=1",
            "https://user@aetelier.xyz",
        ] {
            let e = endpoint(raw).unwrap_err();
            assert_eq!(e.code(), "invalid_url", "{raw}");
        }
    }

    #[test]
    fn blank_api_keys_count_as_missing() {
        assert_eq!(api_key(None), None);
        assert_eq!(api_key(Some("  ".into())), None);
        assert_eq!(api_key(Some(" k ".into())), Some("k".into()));
    }

    #[test]
    fn network_names_map_to_caip2_ids() {
        assert_eq!(NetworkName::Devnet.network().caip2(), Network::DEVNET_CAIP2);
        assert_eq!(
            NetworkName::Mainnet.network().caip2(),
            Network::MAINNET_CAIP2
        );
        assert_eq!(MAINNET_TREASURY, None);
    }

    #[test]
    fn treasury_pins_parse_or_fail_as_usage() {
        assert_eq!(pinned_treasury(None).unwrap(), None);
        let pin = "G7t2qpRF4kKZ1AqQZyT3ZVjERb2njQQY7JxHE2yBUGL7";
        assert_eq!(
            pinned_treasury(Some(pin)).unwrap().unwrap().to_string(),
            pin
        );
        assert_eq!(
            pinned_treasury(Some("nope")).unwrap_err().code(),
            "invalid_treasury"
        );
    }
}
