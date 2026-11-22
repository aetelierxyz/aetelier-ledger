use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_aetelier-ledger-cli");

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aetelier-ledger-cli-bin-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cli(args: &[&str], api_key: Option<&str>) -> Output {
    cli_env(args, api_key, &[])
}

fn cli_env(args: &[&str], api_key: Option<&str>, vars: &[(&str, &str)]) -> Output {
    let mut command = Command::new(BIN);
    command
        .args(args)
        .env_remove("AETELIER_URL")
        .env_remove("AETELIER_API_KEY")
        .env_remove("AETELIER_NETWORK")
        .env_remove("AETELIER_PAYER_KEYPAIR")
        .env_remove("AETELIER_RPC_URL")
        .stdin(Stdio::null());
    if let Some(key) = api_key {
        command.env("AETELIER_API_KEY", key);
    }
    for (name, value) in vars {
        command.env(name, value);
    }
    command.output().unwrap()
}

fn json(output: &Output) -> Value {
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout}");
    serde_json::from_str(&stdout).unwrap()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}

fn keygen(dir: &Path) -> String {
    let path = dir.join("payer.json");
    let out = cli(&["keygen", path.to_str().unwrap()], None);
    assert_eq!(code(&out), 0);
    path.to_str().unwrap().to_string()
}

#[test]
fn help_and_version_print_text_and_exit_zero() {
    let out = cli(&["--help"], None);
    assert_eq!(code(&out), 0);
    let text = String::from_utf8(out.stdout).unwrap();
    for command in [
        "catalog", "status", "balance", "history", "buy", "receipt", "keygen", "address",
    ] {
        assert!(text.contains(command), "{command}");
    }
    assert_eq!(code(&cli(&["--version"], None)), 0);
}

#[test]
fn usage_errors_are_one_json_object_and_exit_one() {
    let out = cli(&["frobnicate"], None);
    assert_eq!(code(&out), 1);
    assert_eq!(json(&out)["error"]["code"], "usage");
    let out = cli(&["balance"], None);
    assert_eq!(code(&out), 1);
    assert_eq!(json(&out)["error"]["code"], "missing_api_key");
    let out = cli(&["status", "--url", "ftp://aetelier.xyz"], None);
    assert_eq!(code(&out), 1);
    assert_eq!(json(&out)["error"]["code"], "invalid_url");
}

#[test]
fn keygen_and_address_round_trip_without_overwriting() {
    let dir = scratch("keys");
    let path = dir.join("payer.json");
    let out = cli(&["keygen", path.to_str().unwrap()], None);
    assert_eq!(code(&out), 0);
    let created = json(&out);
    let out = cli(&["address", "--keypair", path.to_str().unwrap()], None);
    assert_eq!(code(&out), 0);
    assert_eq!(json(&out)["address"], created["address"]);
    let out = cli(&["keygen", path.to_str().unwrap()], None);
    assert_eq!(code(&out), 1);
    assert_eq!(json(&out)["error"]["code"], "keypair_exists");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let out = cli(&["address", "--keypair", path.to_str().unwrap()], None);
        assert_eq!(code(&out), 1);
        assert_eq!(json(&out)["error"]["code"], "keypair_permissions");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn buy_without_network_or_on_mainnet_is_refused_early() {
    let dir = scratch("network");
    let key = keygen(&dir);
    let out = cli(&["buy", "data_100", "--keypair", &key], Some("k"));
    assert_eq!(code(&out), 1);
    assert_eq!(json(&out)["error"]["code"], "missing_network");
    let out = cli(
        &[
            "buy",
            "data_100",
            "--keypair",
            &key,
            "--network",
            "mainnet",
            "--yes",
        ],
        Some("k"),
    );
    assert_eq!(code(&out), 2);
    assert_eq!(json(&out)["error"]["code"], "mainnet_not_enabled");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn buy_without_a_terminal_or_yes_is_refused_after_one_warning() {
    let app = Router::new().route(
        "/api/payments/catalog",
        get(|| async {
            Json(json!({"bundles":[{"sku":"data_100","kind":"data","units":100,"price_atomic":1000000,"price_usdc":"1.00"}]}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let dir = scratch("tty");
    let key = keygen(&dir);
    let out = tokio::task::spawn_blocking(move || {
        cli(
            &[
                "buy",
                "data_100",
                "--url",
                &url,
                "--network",
                "devnet",
                "--keypair",
                &key,
                "--rpc-url",
                "http://127.0.0.1:9",
            ],
            Some("k"),
        )
    })
    .await
    .unwrap();
    assert_eq!(code(&out), 2, "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(json(&out)["error"]["code"], "confirmation_required");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(stderr.matches("warning:").count(), 1, "{stderr}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn flags_win_over_environment_variables() {
    let dir = scratch("precedence");
    let key = keygen(&dir);
    let out = cli_env(
        &["status", "--url", "http://127.0.0.1:9"],
        None,
        &[("AETELIER_URL", "ftp://aetelier.xyz")],
    );
    assert_eq!(code(&out), 6);
    assert_eq!(json(&out)["error"]["code"], "network_error");
    let out = cli_env(&["status"], None, &[("AETELIER_URL", "ftp://aetelier.xyz")]);
    assert_eq!(code(&out), 1);
    assert_eq!(json(&out)["error"]["code"], "invalid_url");

    let buy = ["buy", "data_100", "--url", "http://127.0.0.1:9", "--yes"];
    let out = cli_env(
        &buy,
        Some("k"),
        &[
            ("AETELIER_NETWORK", "mainnet"),
            ("AETELIER_PAYER_KEYPAIR", key.as_str()),
        ],
    );
    assert_eq!(code(&out), 2);
    assert_eq!(json(&out)["error"]["code"], "mainnet_not_enabled");
    let mut devnet = buy.to_vec();
    devnet.extend(["--network", "devnet", "--keypair", key.as_str()]);
    let out = cli_env(
        &devnet,
        Some("k"),
        &[
            ("AETELIER_NETWORK", "mainnet"),
            ("AETELIER_PAYER_KEYPAIR", "/nonexistent.json"),
        ],
    );
    assert_eq!(code(&out), 6);
    assert_eq!(json(&out)["error"]["code"], "network_error");
    let out = cli_env(
        &[
            "buy",
            "data_100",
            "--network",
            "devnet",
            "--keypair",
            key.as_str(),
            "--rpc-url",
            "http://rpc.example:8899",
        ],
        Some("k"),
        &[],
    );
    assert_eq!(code(&out), 1);
    assert_eq!(json(&out)["error"]["code"], "invalid_rpc_url");
    std::fs::remove_dir_all(dir).unwrap();
}
