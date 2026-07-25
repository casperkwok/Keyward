//! The only code in the product that reads or writes a secret value.
//!
//! Values live in the OS keychain; everything else lives in `vault.json`, in the
//! clear, containing nothing sensitive (ARCHITECTURE.md §5). Keeping both behind
//! one type means there is a single place to audit the claim "no plaintext is ever
//! written to disk by Keyward".

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use keyward_core::{AddError, Approval, Delivery, Secret, Use, Vault, mask};
use zeroize::Zeroize;

/// Keychain service name. Every item Keyward owns carries it, so a user can find
/// and audit them in Keychain Access under one heading.
const SERVICE: &str = "ai.keyward.vault";

pub struct SecretStore {
    dir: PathBuf,
    vault: Vault,
}

#[derive(Debug)]
pub enum StoreError {
    Io(String),
    Keychain(String),
    NotFound(String),
    NameTaken(String),
    Corrupt(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(m) => write!(f, "{m}"),
            StoreError::Keychain(m) => write!(f, "the keychain refused: {m}"),
            StoreError::NotFound(n) => write!(f, "no secret named `{n}`"),
            StoreError::NameTaken(n) => write!(f, "a secret named `{n}` already exists"),
            StoreError::Corrupt(m) => write!(f, "vault.json could not be read: {m}"),
        }
    }
}

impl StoreError {
    /// Wire error code (§7).
    pub fn code(&self) -> &'static str {
        match self {
            StoreError::Io(_) | StoreError::Corrupt(_) => "io_error",
            StoreError::Keychain(_) => "keychain_error",
            StoreError::NotFound(_) => "not_found",
            StoreError::NameTaken(_) => "name_taken",
        }
    }
}

impl SecretStore {
    pub fn open(dir: PathBuf) -> Result<Self, StoreError> {
        fs::create_dir_all(&dir).map_err(|e| StoreError::Io(e.to_string()))?;
        let path = dir.join("vault.json");
        let vault = if path.exists() {
            let text = fs::read_to_string(&path).map_err(|e| StoreError::Io(e.to_string()))?;
            // A parse failure must not be papered over with `Vault::default()`:
            // that would present an empty list to someone whose secrets are still
            // in the keychain, and the obvious next move — re-adding them — would
            // overwrite the file for real.
            serde_json::from_str(&text).map_err(|e| StoreError::Corrupt(e.to_string()))?
        } else {
            Vault {
                schema: 1,
                ..Vault::default()
            }
        };
        Ok(Self { dir, vault })
    }

    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    pub fn add(
        &mut self,
        name: String,
        display: String,
        delivery: Delivery,
        mut value: String,
    ) -> Result<(), StoreError> {
        if self.vault.get(&name).is_some() {
            value.zeroize();
            return Err(StoreError::NameTaken(name));
        }
        let secret = Secret {
            name: name.clone(),
            display,
            delivery,
            masked: Some(mask(&value)),
            revision: 1,
            approval: Default::default(),
        };
        // Keychain first: if it fails there must be no metadata entry pointing at
        // a value that was never stored.
        let result = Self::write_keychain(&secret.keychain_account(), &value);
        value.zeroize();
        result?;

        self.vault.add(secret).map_err(|e| match e {
            AddError::NameTaken(n) => StoreError::NameTaken(n),
        })?;
        self.persist()
    }

    pub fn rotate(&mut self, name: &str, mut value: String) -> Result<(), StoreError> {
        let Some(current) = self.vault.get(name) else {
            value.zeroize();
            return Err(StoreError::NotFound(name.to_owned()));
        };
        let old_account = current.keychain_account();
        let masked = mask(&value);

        let account = self
            .vault
            .rotate(name, masked)
            .map_err(|_| StoreError::NotFound(name.to_owned()))?;
        let result = Self::write_keychain(&account, &value);
        value.zeroize();
        result?;

        self.persist()?;
        // Only now drop the previous revision. Deleting first would leave a window
        // where a crash loses the secret entirely.
        let _ = Self::delete_keychain(&old_account);
        Ok(())
    }

    /// Rename the label only. The slug is immutable — changing it would silently
    /// break every `.env` pointing at the old reference, and a rename that breaks
    /// a project is not a rename the user asked for.
    pub fn set_display(&mut self, name: &str, display: String) -> Result<(), StoreError> {
        let secret = self
            .vault
            .secrets
            .get_mut(name)
            .ok_or_else(|| StoreError::NotFound(name.to_owned()))?;
        secret.display = display;
        self.persist()
    }

