use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use solana_keypair::Keypair;

use crate::error::CliError;

pub fn expand(path: &Path) -> PathBuf {
    match (
        path.to_str().and_then(|p| p.strip_prefix("~/")),
        std::env::var_os("HOME"),
    ) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => path.to_path_buf(),
    }
}

pub fn load_keypair(path: &Path) -> Result<Keypair, CliError> {
    let path = expand(path);
    let shown = path.display();
    let meta = fs::metadata(&path)
        .map_err(|e| CliError::usage("keypair_unreadable", format!("{shown}: {e}")))?;
    if !meta.is_file() {
        return Err(CliError::usage(
            "keypair_unreadable",
            format!("{shown} is not a file"),
        ));
    }
    check_private(&path, &meta)?;
    let text = fs::read_to_string(&path)
        .map_err(|e| CliError::usage("keypair_unreadable", format!("{shown}: {e}")))?;
    let bytes: Vec<u8> = serde_json::from_str(&text).map_err(|_| {
        CliError::usage(
            "keypair_format",
            format!("{shown} is not a Solana CLI keypair file (JSON byte array)"),
        )
    })?;
    Keypair::try_from(bytes.as_slice())
        .map_err(|e| CliError::usage("keypair_format", format!("{shown}: {e}")))
}

#[cfg(unix)]
fn check_private(path: &Path, meta: &fs::Metadata) -> Result<(), CliError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(CliError::usage(
            "keypair_permissions",
            format!(
                "{} can be read by other users (mode {mode:o}); run chmod 600 on it",
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_private(_path: &Path, _meta: &fs::Metadata) -> Result<(), CliError> {
    Ok(())
}

pub fn keygen(path: &Path) -> Result<Keypair, CliError> {
    let path = expand(path);
    let shown = path.display();
    let keypair = Keypair::new();
    let json = serde_json::to_string(&keypair.to_bytes().to_vec())
        .map_err(|e| CliError::usage("keypair_write", e.to_string()))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).map_err(|e| {
        if e.kind() == ErrorKind::AlreadyExists {
            CliError::usage(
                "keypair_exists",
                format!("{shown} already exists; refusing to overwrite it"),
            )
        } else {
            CliError::usage("keypair_write", format!("{shown}: {e}"))
        }
    })?;
    file.write_all(json.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| CliError::usage("keypair_write", format!("{shown}: {e}")))?;
    Ok(keypair)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_signer::Signer;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aetelier-ledger-cli-keys-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn keygen_writes_a_private_file_that_loads_back() {
        let dir = scratch("roundtrip");
        let path = dir.join("payer.json");
        let created = keygen(&path).unwrap();
        let loaded = load_keypair(&path).unwrap();
        assert_eq!(created.pubkey(), loaded.pubkey());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let again = keygen(&path).unwrap_err();
        assert_eq!(again.code(), "keypair_exists");
        assert_eq!(load_keypair(&path).unwrap().pubkey(), created.pubkey());
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn key_files_readable_by_others_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("perms");
        let path = dir.join("payer.json");
        keygen(&path).unwrap();
        for mode in [0o644, 0o640, 0o604] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(
                load_keypair(&path).unwrap_err().code(),
                "keypair_permissions"
            );
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        assert!(load_keypair(&path).is_ok());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_and_malformed_files_are_usage_errors() {
        let dir = scratch("bad");
        assert_eq!(
            load_keypair(&dir.join("absent.json")).unwrap_err().code(),
            "keypair_unreadable"
        );
        assert_eq!(load_keypair(&dir).unwrap_err().code(), "keypair_unreadable");
        let path = dir.join("bad.json");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options
            .open(&path)
            .unwrap()
            .write_all(b"4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU")
            .unwrap();
        assert_eq!(load_keypair(&path).unwrap_err().code(), "keypair_format");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn home_relative_paths_expand() {
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(
                expand(Path::new("~/k.json")),
                PathBuf::from(home).join("k.json")
            );
        }
        assert_eq!(
            expand(Path::new("/tmp/k.json")),
            PathBuf::from("/tmp/k.json")
        );
    }
}
