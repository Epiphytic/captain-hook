use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{HookwiseError, Result};

/// Top-level project policy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyConfig {
    /// Paths that default to `ask` regardless of role.
    #[serde(default)]
    pub sensitive_paths: SensitivePathConfig,

    /// Confidence thresholds per scope level.
    #[serde(default)]
    pub confidence: ConfidenceConfig,

    /// Similarity thresholds for Jaccard and embedding tiers.
    #[serde(default)]
    pub similarity: SimilarityConfig,

    /// Human decision timeout in seconds. Default: 60.
    #[serde(default = "default_human_timeout")]
    pub human_timeout_secs: u64,

    /// Registration wait timeout in seconds. Default: 5.
    #[serde(default = "default_registration_timeout")]
    pub registration_timeout_secs: u64,

    /// Supervisor backend configuration.
    #[serde(default)]
    pub supervisor: SupervisorConfig,
}

fn default_human_timeout() -> u64 {
    60
}
fn default_registration_timeout() -> u64 {
    5
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            sensitive_paths: SensitivePathConfig::default(),
            confidence: ConfidenceConfig::default(),
            similarity: SimilarityConfig::default(),
            human_timeout_secs: 60,
            registration_timeout_secs: 5,
            supervisor: SupervisorConfig::default(),
        }
    }
}

impl PolicyConfig {
    /// Load policy from a YAML file. Returns default if file doesn't exist.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)?;
        serde_yaml::from_str(&contents).map_err(|e| HookwiseError::ConfigParse {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })
    }

    /// Load policy from the project root. Checks `.hookwise/policy.yml`.
    pub fn load_project(project_root: &Path) -> Result<Self> {
        let path = project_root.join(".hookwise").join("policy.yml");
        Self::load_from(&path)
    }
}

/// Sensitive path configuration -- paths that default to `ask`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitivePathConfig {
    /// Glob patterns for paths that trigger `ask` on write.
    pub ask_write: Vec<String>,
}

impl Default for SensitivePathConfig {
    fn default() -> Self {
        Self {
            ask_write: vec![
                ".claude/**".into(),
                ".hookwise/**".into(),
                ".env*".into(),
                "**/.env*".into(),
                ".git/hooks/**".into(),
                "**/secrets/**".into(),
                "~/.claude/**".into(),
                "~/.config/**".into(),
            ],
        }
    }
}

/// Confidence thresholds per scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceConfig {
    pub org: f64,
    pub project: f64,
    pub user: f64,
}

impl Default for ConfidenceConfig {
    fn default() -> Self {
        Self {
            org: 0.9,
            project: 0.7,
            user: 0.6,
        }
    }
}

/// Similarity thresholds for Tier 2a (Jaccard) and Tier 2b (embedding).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarityConfig {
    pub jaccard_threshold: f64,
    pub embedding_threshold: f64,
    pub jaccard_min_tokens: usize,
}

impl Default for SimilarityConfig {
    fn default() -> Self {
        Self {
            jaccard_threshold: 0.7,
            embedding_threshold: 0.85,
            jaccard_min_tokens: 3,
        }
    }
}

/// Supervisor backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "backend")]
pub enum SupervisorConfig {
    #[serde(rename = "socket")]
    Socket { socket_path: Option<PathBuf> },
    #[serde(rename = "api")]
    Api {
        api_base_url: Option<String>,
        model: Option<String>,
        max_tokens: Option<u32>,
    },
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self::Socket { socket_path: None }
    }
}

/// Global hookwise configuration from `~/.config/hookwise/config.yml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    pub supervisor: SupervisorConfig,
    pub api_key: Option<String>,
    pub embedding_model: Option<String>,
}

impl GlobalConfig {
    /// Load global config. Returns None if not present.
    pub fn load() -> Result<Option<Self>> {
        let home = super::dirs_global();
        let path = home.join("config.yml");
        if !path.exists() {
            return Ok(None);
        }
        let contents = std::fs::read_to_string(&path)?;
        let config: Self =
            serde_yaml::from_str(&contents).map_err(|e| HookwiseError::ConfigParse {
                path: path.clone(),
                reason: e.to_string(),
            })?;
        Ok(Some(config))
    }
}
