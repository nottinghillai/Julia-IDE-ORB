//! Agent registry - manages built-in and custom agents
#![allow(dead_code)]
use anyhow::{Context, Result};
use paths::data_dir;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use rust_embed::RustEmbed;
use zed_env_vars::ZED_STATELESS;

#[derive(RustEmbed)]
#[folder = "../../assets/agents/builtin"]
struct BuiltinAgentAssets;

/// Agent metadata from agent.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub agent_type: String,
    pub version: String,
    pub description: Option<String>,
    pub metadata: Option<AgentMetadataFields>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadataFields {
    pub icon: Option<String>,
    pub telemetry_id: Option<String>,
}

/// Manifest entry for an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifestEntry {
    pub source: String, // "builtin" or "custom"
    pub version: String,
    pub path: String,
    pub checksum: String,
    pub last_updated: String,
    pub user_modified: bool,
}

/// Agent registry manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRegistryManifest {
    pub version: String,
    pub source_version: String,
    pub agents: HashMap<String, AgentManifestEntry>,
}

impl AgentRegistryManifest {
    pub fn new() -> Self {
        Self {
            version: "1.0.0".to_string(),
            source_version: "1.0.0".to_string(),
            agents: HashMap::new(),
        }
    }
}

/// Agent registry
pub struct AgentRegistry {
    manifest_path: PathBuf,
    agents_dir: PathBuf,
}

impl AgentRegistry {
    /// Initialize the agent registry
    pub fn new() -> Result<Self> {
        if *ZED_STATELESS {
            log::warn!("Agent registry disabled in stateless mode");
            // Return a dummy registry that won't do anything
            return Ok(Self {
                manifest_path: PathBuf::new(),
                agents_dir: PathBuf::new(),
            });
        }

        let agents_dir = data_dir().join("agents");
        let manifest_path = agents_dir.join("index.json");

        Ok(Self {
            manifest_path,
            agents_dir,
        })
    }

    /// Initialize and seed the registry
    pub fn initialize(&self) -> Result<()> {
        if *ZED_STATELESS {
            return Ok(());
        }

        // Create agents directory if it doesn't exist
        fs::create_dir_all(&self.agents_dir)?;
        fs::create_dir_all(self.agents_dir.join("builtin"))?;
        fs::create_dir_all(self.agents_dir.join("custom"))?;

        // Load or create manifest
        let mut manifest = if self.manifest_path.exists() {
            let content = fs::read_to_string(&self.manifest_path)?;
            serde_json::from_str(&content)
                .context("Failed to parse agent registry manifest")?
        } else {
            AgentRegistryManifest::new()
        };

        // Seed built-in agents if needed
        self.seed_builtin_agents(&mut manifest)?;

        // Detect user modifications before applying upgrades
        self.detect_user_modifications(&mut manifest)?;

        // Check for upgrades
        self.check_and_upgrade(&mut manifest)?;

        // Save manifest
        self.save_manifest(&manifest)?;

        Ok(())
    }

    /// Seed built-in agents from source to data directory
    fn seed_builtin_agents(&self, manifest: &mut AgentRegistryManifest) -> Result<()> {
        let builtin_dest = self.agents_dir.join("builtin");

        for agent_id in Self::builtin_agent_ids() {
            let dest_path = builtin_dest.join(&agent_id);

            // Check if agent already exists in manifest
            let needs_seeding = if let Some(_existing_entry) = manifest.agents.get(&agent_id) {
                // Agent exists, check if files are present
                !dest_path.exists()
            } else {
                // New agent, needs seeding
                true
            };

            if needs_seeding {
                log::info!("Seeding agent: {}", agent_id);
                self.copy_agent_from_assets(&agent_id, &dest_path)?;
                
                // Compute checksum
                let checksum = Self::embedded_agent_checksum(&agent_id)?;
                let now = chrono::Utc::now().to_rfc3339();

                manifest.agents.insert(
                    agent_id.clone(),
                    AgentManifestEntry {
                        source: "builtin".to_string(),
                        version: "1.0.0".to_string(),
                        path: format!("builtin/{}", agent_id),
                        checksum,
                        last_updated: now,
                        user_modified: false,
                    },
                );
            }
        }

        Ok(())
    }

