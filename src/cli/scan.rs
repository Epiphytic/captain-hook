use std::path::PathBuf;

use crate::error::Result;
use crate::sanitize::SanitizePipeline;

/// Pre-commit secret scan on staged files or a specified path.
pub async fn run(staged: bool, path: Option<&str>) -> Result<()> {
    let pipeline = SanitizePipeline::default_pipeline();
    let mut total_findings = 0;

    if staged {
        // Scan git staged files
        let output = std::process::Command::new("git")
            .args(["diff", "--cached", "--name-only"])
            .output()?;

        if !output.status.success() {
            eprintln!("captain-hook: failed to get staged files (not a git repo?)");
            std::process::exit(1);
        }

        let file_list = String::from_utf8_lossy(&output.stdout);
        let files: Vec<&str> = file_list.lines().filter(|l| !l.is_empty()).collect();

        if files.is_empty() {
            eprintln!("captain-hook: no staged files to scan.");
            return Ok(());
        }

        eprintln!("captain-hook: scanning {} staged file(s)...", files.len());

        for file in files {
            let findings = scan_file(&pipeline, file)?;
            total_findings += findings;
        }
    } else if let Some(path) = path {
        let path_buf = PathBuf::from(path);
        if path_buf.is_dir() {
            // Scan all files in directory recursively
            eprintln!("captain-hook: scanning directory {}...", path);
            total_findings += scan_dir(&pipeline, &path_buf)?;
        } else if path_buf.is_file() {
            eprintln!("captain-hook: scanning file {}...", path);
            total_findings += scan_file(&pipeline, path)?;
        } else {
            eprintln!("captain-hook: path not found: {}", path);
            std::process::exit(1);
        }
    } else {
        // Scan .captain-hook/rules/ by default
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let rules_dir = cwd.join(".captain-hook").join("rules");

        if rules_dir.exists() {
            eprintln!("captain-hook: scanning rules directory...");
            total_findings += scan_dir(&pipeline, &rules_dir)?;
        } else {
            eprintln!(
                "captain-hook: no .captain-hook/rules/ found. Use --staged or provide a path."
            );
            std::process::exit(1);
        }
    }

    if total_findings > 0 {
        eprintln!(
            "\ncaptain-hook: {} potential secret(s) found. Aborting.",
            total_findings
        );
        std::process::exit(1);
    } else {
        eprintln!("captain-hook: scan clean -- no secrets detected.");
    }

    Ok(())
}

/// Scan a single file for secrets. Returns the number of findings.
fn scan_file(pipeline: &SanitizePipeline, path: &str) -> Result<usize> {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(0), // Skip binary/unreadable files
    };

    let mut findings = 0;

    for (line_num, line) in contents.lines().enumerate() {
        let sanitized = pipeline.sanitize(line);
        if sanitized != line {
            findings += 1;
            eprintln!("  {}:{}: potential secret detected", path, line_num + 1);
        }
    }

    Ok(findings)
}

/// Scan a directory recursively for secrets. Returns the number of findings.
fn scan_dir(pipeline: &SanitizePipeline, dir: &PathBuf) -> Result<usize> {
    let mut total = 0;

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Skip hidden directories
            if path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            {
                continue;
            }
            total += scan_dir(pipeline, &path)?;
        } else if path.is_file() {
            total += scan_file(pipeline, &path.to_string_lossy())?;
        }
    }

    Ok(total)
}
