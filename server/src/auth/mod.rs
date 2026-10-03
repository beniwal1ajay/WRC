use std::collections::HashMap;
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::{Context, Result, bail};
use rand_core::{OsRng, RngCore};
use ring::pbkdf2;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub const AUTH_METHOD_PIN: u16 = 1;
pub const AUTH_METHOD_TOKEN: u16 = 2;
pub const MAX_FAILED_PIN_ATTEMPTS: u32 = 5;
pub const LOCKOUT_DURATION_SECS: u64 = 60;
pub const PBKDF2_ITERATIONS: u32 = 600_000;

/// Persistent Host Configuration containing permanent PIN hash and host state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostConfig {
    /// 32-byte salt hex
    pub pin_salt_hex: String,
    /// 32-byte PBKDF2-HMAC-SHA256 hash hex
    pub pin_hash_hex: String,
    /// Number of PBKDF2 iterations (default: 600,000)
    pub pin_iterations: u32,
    /// Host disable switch (default: true)
    pub host_enabled: bool,
    /// Creation timestamp (seconds since Unix epoch)
    pub created_at: u64,
    /// Last updated timestamp
    pub updated_at: u64,
}

impl HostConfig {
    pub fn default_path() -> Result<PathBuf> {
        let config_dir = dirs_config_dir().unwrap_or_else(|| PathBuf::from("./config"));
        let ward_dir = config_dir.join("ward");
        fs::create_dir_all(&ward_dir)
            .context(format!("Failed to create ward config directory at {}", ward_dir.display()))?;
        Ok(ward_dir.join("host_config.json"))
    }

    pub fn load_default() -> Result<Self> {
        let path = Self::default_path()?;
        Self::load_from_path(&path)
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        if !path.exists() {
            bail!("Host configuration not found at {}. Please run 'ward-server setup' first.", path.display());
        }
        let content = fs::read_to_string(path)
            .context(format!("Failed to read host configuration from {}", path.display()))?;
        let config: Self = serde_json::from_str(&content)
            .context("Failed to deserialize host configuration JSON")?;
        Ok(config)
    }

    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize host configuration")?;
        fs::write(path, json)
            .context(format!("Failed to write host configuration to {}", path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = fs::Permissions::from_mode(0o600);
            let _ = fs::set_permissions(path, permissions);
        }

        Ok(())
    }

    pub fn save_default(&self) -> Result<()> {
        let path = Self::default_path()?;
        self.save_to_path(&path)
    }

    /// Create or update permanent PIN using PBKDF2-HMAC-SHA256 with 600,000 iterations.
    pub fn create_or_update(pin: &str) -> Result<Self> {
        let path = Self::default_path()?;
        let mut config = if path.exists() {
            Self::load_from_path(&path).unwrap_or_else(|_| Self::empty())
        } else {
            Self::empty()
        };

        config.set_pin(pin)?;
        config.save_to_path(&path)?;
        Ok(config)
    }

    fn empty() -> Self {
        Self {
            pin_salt_hex: String::new(),
            pin_hash_hex: String::new(),
            pin_iterations: PBKDF2_ITERATIONS,
            host_enabled: true,
            created_at: current_timestamp(),
            updated_at: current_timestamp(),
        }
    }

    /// Set a new permanent PIN. Derives PBKDF2-HMAC-SHA256 hash with random 32-byte salt.
    pub fn set_pin(&mut self, pin: &str) -> Result<()> {
        let trimmed = pin.trim();
        if trimmed.len() < 6 {
            bail!("PIN must be at least 6 digits/characters");
        }

        let mut salt = [0u8; 32];
        OsRng.fill_bytes(&mut salt);

        let mut hash = [0u8; 32];
        let iterations = NonZeroU32::new(PBKDF2_ITERATIONS).unwrap();
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            iterations,
            &salt,
            trimmed.as_bytes(),
            &mut hash,
        );

