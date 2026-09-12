//! API-key storage backed by the operating system's credential store.
//!
//! Keys are never written to disk in plaintext: they go to Windows Credential Manager /
//! macOS Keychain / the Linux kernel keyring through `keyring`. The frontend can read only
//! *whether* a key exists, never the secret itself — the engine receives it as an
//! environment variable at process spawn time.

use keyring::Entry;

const SERVICE: &str = "wenyi-desktop";

fn entry(account: &str) -> Result<Entry, String> {
    if account.trim().is_empty() {
        return Err("A credential name is required".into());
    }
    Entry::new(SERVICE, account.trim()).map_err(|e| e.to_string())
}

/// Store (or replace) the secret for `account`.
pub fn set(account: &str, secret: &str) -> Result<(), String> {
    if secret.is_empty() {
        return Err("Refusing to store an empty key".into());
    }
    entry(account)?
        .set_password(secret)
        .map_err(|e| format!("Could not save the key to the system credential store: {e}"))
}

/// Read the secret for `account`, or `None` when nothing is stored.
pub fn get(account: &str) -> Option<String> {
    let entry = entry(account).ok()?;
    match entry.get_password() {
        Ok(secret) if !secret.is_empty() => Some(secret),
        _ => None,
    }
}

/// Whether a secret exists for `account`.
pub fn has(account: &str) -> bool {
    get(account).is_some()
}

/// Remove the stored secret; missing entries are not an error.
pub fn clear(account: &str) -> Result<(), String> {
    let entry = entry(account)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Could not remove the stored key: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Distinct from the app's real service name so a developer run cannot clobber a
    /// stored credential, though the account name is still unique per test run.
    const TEST_ACCOUNT: &str = "wenyi-desktop-selftest";

    #[test]
    fn empty_account_is_rejected() {
        assert!(set("", "secret").is_err());
        assert!(clear("").is_err());
        assert!(!has(""));
    }

    #[test]
    fn empty_secret_is_rejected() {
        assert!(set(TEST_ACCOUNT, "").is_err());
    }

    /// Exercises the real OS credential store: Windows Credential Manager here, Keychain
    /// or the kernel keyring elsewhere.
    #[test]
    fn roundtrip_through_the_os_credential_store() {
        let _ = clear(TEST_ACCOUNT); // start from a known state

        set(TEST_ACCOUNT, "sk-roundtrip-test").expect("set should succeed");
        assert!(has(TEST_ACCOUNT));
        assert_eq!(get(TEST_ACCOUNT).as_deref(), Some("sk-roundtrip-test"));

        // Replacing an existing secret must overwrite, not append or fail.
        set(TEST_ACCOUNT, "sk-second").expect("overwrite should succeed");
        assert_eq!(get(TEST_ACCOUNT).as_deref(), Some("sk-second"));

        clear(TEST_ACCOUNT).expect("clear should succeed");
        assert!(!has(TEST_ACCOUNT));
        assert_eq!(get(TEST_ACCOUNT), None);

        // Clearing twice must be idempotent rather than an error.
        clear(TEST_ACCOUNT).expect("second clear should be a no-op");
    }
}
