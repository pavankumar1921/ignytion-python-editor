//! API keys live in the operating system's credential store, never in the web
//! frontend, source control, or the packaged app:
//!   Windows -> Credential Manager, macOS -> Keychain, Linux -> kernel keyring.
//! Keys are stored per provider host, so a key saved for one provider can never
//! be sent to a different one by editing the base URL.

use crate::ai::host_of;

const SERVICE: &str = "io.ignytion.python-editor";

fn entry(base_url: &str) -> Result<keyring::Entry, String> {
    let host = host_of(base_url).ok_or_else(|| "The provider URL is not valid.".to_string())?;
    keyring::Entry::new(SERVICE, &format!("ai-api-key@{host}")).map_err(|e| e.to_string())
}

pub fn get(base_url: &str) -> Result<Option<String>, String> {
    match entry(base_url)?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Could not read the OS credential store: {e}")),
    }
}

pub fn set(base_url: &str, key: &str) -> Result<(), String> {
    entry(base_url)?
        .set_password(key)
        .map_err(|e| format!("Could not save to the OS credential store: {e}"))
}

pub fn clear(base_url: &str) -> Result<(), String> {
    match entry(base_url)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Could not remove the key from the OS credential store: {e}")),
    }
}