        self.pin_salt_hex = hex_encode(&salt);
        self.pin_hash_hex = hex_encode(&hash);
        self.pin_iterations = PBKDF2_ITERATIONS;
        self.updated_at = current_timestamp();

        salt.fill(0);
        hash.fill(0);

        Ok(())
    }

    /// Verify a provided PIN in constant time using PBKDF2-HMAC-SHA256.
    pub fn verify_pin(&self, provided_pin: &str) -> bool {
        let salt = match hex_decode(&self.pin_salt_hex) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let expected_hash = match hex_decode(&self.pin_hash_hex) {
            Ok(h) => h,
            Err(_) => return false,
        };

        let iterations = match NonZeroU32::new(self.pin_iterations) {
            Some(i) => i,
            None => return false,
        };

        pbkdf2::verify(
            pbkdf2::PBKDF2_HMAC_SHA256,
            iterations,
            &salt,
            provided_pin.trim().as_bytes(),
            &expected_hash,
        )
        .is_ok()
    }

    pub fn has_pin(&self) -> bool {
        !self.pin_hash_hex.is_empty() && !self.pin_salt_hex.is_empty()
    }
}

/// Rate-limit tracker for failed authentication attempts.
#[derive(Debug, Clone)]
struct FailedAttemptRecord {
    attempts: u32,
    locked_until: u64,
}

static FAILED_ATTEMPTS: LazyLock<Mutex<HashMap<String, FailedAttemptRecord>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static PENDING_PINS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Perform constant-time string comparison using subtle's ConstantTimeEq.
pub fn constant_time_eq_str(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    bool::from(a_bytes.ct_eq(b_bytes))
}

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
    host_config: Option<HostConfig>,
}

impl AuthManager {
    /// Load or initialize the authorized clients store from default config path.
    pub fn load_default() -> Result<Self> {
        let config_dir = dirs_config_dir().unwrap_or_else(|| PathBuf::from("./config"));
        let ward_dir = config_dir.join("ward");
        fs::create_dir_all(&ward_dir)
            .context(format!("Failed to create ward config directory at {}", ward_dir.display()))?;

        let store_path = ward_dir.join("authorized_clients.json");
        let host_config = HostConfig::load_default().ok();

        Self::load_from_path_with_config(store_path, host_config)
    }

    /// Load or initialize from a specific file path.
    pub fn load_from_path(store_path: PathBuf) -> Result<Self> {
        Self::load_from_path_with_config(store_path, None)
    }

