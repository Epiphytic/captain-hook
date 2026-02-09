use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::decision::{Decision, DecisionRecord};
use crate::error::Result;
use crate::sanitize::SanitizePipeline;
use crate::scope::ScopeLevel;

use super::{SecretFinding, StorageBackend};

/// JSONL-based storage implementation.
pub struct JsonlStorage {
    project_root: PathBuf,
    global_root: PathBuf,
    org_name: Option<String>,
}

impl JsonlStorage {
    pub fn new(project_root: PathBuf, global_root: PathBuf, org_name: Option<String>) -> Self {
        Self {
            project_root,
            global_root,
            org_name,
        }
    }

    /// Resolve the directory path for a given scope.
    fn scope_dir(&self, scope: ScopeLevel) -> PathBuf {
        match scope {
            ScopeLevel::Project => self.project_root.join("rules"),
            ScopeLevel::Org => {
                let org = self.org_name.as_deref().unwrap_or("default");
                self.global_root.join("org").join(org).join("rules")
            }
            ScopeLevel::User => self.global_root.join("user"),
            ScopeLevel::Role => self.project_root.join("rules"),
        }
    }

    /// Resolve the JSONL file path for a given scope and decision type.
    fn jsonl_path(&self, scope: ScopeLevel, decision: Decision) -> PathBuf {
        let dir = self.scope_dir(scope);
        let filename = match decision {
            Decision::Allow => "allow.jsonl",
            Decision::Deny => "deny.jsonl",
            Decision::Ask => "ask.jsonl",
        };
        dir.join(filename)
    }

    /// Read all decision records from a JSONL file.
    fn read_jsonl_file(path: &Path) -> Result<Vec<DecisionRecord>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for (line_num, line) in reader.lines().enumerate() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<DecisionRecord>(trimmed) {
                Ok(record) => records.push(record),
                Err(e) => {
                    tracing::warn!(
                        "skipping malformed line {} in {}: {}",
                        line_num + 1,
                        path.display(),
                        e
                    );
                }
            }
        }

        Ok(records)
    }

    /// Append a record to a JSONL file, creating parent dirs if needed.
    fn append_jsonl_file(path: &Path, record: &DecisionRecord) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        let json = serde_json::to_string(record)?;
        writeln!(file, "{}", json)?;
        Ok(())
    }

    /// Rewrite a JSONL file, keeping only records that match a predicate.
    fn filter_jsonl_file<F>(path: &Path, predicate: F) -> Result<()>
    where
        F: Fn(&DecisionRecord) -> bool,
    {
        if !path.exists() {
            return Ok(());
        }
        let records = Self::read_jsonl_file(path)?;
        let kept: Vec<&DecisionRecord> = records.iter().filter(|r| predicate(r)).collect();

        // Write the filtered records back
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(path)?;
        for record in kept {
            let json = serde_json::to_string(record)?;
            writeln!(file, "{}", json)?;
        }
        Ok(())
    }
}

impl StorageBackend for JsonlStorage {
    fn load_decisions(&self, scope: ScopeLevel) -> Result<Vec<DecisionRecord>> {
        let mut all = Vec::new();
        for decision in &[Decision::Allow, Decision::Deny, Decision::Ask] {
            let path = self.jsonl_path(scope, *decision);
            let records = Self::read_jsonl_file(&path)?;
            all.extend(records);
        }
        Ok(all)
    }

    fn load_decisions_for_role(
        &self,
        scope: ScopeLevel,
        role: &str,
    ) -> Result<Vec<DecisionRecord>> {
        let all = self.load_decisions(scope)?;
        Ok(all
            .into_iter()
            .filter(|r| r.key.role == role || r.key.role == "*")
            .collect())
    }

    fn save_decision(&self, record: &DecisionRecord) -> Result<()> {
        let path = self.jsonl_path(record.scope, record.decision);
        Self::append_jsonl_file(&path, record)
    }

    fn invalidate_role(&self, scope: ScopeLevel, role: &str) -> Result<()> {
        for decision in &[Decision::Allow, Decision::Deny, Decision::Ask] {
            let path = self.jsonl_path(scope, *decision);
            Self::filter_jsonl_file(&path, |r| r.key.role != role)?;
        }
        Ok(())
    }

