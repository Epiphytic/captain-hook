use globset::GlobSet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::{CaptainHookError, Result};

/// A role definition from `roles.yml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleDefinition {
    /// Role name (e.g., "coder", "tester", "maintainer").
    pub name: String,

    /// Natural language description of the role.
    pub description: String,

    /// Deterministic path policies for this role.
    pub paths: PathPolicyConfig,
}

/// Raw path policy from YAML (string globs, before compilation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathPolicyConfig {
    pub allow_write: Vec<String>,
    pub deny_write: Vec<String>,
    pub allow_read: Vec<String>,
}

/// Compiled path policy -- globset instances ready for matching.
/// GlobSet doesn't implement Debug, so we implement it manually.
pub struct CompiledPathPolicy {
    pub allow_write: GlobSet,
    pub deny_write: GlobSet,
    pub allow_read: GlobSet,
    pub sensitive_ask_write: GlobSet,
}

impl std::fmt::Debug for CompiledPathPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledPathPolicy")
            .field("allow_write", &"<GlobSet>")
            .field("deny_write", &"<GlobSet>")
            .field("allow_read", &"<GlobSet>")
            .field("sensitive_ask_write", &"<GlobSet>")
            .finish()
    }
}

impl CompiledPathPolicy {
    /// Compile a PathPolicyConfig into GlobSet instances.
    pub fn compile(config: &PathPolicyConfig, sensitive_patterns: &[String]) -> Result<Self> {
        let allow_write = build_globset(&config.allow_write)?;
        let deny_write = build_globset(&config.deny_write)?;
        let allow_read = build_globset(&config.allow_read)?;
        let sensitive_ask_write = build_globset(sensitive_patterns)?;

        Ok(Self {
            allow_write,
            deny_write,
            allow_read,
            sensitive_ask_write,
        })
    }
}

fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        let glob = globset::Glob::new(pattern).map_err(|e| CaptainHookError::GlobPattern {
            pattern: pattern.clone(),
            reason: e.to_string(),
        })?;
        builder.add(glob);
    }
    builder.build().map_err(|e| CaptainHookError::GlobPattern {
        pattern: String::new(),
        reason: e.to_string(),
    })
}

/// Roles configuration loaded from roles.yml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolesConfig {
    pub roles: HashMap<String, RoleDefinition>,
}

impl RolesConfig {
    /// Load roles from a YAML file.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                roles: HashMap::new(),
            });
        }
        let contents = std::fs::read_to_string(path)?;
        serde_yaml::from_str(&contents).map_err(|e| CaptainHookError::ConfigParse {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })
    }

    /// Load roles from the project root. Checks `.captain-hook/roles.yml`.
    pub fn load_project(project_root: &Path) -> Result<Self> {
        let path = project_root.join(".captain-hook").join("roles.yml");
        Self::load_from(&path)
    }

    /// Look up a role by name.
    pub fn get_role(&self, name: &str) -> Option<&RoleDefinition> {
        self.roles.get(name)
    }
}