    pub fn load_from_path_with_config(store_path: PathBuf, host_config: Option<HostConfig>) -> Result<Self> {
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
            host_config,
        })
    }

    pub fn set_host_config(&mut self, config: HostConfig) {
        self.host_config = Some(config);
    }

    pub fn host_config(&self) -> Option<&HostConfig> {
        self.host_config.as_ref()
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
            let _ = fs::set_permissions(path, permissions);
        }

        Ok(())
    }

    /// Check if a client device ID is already authorized.
    pub fn is_authorized(&self, device_id: &str) -> bool {
        if let Some(client) = self.store.clients.get(device_id) {
            if let Some(ref config) = self.host_config {
                if !config.host_enabled || client.paired_at < config.updated_at {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }

    /// Retrieve the authorized token for a paired device ID if authorized.
    pub fn get_authorized_token(&self, device_id: &str) -> Option<String> {
        if !self.is_authorized(device_id) {
            return None;
        }
        self.store.clients.get(device_id).map(|c| c.auth_token.clone())
    }

    /// Remove an authorized client device (e.g. unpair / cleanup).
    pub fn remove_authorized_client(&mut self, device_id: &str) -> Result<()> {
        self.store.clients.remove(device_id);
        Self::save_store(&self.store_path, &self.store)
    }

    /// Remove all authorized client devices.
    pub fn unpair_all(&mut self) -> Result<()> {
        self.store.clients.clear();
        Self::save_store(&self.store_path, &self.store)
    }

    /// List all currently authorized client devices.
    pub fn list_authorized_clients(&self) -> Vec<(String, String, u64)> {
        self.store
            .clients
            .iter()
            .map(|(id, c)| (id.clone(), c.device_name.clone(), c.last_seen_at))
            .collect()
    }

    /// Verify an existing client's auth token using constant-time comparison.
    pub fn verify_token(&mut self, device_id: &str, token: &str) -> bool {
        if let Some(ref config) = self.host_config {
            if !config.host_enabled {
                return false;
            }
        }

        if let Some(client) = self.store.clients.get_mut(device_id) {
            if let Some(ref config) = self.host_config {
                if client.paired_at < config.updated_at {
                    warn!(
                        "[WARD AUTH] Client '{}' ({}) token was paired before host PIN updated at {}; rejecting token",
                        client.device_name, device_id, config.updated_at
                    );
                    return false;
                }
            }

            if constant_time_eq_str(&client.auth_token, token.trim()) {
                client.last_seen_at = current_timestamp();
                let _ = Self::save_store(&self.store_path, &self.store);
                return true;
            }
        }
        false
    }

    /// Retrieve pending PIN if set (used by tests and OTP fallback).
    pub fn get_pending_pin(device_id: &str) -> Option<String> {
        let lock = PENDING_PINS.lock().unwrap();
        lock.get(device_id).cloned()
    }

    /// Verify the client using the permanent host PIN.
    ///
    /// Validates against PBKDF2 hash (or pending PIN) with rate-limiting, lockout cooldown, and audit logging.
    /// Never logs or displays the PIN.
    pub fn verify_permanent_pin(
        &mut self,
        device_id: &str,
        device_name: &str,
        provided_pin: &str,
    ) -> Result<String> {
        if let Some(ref config) = self.host_config {
            if !config.host_enabled {
                bail!("WARD Host is disabled by administrator. Remote access rejected.");
            }
        }

        let now = current_timestamp();
        let mut lock = FAILED_ATTEMPTS.lock().unwrap();
        let record = lock.entry(device_id.to_string()).or_insert(FailedAttemptRecord {
            attempts: 0,
            locked_until: 0,
        });

        // Check if currently in lockout cooldown
        if record.locked_until > now {
            let remaining = record.locked_until - now;
            warn!("[WARD AUDIT] Rejected authentication for locked-out device '{}' ({}) - {}s remaining", device_name, device_id, remaining);
            bail!("Too many failed authentication attempts. Device locked out for {} more seconds.", remaining);
        }

        // Verify against pending PIN (if set by test/OTP) or permanent PIN via PBKDF2
        let pending_match = {
            let p_lock = PENDING_PINS.lock().unwrap();
            p_lock.get(device_id).map(|p| constant_time_eq_str(p, provided_pin.trim())).unwrap_or(false)
        };

        let is_valid = if pending_match {
            true
        } else if let Some(ref config) = self.host_config {
            config.verify_pin(provided_pin)
        } else {
            false
        };

        if !is_valid {
            record.attempts += 1;
            let attempts = record.attempts;

            if attempts >= MAX_FAILED_PIN_ATTEMPTS {
                record.locked_until = now + LOCKOUT_DURATION_SECS;
                warn!(
                    "[WARD AUDIT] Max failed attempts exceeded (5/5) for device '{}' ({}). Locked out for {}s.",
                    device_name, device_id, LOCKOUT_DURATION_SECS
                );
                bail!(
                    "Too many failed authentication attempts (5/5). Device locked out for {} seconds.",
                    LOCKOUT_DURATION_SECS
                );
            } else {
                warn!(
                    "[WARD AUDIT] Invalid PIN attempt ({}/{}) for device '{}' ({})",
                    attempts, MAX_FAILED_PIN_ATTEMPTS, device_name, device_id
                );
                bail!(
                    "Invalid host PIN (attempt {}/{}).",
                    attempts, MAX_FAILED_PIN_ATTEMPTS
                );
            }
        }

        // Reset failure record and clear pending PIN upon successful authentication
        lock.remove(device_id);
        drop(lock);
        PENDING_PINS.lock().unwrap().remove(device_id);

        // Generate 32-byte cryptographically secure session auth token
        let mut token_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut token_bytes);
        let token = hex_encode(&token_bytes);
        token_bytes.fill(0);

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
            "[WARD AUDIT] Device '{}' ({}) successfully authenticated with permanent PIN and paired",
            device_name, device_id
        );

        Ok(token)
    }

    /// Legacy OTP PIN generator for backwards-compatible test fixtures.
    pub fn generate_pairing_pin(&mut self, device_id: &str, _device_name: &str) -> (String, String) {
        let mut random_bytes = [0u8; 4];
        OsRng.fill_bytes(&mut random_bytes);
        let num = u32::from_be_bytes(random_bytes) % 900_000 + 100_000;
        let pin = num.to_string();

        PENDING_PINS.lock().unwrap().insert(device_id.to_string(), pin.clone());

        let mut token_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut token_bytes);
        let token = hex_encode(&token_bytes);
        token_bytes.fill(0);

        (pin, token)
    }

    /// Legacy OTP PIN verification for backwards-compatible test fixtures.
    pub fn verify_pairing_pin(
        &mut self,
        device_id: &str,
        device_name: &str,
        provided_pin: &str,
    ) -> Result<String> {
        self.verify_permanent_pin(device_id, device_name, provided_pin)
    }
}

