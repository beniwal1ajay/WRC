use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::{Context, Result, bail};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use tracing::info;

use std::sync::{LazyLock, Mutex};

pub const AUTH_METHOD_PIN: u16 = 1;
pub const AUTH_METHOD_TOKEN: u16 = 2;

static PENDING_PINS: LazyLock<Mutex<HashMap<String, (String, String, u64)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Representation of an authorized remote client device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedClient {
    pub device_id: String,
    pub device_name: String,
    pub auth_token: String,
    pub paired_at: u64,
    pub last_seen_at: u64,
}

/// Persistent store of authorized clients saved at ~/.config/ward/authorized_clients.json
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AuthorizedClientsStore {
    pub clients: HashMap<String, AuthorizedClient>,
}

/// Authentication manager for verifying clients and managing PIN/token pairings.
pub struct AuthManager {
    store_path: PathBuf,
    store: AuthorizedClientsStore,
}

impl AuthManager {
    /// Load or initialize the authorized clients store from default config path.
    pub fn load_default() -> Result<Self> {
        let config_dir = dirs_config_dir().unwrap_or_else(|| PathBuf::from("./config"));
        let ward_dir = config_dir.join("ward");
        fs::create_dir_all(&ward_dir)
            .context(format!("Failed to create ward config directory at {}", ward_dir.display()))?;

        let store_path = ward_dir.join("authorized_clients.json");
        Self::load_from_path(store_path)
    }

    /// Load or initialize from a specific file path.
    pub fn load_from_path(store_path: PathBuf) -> Result<Self> {
        let store = if store_path.exists() {
            let content = fs::read_to_string(&store_path)
                .context(format!("Failed to read authorized clients at {}", store_path.display()))?;
            serde_json::from_str(&content)
                .context("Failed to deserialize authorized clients JSON")?
        } else {
            let initial = AuthorizedClientsStore::default();
            Self::save_store(&store_path, &initial)?;
            initial
        };

        Ok(Self {
            store_path,
            store,
        })
    }

    fn save_store(path: &Path, store: &AuthorizedClientsStore) -> Result<()> {
        let json = serde_json::to_string_pretty(store)
            .context("Failed to serialize authorized clients")?;
        fs::write(path, json)
            .context(format!("Failed to write authorized clients to {}", path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = fs::Permissions::from_mode(0o600);
            fs::set_permissions(path, permissions)
                .context("Failed to set 0600 permissions on authorized_clients.json")?;
        }

        Ok(())
    }

    /// Check if a client device ID is already authorized.
    pub fn is_authorized(&self, device_id: &str) -> bool {
        self.store.clients.contains_key(device_id)
    }

    /// Retrieve any active pairing PIN pending for a device.
    pub fn get_pending_pin(device_id: &str) -> Option<String> {
        let lock = PENDING_PINS.lock().unwrap();
        lock.get(device_id).map(|(pin, _, _)| pin.clone())
    }

    /// Verify an existing client's auth token.
    pub fn verify_token(&mut self, device_id: &str, token: &str) -> bool {
        if let Some(client) = self.store.clients.get_mut(device_id) {
            if client.auth_token == token {
                client.last_seen_at = current_timestamp();
                let _ = Self::save_store(&self.store_path, &self.store);
                return true;
            }
        }
        false
    }

    /// Generate a 6-digit numeric pairing PIN and a new session auth token for an unknown client.
    pub fn generate_pairing_pin(&mut self, device_id: &str, device_name: &str) -> (String, String) {
        // Generate 6-digit numeric PIN: 100000 - 999999
        let mut random_bytes = [0u8; 4];
        OsRng.fill_bytes(&mut random_bytes);
        let num = u32::from_be_bytes(random_bytes) % 900_000 + 100_000;
        let pin = num.to_string();

        // Generate 32-byte hex token for future passwordless sessions
        let mut token_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut token_bytes);
        let token = hex_encode(&token_bytes);

        PENDING_PINS.lock().unwrap().insert(
            device_id.to_string(),
            (pin.clone(), token.clone(), current_timestamp()),
        );

        info!("============================================================");
        info!("  [WARD AUTH] Pairing Request from: '{}'", device_name);
        info!("  Device ID: {}", device_id);
        info!("  PIN CODE:  {}", pin);
        info!("  Enter this 6-digit PIN on your Android device to connect.");
        info!("============================================================");

        (pin, token)
    }

    /// Verify the PIN provided by the client. If valid, register the client permanently.
    pub fn verify_pairing_pin(
        &mut self,
        device_id: &str,
        device_name: &str,
        provided_pin: &str,
    ) -> Result<String> {
        let (expected_pin, token, created_at) = {
            let lock = PENDING_PINS.lock().unwrap();
            match lock.get(device_id) {
                Some(pending) => pending.clone(),
                None => bail!("No pending pairing request found for device ID {}", device_id),
            }
        };

        // Expire PINs after 5 minutes (300 seconds)
        if current_timestamp().saturating_sub(created_at) > 300 {
            PENDING_PINS.lock().unwrap().remove(device_id);
            bail!("Pairing PIN expired. Please reconnect to request a new PIN.");
        }

        if expected_pin.trim() != provided_pin.trim() {
            bail!("Invalid pairing PIN");
        }

        // Successfully verified PIN! Save to authorized store
        PENDING_PINS.lock().unwrap().remove(device_id);
        let client = AuthorizedClient {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            auth_token: token.clone(),
            paired_at: current_timestamp(),
            last_seen_at: current_timestamp(),
        };

        self.store.clients.insert(device_id.to_string(), client);
        Self::save_store(&self.store_path, &self.store)?;

        info!(
            "Device '{}' ({}) successfully paired and authorized!",
            device_name, device_id
        );

        Ok(token)
    }
}

fn dirs_config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
        })
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_pin_generation_and_verification() {
        let tmp = std::env::temp_dir().join(format!("ward_auth_test_{}.json", current_timestamp()));
        let mut auth = AuthManager::load_from_path(tmp.clone()).expect("Failed to create AuthManager");

        let device_id = "test_device_12345";
        let device_name = "Test Pixel";

        assert!(!auth.is_authorized(device_id));

        let (pin, _token) = auth.generate_pairing_pin(device_id, device_name);
        assert_eq!(pin.len(), 6);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));

        // Invalid PIN should fail
        assert!(auth.verify_pairing_pin(device_id, device_name, "000000").is_err());

        // Correct PIN should succeed
        let auth_token = auth.verify_pairing_pin(device_id, device_name, &pin).expect("Verification failed");
        assert!(!auth_token.is_empty());

        // Should now be authorized
        assert!(auth.is_authorized(device_id));

        // Reconnect with token should succeed
        assert!(auth.verify_token(device_id, &auth_token));

        let _ = fs::remove_file(tmp);
    }
}