    /// Turn the approval prompt on or off for one secret (§7
    /// `vault.set_approval`).
    ///
    /// A human decision made in the GUI, and the only way `Approval::Ask` is ever
    /// set — nothing on the request path may raise or lower it, or a caller could
    /// switch off the prompt that exists to catch it.
    pub fn set_approval(&mut self, name: &str, approval: Approval) -> Result<(), StoreError> {
        let secret = self
            .vault
            .secrets
            .get_mut(name)
            .ok_or_else(|| StoreError::NotFound(name.to_owned()))?;
        secret.approval = approval;
        self.persist()
    }

    pub fn remove(&mut self, name: &str) -> Result<(), StoreError> {
        let Some(secret) = self.vault.get(name) else {
            return Err(StoreError::NotFound(name.to_owned()));
        };
        let account = secret.keychain_account();
        self.vault.secrets.remove(name);
        self.persist()?;
        // Metadata is gone, so a keychain item left behind is orphaned rather than
        // dangerous; report success and let the next `prune` collect it.
        let _ = Self::delete_keychain(&account);
        Ok(())
    }

    /// Read a plaintext value. **The only function in the daemon that returns
    /// one.** Callers are responsible for zeroizing what they receive.
    ///
    /// It is called from exactly two places — `secret.hand` and `broker.open` —
    /// and both run the policy check first. Any third caller is a new hole in
    /// §3.3, so keep the plaintext surface one function wide and grep for it.
    pub fn reveal(&self, name: &str) -> Result<String, StoreError> {
        let secret = self
            .vault
            .get(name)
            .ok_or_else(|| StoreError::NotFound(name.to_owned()))?;
        Self::read_keychain(&secret.keychain_account())
    }

    // MARK: keychain

    fn entry(account: &str) -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(SERVICE, account).map_err(|e| StoreError::Keychain(e.to_string()))
    }

    fn write_keychain(account: &str, value: &str) -> Result<(), StoreError> {
        Self::entry(account)?
            .set_password(value)
            .map_err(|e| StoreError::Keychain(e.to_string()))
    }

    fn read_keychain(account: &str) -> Result<String, StoreError> {
        Self::entry(account)?
            .get_password()
            .map_err(|e| StoreError::Keychain(e.to_string()))
    }

    fn delete_keychain(account: &str) -> Result<(), StoreError> {
        Self::entry(account)?
            .delete_credential()
            .map_err(|e| StoreError::Keychain(e.to_string()))
    }

    // MARK: persistence

    /// Write `vault.json` atomically: serialise to a temp file in the same
    /// directory, then rename. A partial write here would strand every secret in
    /// the keychain with no metadata pointing at it.
    fn persist(&self) -> Result<(), StoreError> {
        let json =
            serde_json::to_string_pretty(&self.vault).map_err(|e| StoreError::Io(e.to_string()))?;
        let final_path = self.dir.join("vault.json");
        let temp_path = self.dir.join("vault.json.tmp");
        {
            let mut file =
                fs::File::create(&temp_path).map_err(|e| StoreError::Io(e.to_string()))?;
            file.write_all(json.as_bytes())
                .map_err(|e| StoreError::Io(e.to_string()))?;
            file.sync_all().map_err(|e| StoreError::Io(e.to_string()))?;
        }
        fs::rename(&temp_path, &final_path).map_err(|e| StoreError::Io(e.to_string()))?;
        Self::restrict(&final_path);
        Ok(())
    }

    #[cfg(unix)]
    fn restrict(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }

    #[cfg(not(unix))]
    fn restrict(_path: &Path) {}

    // MARK: usage log

    /// Append one use. The log is the "who used it" feature, which is half the
    /// product — so a failure to write it is logged and swallowed rather than
    /// failing the request the user actually made.
    pub fn record(&self, use_record: &Use) {
        let Ok(line) = serde_json::to_string(use_record) else {
            return;
        };
        let path = self.dir.join("uses.jsonl");
        let opened = fs::OpenOptions::new().create(true).append(true).open(&path);
        match opened {
            Ok(mut file) => {
                let _ = writeln!(file, "{line}");
                Self::restrict(&path);
            }
            Err(e) => eprintln!("could not record use: {e}"),
        }
    }

    /// Most recent uses, newest first, optionally for one secret.
    pub fn uses(&self, name: Option<&str>, limit: usize) -> Vec<Use> {
        let path = self.dir.join("uses.jsonl");
        let Ok(text) = fs::read_to_string(&path) else {
            return Vec::new();
        };
        let mut out: Vec<Use> = text
            .lines()
            .filter_map(|l| serde_json::from_str::<Use>(l).ok())
            .filter(|u| name.is_none_or(|n| u.secret == n))
            .collect();
        out.reverse();
        out.truncate(limit);
        out
    }
}