    /// Detect user modifications to builtin agents
    fn detect_user_modifications(
        &self,
        manifest: &mut AgentRegistryManifest,
    ) -> Result<()> {
        for (_agent_id, entry) in manifest.agents.iter_mut() {
            if entry.source != "builtin" {
                continue;
            }

            let dest_path = self.agents_dir.join(&entry.path);
            if !dest_path.exists() {
                entry.user_modified = false;
                continue;
            }

            let dest_checksum = self.compute_directory_checksum(&dest_path)?;
            if dest_checksum != entry.checksum {
                entry.user_modified = true;
            }
        }

        Ok(())
    }

    /// Check for upgrades and apply them
    fn check_and_upgrade(&self, manifest: &mut AgentRegistryManifest) -> Result<()> {
        let builtin_dest = self.agents_dir.join("builtin");

        for agent_id in Self::builtin_agent_ids() {
            // Compute source checksum
            let source_checksum = Self::embedded_agent_checksum(&agent_id)?;

            // Check manifest
            if let Some(manifest_entry) = manifest.agents.get_mut(&agent_id) {
                if manifest_entry.source == "builtin" {
                    if manifest_entry.checksum != source_checksum {
                        // Source has changed
                        if manifest_entry.user_modified {
                            log::warn!(
                                "Agent {} has been modified by user, skipping upgrade from source",
                                agent_id
                            );
                        } else {
                            // Upgrade from source
                            log::info!("Upgrading agent: {}", agent_id);
                            let dest_path = builtin_dest.join(&agent_id);
                            self.copy_agent_from_assets(&agent_id, &dest_path)?;
                            
                            manifest_entry.checksum = source_checksum;
                            manifest_entry.last_updated = chrono::Utc::now().to_rfc3339();
                            manifest_entry.user_modified = false;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Copy agent directory from embedded assets to destination
    fn copy_agent_from_assets(&self, agent_id: &str, dest: &Path) -> Result<()> {
        if dest.exists() {
            fs::remove_dir_all(dest)?;
        }
        fs::create_dir_all(dest)?;

        let prefix = format!("{agent_id}/");
        for asset_path in BuiltinAgentAssets::iter()
            .filter(|path| path.starts_with(&prefix))
            .map(|path| path.to_string())
        {
            let relative_path = asset_path.trim_start_matches(&prefix);
            if relative_path.is_empty() {
                continue;
            }
            let dest_file = dest.join(relative_path);
            if let Some(parent) = dest_file.parent() {
                fs::create_dir_all(parent)?;
            }
            let data = BuiltinAgentAssets::get(&asset_path)
                .ok_or_else(|| anyhow::anyhow!("Missing embedded agent asset: {}", asset_path))?;
            fs::write(dest_file, data.data.as_ref())?;
        }

        Ok(())
    }

    /// Compute SHA256 checksum of an embedded agent directory
    fn embedded_agent_checksum(agent_id: &str) -> Result<String> {
        let mut hasher = Sha256::new();
        let prefix = format!("{agent_id}/");
        let mut entries = BuiltinAgentAssets::iter()
            .filter(|path| path.starts_with(&prefix))
            .map(|path| path.to_string())
            .collect::<Vec<_>>();
        entries.sort();

        for asset_path in entries {
            let data = BuiltinAgentAssets::get(&asset_path)
                .ok_or_else(|| anyhow::anyhow!("Missing embedded agent asset: {}", asset_path))?;
            hasher.update(data.data.as_ref());
        }

        Ok(hex::encode(hasher.finalize()))
    }

    /// Recursively hash directory contents on disk
    fn compute_directory_checksum(&self, path: &Path) -> Result<String> {
        let mut hasher = Sha256::new();
        self.hash_directory_fs(path, &mut hasher)?;
        Ok(hex::encode(hasher.finalize()))
    }

    fn hash_directory_fs(&self, path: &Path, hasher: &mut Sha256) -> Result<()> {
        let entries = fs::read_dir(path)?;
        let mut entries: Vec<_> = entries.collect::<std::io::Result<_>>()?;
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let entry_path = entry.path();
            if entry_path.is_file() {
                let content = fs::read(&entry_path)?;
                hasher.update(&content);
            } else if entry_path.is_dir() {
                self.hash_directory_fs(&entry_path, hasher)?;
            }
        }

        Ok(())
    }

    /// Load agent metadata from agent.toml
    pub fn load_agent_metadata(&self, agent_id: &str) -> Result<Option<AgentMetadata>> {
        if *ZED_STATELESS {
            return Ok(None);
        }

        // Try builtin first
        let builtin_path = self.agents_dir.join("builtin").join(agent_id).join("agent.toml");
        if builtin_path.exists() {
            let content = fs::read_to_string(&builtin_path)?;
            let metadata: AgentMetadata = toml::from_str(&content)?;
            return Ok(Some(metadata));
        }

        // Try custom
        let custom_path = self.agents_dir.join("custom").join(agent_id).join("agent.toml");
        if custom_path.exists() {
            let content = fs::read_to_string(&custom_path)?;
            let metadata: AgentMetadata = toml::from_str(&content)?;
            return Ok(Some(metadata));
        }

        Ok(None)
    }

    /// Save manifest to disk
    fn save_manifest(&self, manifest: &AgentRegistryManifest) -> Result<()> {
        if *ZED_STATELESS {
            return Ok(());
        }

        let content = serde_json::to_string_pretty(manifest)?;
        fs::write(&self.manifest_path, content)?;
        Ok(())
    }

    /// Get agent path
    pub fn get_agent_path(&self, agent_id: &str) -> Option<PathBuf> {
        if *ZED_STATELESS {
            return None;
        }

        // Try builtin first
        let builtin_path = self.agents_dir.join("builtin").join(agent_id);
        if builtin_path.exists() {
            return Some(builtin_path);
        }

        // Try custom
        let custom_path = self.agents_dir.join("custom").join(agent_id);
        if custom_path.exists() {
            return Some(custom_path);
        }

        None
    }

    /// Load system prompt for an agent
    pub fn load_system_prompt(&self, agent_id: &str) -> Result<Option<String>> {
        if *ZED_STATELESS {
            return Ok(None);
        }

        // Try builtin first
        let builtin_path = self.agents_dir.join("builtin").join(agent_id).join("system_prompt.md");
        if builtin_path.exists() {
            let content = fs::read_to_string(&builtin_path)?;
            return Ok(Some(content));
        }

        // Try custom
        let custom_path = self.agents_dir.join("custom").join(agent_id).join("system_prompt.md");
        if custom_path.exists() {
            let content = fs::read_to_string(&custom_path)?;
            return Ok(Some(content));
        }

        // Try embedded assets as fallback
        let asset_path = format!("{}/system_prompt.md", agent_id);
        if let Some(asset) = BuiltinAgentAssets::get(&asset_path) {
            let content = std::str::from_utf8(asset.data.as_ref())
                .context("Failed to decode system prompt from embedded asset")?;
            return Ok(Some(content.to_string()));
        }

        Ok(None)
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self {
            manifest_path: PathBuf::new(),
            agents_dir: PathBuf::new(),
        })
    }
}

impl AgentRegistry {
    fn builtin_agent_ids() -> Vec<String> {
        let mut ids = HashSet::new();
        for asset in BuiltinAgentAssets::iter() {
            if let Some((agent_id, _)) = asset.split_once('/') {
                if !agent_id.is_empty() {
                    ids.insert(agent_id.to_string());
                }
            }
        }
        let mut ids: Vec<_> = ids.into_iter().collect();
        ids.sort();
        ids
    }
}
