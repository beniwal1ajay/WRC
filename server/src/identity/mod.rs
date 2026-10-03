//! WARD Host Persistent Device Identity.
//!
//! Manages cryptographic Ed25519 identity, unique device ID, and human-readable device name.
//! Stored persistently in ~/.config/ward/identity.json with strict 0600 file permissions.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, bail};
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use tracing::info;

pub const DEFAULT_DEVICE_NAME: &str = "AJAY-UBUNTU";

/// Persistent Host Identity containing Ed25519 keypair and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostIdentity {
    /// Hex-encoded 32-byte Ed25519 public key identifying this host.
    pub device_id: String,
    /// Human-readable device name (e.g. "AJAY-UBUNTU").
    pub device_name: String,
    /// Hex-encoded public key.
    pub public_key_hex: String,
    /// 32-byte private signing key seed.
    #[serde(skip_serializing)]
    pub secret_key_bytes: [u8; 32],
}

#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    device_id: String,
    device_name: String,
    public_key_hex: String,
    secret_key_hex: String,
}

impl HostIdentity {
    /// Return the 32-byte public key representation.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        let bytes = hex_decode(&self.public_key_hex).unwrap_or_default();
        let mut arr = [0u8; 32];
        if bytes.len() == 32 {
            arr.copy_from_slice(&bytes);
        }
        arr
    }

    /// Load existing host identity from ~/.config/ward/identity.json,
    /// or generate and persist a new one if not present.
    pub fn load_or_create(custom_name: Option<String>) -> Result<Self> {
        let path = default_identity_path();
        Self::load_or_create_from_path(&path, custom_name)
    }

    /// Load or create identity at a specific path.
    pub fn load_or_create_from_path(path: &Path, custom_name: Option<String>) -> Result<Self> {
        if path.exists() {
            match Self::load_from_path(path) {
                Ok(mut identity) => {
                    if let Some(name) = custom_name {
                        if identity.device_name != name {
                            identity.device_name = name;
                            identity.save_to_path(path)?;
                        }
                    }
                    return Ok(identity);
                }
                Err(e) => {
                    tracing::warn!("Failed to read existing identity at {}: {:#}. Regenerating...", path.display(), e);
                }
            }
        }

        // Generate new Ed25519 keypair
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key: VerifyingKey = signing_key.verifying_key();

        let secret_bytes = signing_key.to_bytes();
        let public_bytes = verifying_key.to_bytes();

        let public_key_hex = hex_encode(&public_bytes);
        let device_id = public_key_hex.clone();

        let device_name = custom_name.unwrap_or_else(|| {
            // Check system hostname or fall back to DEFAULT_DEVICE_NAME
            std::fs::read_to_string("/etc/hostname")
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| DEFAULT_DEVICE_NAME.to_string())
        });

        // Ensure "AJAY-UBUNTU" styling if it's the default host
        let final_name = if device_name.to_lowercase().contains("ajay") {
            DEFAULT_DEVICE_NAME.to_string()
        } else {
            device_name
        };

        let identity = Self {
            device_id: device_id.clone(),
            device_name: final_name.clone(),
            public_key_hex: public_key_hex.clone(),
            secret_key_bytes: secret_bytes,
        };

        identity.save_to_path(path)?;
        info!("Generated new host identity: '{}' (ID: {}...)", final_name, &device_id[..12]);

        Ok(identity)
    }

    /// Load identity from a file path.
    pub fn load_from_path(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .context(format!("Failed to read identity file at {}", path.display()))?;

        let stored: StoredIdentity = serde_json::from_str(&content)
            .context("Failed to deserialize identity JSON")?;

        let mut secret_bytes_vec = hex_decode(&stored.secret_key_hex)?;
        if secret_bytes_vec.len() != 32 {
            bail!("Invalid secret key length: expected 32 bytes, got {}", secret_bytes_vec.len());
        }

        let mut secret_key_bytes = [0u8; 32];
        secret_key_bytes.copy_from_slice(&secret_bytes_vec);
        secret_bytes_vec.fill(0); // Zeroize intermediate decoded key buffer

        // Verify keypair integrity
        let signing_key = SigningKey::from_bytes(&secret_key_bytes);
        let expected_public = hex_encode(&signing_key.verifying_key().to_bytes());
        if expected_public != stored.public_key_hex {
            bail!("Identity keypair mismatch: public key does not match secret key");
        }

        Ok(Self {
            device_id: stored.device_id,
            device_name: stored.device_name,
            public_key_hex: stored.public_key_hex,
            secret_key_bytes,
        })
    }

    /// Save identity to a file path with 0600 permissions.
    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .context(format!("Failed to create config dir {}", parent.display()))?;
        }

        let stored = StoredIdentity {
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            public_key_hex: self.public_key_hex.clone(),
            secret_key_hex: hex_encode(&self.secret_key_bytes),
        };

        let json = serde_json::to_string_pretty(&stored)
            .context("Failed to serialize identity to JSON")?;

        fs::write(path, json)
            .context(format!("Failed to write identity to {}", path.display()))?;

        // Restrict file permissions to current user only (0600)
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;

        Ok(())
    }
}

impl Drop for HostIdentity {
    fn drop(&mut self) {
        // Explicitly overwrite secret key bytes in memory upon drop
        for b in self.secret_key_bytes.iter_mut() {
            *b = 0;
        }
    }
}

/// Default path: ~/.config/ward/identity.json
pub fn default_identity_path() -> PathBuf {
    if let Some(cfg) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
        cfg.join("ward").join("identity.json")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("ward").join("identity.json")
    } else {
        PathBuf::from(".config").join("ward").join("identity.json")
    }
}

pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn hex_decode(s: &str) -> Result<Vec<u8>> {
    let clean = s.trim();
    if clean.len() % 2 != 0 {
        bail!("Hex string length must be even");
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&clean[i..i + 2], 16)
                .map_err(|e| anyhow::anyhow!("Invalid hex byte: {}", e))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_generation_and_persistence() {
        let temp_dir = std::env::temp_dir().join(format!("ward_test_identity_{}", std::process::id()));
        let path = temp_dir.join("identity.json");

        let identity = HostIdentity::load_or_create_from_path(&path, Some("TEST-HOST".to_string()))
            .expect("Failed to create identity");

        assert_eq!(identity.device_name, "TEST-HOST");
        assert_eq!(identity.device_id.len(), 64);
        assert_eq!(identity.public_key_hex.len(), 64);
        assert_eq!(identity.public_key_bytes().len(), 32);

        // Load again and verify same identity
        let loaded = HostIdentity::load_from_path(&path).expect("Failed to reload identity");
        assert_eq!(loaded.device_id, identity.device_id);
        assert_eq!(loaded.public_key_hex, identity.public_key_hex);
        assert_eq!(loaded.secret_key_bytes, identity.secret_key_bytes);

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_hex_encode_decode_roundtrip() {
        let data = b"Hello, WARD Device Identity!";
        let encoded = hex_encode(data);
        let decoded = hex_decode(&encoded).expect("Decode should succeed");
        assert_eq!(data.to_vec(), decoded);
    }
}