    fn invalidate_all(&self, scope: ScopeLevel) -> Result<()> {
        for decision in &[Decision::Allow, Decision::Deny, Decision::Ask] {
            let path = self.jsonl_path(scope, *decision);
            if path.exists() {
                fs::remove_file(&path)?;
            }
        }
        Ok(())
    }

    fn rebuild_index(&self, _scope: ScopeLevel) -> Result<()> {
        // Index rebuild is handled by the embedding/jaccard tiers, not storage.
        // This is a no-op placeholder that the cascade engine will call into
        // the appropriate tier's rebuild method.
        Ok(())
    }

    fn scan_for_secrets(&self, path: &Path) -> Result<Vec<SecretFinding>> {
        let pipeline = SanitizePipeline::default_pipeline();
        let mut findings = Vec::new();

        // Scan all JSONL files at the given path
        let entries = if path.is_dir() {
            fs::read_dir(path)?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
                .map(|e| e.path())
                .collect::<Vec<_>>()
        } else if path.exists() {
            vec![path.to_path_buf()]
        } else {
            return Ok(findings);
        };

        for file_path in entries {
            let file = fs::File::open(&file_path)?;
            let reader = BufReader::new(file);

            for (line_num, line) in reader.lines().enumerate() {
                let line = line?;
                let sanitized = pipeline.sanitize(&line);
                if sanitized != line {
                    findings.push(SecretFinding {
                        file: file_path.clone(),
                        line: line_num + 1,
                        description: "potential secret detected in stored decision".into(),
                        detector: "sanitize-pipeline".into(),
                    });
                }
            }
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::{CacheKey, DecisionMetadata, DecisionTier};
    use chrono::Utc;
    use tempfile::TempDir;

    fn make_record(decision: Decision, role: &str) -> DecisionRecord {
        DecisionRecord {
            key: CacheKey {
                sanitized_input: "test command".into(),
                tool: "Bash".into(),
                role: role.into(),
            },
            decision,
            metadata: DecisionMetadata {
                tier: DecisionTier::Human,
                confidence: 1.0,
                reason: "test".into(),
                matched_key: None,
                similarity_score: None,
            },
            timestamp: Utc::now(),
            scope: ScopeLevel::Project,
            file_path: None,
            session_id: "test-session".into(),
        }
    }

    #[test]
    fn test_save_and_load() {
        let tmp = TempDir::new().unwrap();
        let storage = JsonlStorage::new(tmp.path().to_path_buf(), tmp.path().join("global"), None);

        let record = make_record(Decision::Allow, "coder");
        storage.save_decision(&record).unwrap();

        let loaded = storage.load_decisions(ScopeLevel::Project).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].decision, Decision::Allow);
    }

    #[test]
    fn test_load_for_role() {
        let tmp = TempDir::new().unwrap();
        let storage = JsonlStorage::new(tmp.path().to_path_buf(), tmp.path().join("global"), None);

        storage
            .save_decision(&make_record(Decision::Allow, "coder"))
            .unwrap();
        storage
            .save_decision(&make_record(Decision::Allow, "tester"))
            .unwrap();

        let coder_records = storage
            .load_decisions_for_role(ScopeLevel::Project, "coder")
            .unwrap();
        assert_eq!(coder_records.len(), 1);
    }

    #[test]
    fn test_invalidate_role() {
        let tmp = TempDir::new().unwrap();
        let storage = JsonlStorage::new(tmp.path().to_path_buf(), tmp.path().join("global"), None);

        storage
            .save_decision(&make_record(Decision::Allow, "coder"))
            .unwrap();
        storage
            .save_decision(&make_record(Decision::Allow, "tester"))
            .unwrap();

        storage
            .invalidate_role(ScopeLevel::Project, "coder")
            .unwrap();

        let loaded = storage.load_decisions(ScopeLevel::Project).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].key.role, "tester");
    }

    #[test]
    fn test_invalidate_all() {
        let tmp = TempDir::new().unwrap();
        let storage = JsonlStorage::new(tmp.path().to_path_buf(), tmp.path().join("global"), None);

        storage
            .save_decision(&make_record(Decision::Allow, "coder"))
            .unwrap();
        storage
            .save_decision(&make_record(Decision::Deny, "tester"))
            .unwrap();

        storage.invalidate_all(ScopeLevel::Project).unwrap();

        let loaded = storage.load_decisions(ScopeLevel::Project).unwrap();
        assert_eq!(loaded.len(), 0);
    }
}