fn dirs_config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
        })
}

pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn hex_decode(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 {
        bail!("Hex string has odd length");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|e| anyhow::anyhow!("Invalid hex byte: {}", e))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pbkdf2_pin_derivation_and_verification() {
        let mut config = HostConfig::empty();
        config.set_pin("123456").expect("Failed to set PIN");

        assert!(config.has_pin());
        assert!(config.verify_pin("123456"));
        assert!(!config.verify_pin("000000"));
        assert!(!config.verify_pin("123457"));
    }

    #[test]
    fn test_permanent_pin_authentication_and_lockout() {
        let tmp_store = std::env::temp_dir().join(format!("ward_clients_test_{}.json", current_timestamp()));
        let mut auth = AuthManager::load_from_path(tmp_store.clone()).expect("Failed to create AuthManager");

        let mut config = HostConfig::empty();
        config.set_pin("654321").expect("Failed to set PIN");
        auth.set_host_config(config);

        let device_id = "test_phone_id_abc123";
        let device_name = "Pixel 9 Pro";

        assert!(!auth.is_authorized(device_id));

        // 4 failed attempts
        for i in 1..=4 {
            let res = auth.verify_permanent_pin(device_id, device_name, "000000");
            assert!(res.is_err());
            let msg = res.unwrap_err().to_string();
            assert!(msg.contains(&format!("attempt {}/5", i)));
        }

        // 5th failed attempt triggers lockout
        let fifth = auth.verify_permanent_pin(device_id, device_name, "000000");
        assert!(fifth.is_err());
        assert!(fifth.unwrap_err().to_string().contains("locked out"));

        // Even with the correct PIN, must be locked out
        let during_lockout = auth.verify_permanent_pin(device_id, device_name, "654321");
        assert!(during_lockout.is_err());
        assert!(during_lockout.unwrap_err().to_string().contains("locked out"));

        // Clean up
        let _ = fs::remove_file(tmp_store);
    }

    #[test]
    fn test_constant_time_eq_str() {
        assert!(constant_time_eq_str("abcdef123456", "abcdef123456"));
        assert!(!constant_time_eq_str("abcdef123456", "abcdef123457"));
        assert!(!constant_time_eq_str("short", "longer_string"));
        assert!(!constant_time_eq_str("", "non_empty"));
        assert!(constant_time_eq_str("", ""));
    }
}
