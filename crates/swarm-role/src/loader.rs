//! Role loader.
//!
//! Loads role files from a directory, parses them, normalizes them, validates
//! them, applies the organizational hierarchy, and registers them in a
//! [`RoleRegistry`].

use std::path::{Path, PathBuf};

use crate::error::{RoleError, RoleResult};
use crate::hierarchy::RoleHierarchy;
use crate::model::RoleSpec;
use crate::normalizer::RoleNormalizer;
use crate::parser::RoleParser;
use crate::registry::RoleRegistry;
use crate::validator::{RoleValidator, ValidationSeverity};

/// Loader behavior toggles for role discovery and validation.
#[derive(Debug, Clone, Copy, Default)]
pub struct RoleLoadOptions {
    /// Treat validation warnings as blocking issues instead of soft diagnostics.
    pub treat_warnings_as_errors: bool,
}

/// Result of loading a single role file.
#[derive(Debug)]
pub struct RoleLoadResult {
    /// Path of the loaded file.
    pub path: String,
    /// The loaded role spec (if successful).
    pub spec: Option<RoleSpec>,
    /// Any validation issues found.
    pub issues: Vec<crate::validator::ValidationIssue>,
    /// Error if the file could not be loaded.
    pub error: Option<RoleError>,
}

/// Summary of a directory load operation.
#[derive(Debug)]
pub struct LoadSummary {
    /// Total files found.
    pub total_files: usize,
    /// Successfully loaded roles.
    pub loaded: usize,
    /// Files that had errors.
    pub errors: usize,
    /// Files that had warnings.
    pub warnings: usize,
    /// Per-file results.
    pub results: Vec<RoleLoadResult>,
}

impl LoadSummary {
    /// Returns `true` when at least one file produced a hard error.
    pub fn has_errors(&self) -> bool {
        self.errors > 0
            || self.results.iter().any(|result| {
                result
                    .issues
                    .iter()
                    .any(|issue| issue.severity == ValidationSeverity::Error)
            })
    }

    /// Returns `true` when any file produced a warning.
    pub fn has_warnings(&self) -> bool {
        self.warnings > 0
    }

    /// Returns `true` if the summary should block startup or command success.
    pub fn has_blocking_issues(&self, options: RoleLoadOptions) -> bool {
        self.has_errors() || (options.treat_warnings_as_errors && self.has_warnings())
    }
}

/// Loads role definitions from the filesystem.
///
/// The loader encapsulates the full pipeline:
/// 1. Discover Markdown files in the roles directory.
/// 2. Parse each file into a [`RawRoleSource`].
/// 3. Normalize into a [`RoleSpec`].
/// 4. Validate the specification.
/// 5. Apply organizational hierarchy.
/// 6. Register in the [`RoleRegistry`].
pub struct RoleLoader;

impl RoleLoader {
    /// Load all roles from a directory and register them.
    ///
    /// The directory is expected to follow the structure:
    /// ```text
    /// roles/
    ///   ORGANIGRAM.md
    ///   README.md
    ///   00_GOVERNANCE/
    ///     CEO_Agent.md
    ///     ...
    ///   01_PRODUCT_TECH/
    ///     ...
    /// ```
    ///
    /// Files named `ORGANIGRAM.md`, `README.md`, or any non-`.md` files
    /// are skipped.
    pub fn load_directory(dir: &Path, registry: &RoleRegistry) -> RoleResult<LoadSummary> {
        let hierarchy = RoleHierarchy::from_default_organigram();
        Self::load_directory_with_hierarchy_and_options(
            dir,
            registry,
            &hierarchy,
            RoleLoadOptions::default(),
        )
    }

    /// Load all roles from a directory with caller-provided loader options.
    pub fn load_directory_with_options(
        dir: &Path,
        registry: &RoleRegistry,
        options: RoleLoadOptions,
    ) -> RoleResult<LoadSummary> {
        let hierarchy = RoleHierarchy::from_default_organigram();
        Self::load_directory_with_hierarchy_and_options(dir, registry, &hierarchy, options)
    }

    /// Load all roles with a custom hierarchy.
    pub fn load_directory_with_hierarchy(
        dir: &Path,
        registry: &RoleRegistry,
        hierarchy: &RoleHierarchy,
    ) -> RoleResult<LoadSummary> {
        Self::load_directory_with_hierarchy_and_options(
            dir,
            registry,
            hierarchy,
            RoleLoadOptions::default(),
        )
    }

    /// Load all roles with a custom hierarchy and explicit loader options.
    pub fn load_directory_with_hierarchy_and_options(
        dir: &Path,
        registry: &RoleRegistry,
        hierarchy: &RoleHierarchy,
        options: RoleLoadOptions,
    ) -> RoleResult<LoadSummary> {
        let role_files = Self::discover_role_files(dir)?;
        let total_files = role_files.len();
        let mut results = Vec::with_capacity(total_files);
        let mut specs = Vec::new();

        // Phase 1: Parse and normalize all files.
        for (path, department_dir) in &role_files {
            let rel_path = path
                .strip_prefix(dir)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    results.push(RoleLoadResult {
                        path: rel_path.clone(),
                        spec: None,
                        issues: Vec::new(),
                        error: Some(RoleError::FileRead {
                            path: rel_path,
                            source: e,
                        }),
                    });
                    continue;
                }
            };

            let raw = match RoleParser::parse(&content, &rel_path, department_dir.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    results.push(RoleLoadResult {
                        path: rel_path,
                        spec: None,
                        issues: Vec::new(),
                        error: Some(e),
                    });
                    continue;
                }
            };

            let spec = match RoleNormalizer::normalize(&raw) {
                Ok(s) => s,
                Err(e) => {
                    results.push(RoleLoadResult {
                        path: rel_path,
                        spec: None,
                        issues: Vec::new(),
                        error: Some(e),
                    });
                    continue;
                }
            };

            let issues = RoleValidator::validate(&spec);
            let has_errors = RoleValidator::has_errors(&issues);
            let has_warnings = issues
                .iter()
                .any(|issue| issue.severity == ValidationSeverity::Warning);
            let blocked = has_errors || (options.treat_warnings_as_errors && has_warnings);

            results.push(RoleLoadResult {
                path: rel_path,
                spec: if blocked { None } else { Some(spec.clone()) },
                issues,
                error: None,
            });

            if !blocked {
                specs.push(spec);
            }
        }

        // Phase 2: Apply hierarchy to all valid specs.
        hierarchy.apply_to_specs(&mut specs)?;

        // Phase 3: Register all valid specs.
        let mut loaded = 0;
        for spec in specs {
            match registry.register(spec) {
                Ok(_) => loaded += 1,
                Err(e) => {
                    tracing::warn!("Failed to register role: {}", e);
                }
            }
        }

        let errors = results
            .iter()
            .filter(|r| {
                r.error.is_some()
                    || r.issues
                        .iter()
                        .any(|i| i.severity == ValidationSeverity::Error)
            })
            .count();
        let warnings = results
            .iter()
            .filter(|r| {
                r.issues
                    .iter()
                    .any(|i| i.severity == ValidationSeverity::Warning)
            })
            .count();

        tracing::info!(
            total = total_files,
            loaded = loaded,
            errors = errors,
            warnings = warnings,
            "Role loading complete"
        );

        Ok(LoadSummary {
            total_files,
            loaded,
            errors,
            warnings,
            results,
        })
    }

    /// Discover all role Markdown files in the directory tree.
    fn discover_role_files(dir: &Path) -> RoleResult<Vec<(PathBuf, Option<String>)>> {
        let mut files = Vec::new();

        if !dir.is_dir() {
            return Err(RoleError::FileRead {
                path: dir.to_string_lossy().to_string(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "roles directory not found",
                ),
            });
        }

        // Scan subdirectories (department folders).
        let entries = std::fs::read_dir(dir).map_err(|e| RoleError::FileRead {
            path: dir.to_string_lossy().to_string(),
            source: e,
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| RoleError::FileRead {
                path: dir.to_string_lossy().to_string(),
                source: e,
            })?;
            let path = entry.path();

            if path.is_dir() {
                let dir_name = path.file_name().map(|n| n.to_string_lossy().to_string());

                let sub_entries = std::fs::read_dir(&path).map_err(|e| RoleError::FileRead {
                    path: path.to_string_lossy().to_string(),
                    source: e,
                })?;

                for sub_entry in sub_entries {
                    let sub_entry = sub_entry.map_err(|e| RoleError::FileRead {
                        path: path.to_string_lossy().to_string(),
                        source: e,
                    })?;
                    let sub_path = sub_entry.path();

                    if Self::is_role_file(&sub_path) {
                        files.push((sub_path, dir_name.clone()));
                    }
                }
            } else if Self::is_role_file(&path) {
                // Top-level role files (if any).
                files.push((path, None));
            }
        }

        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(files)
    }

    /// Check whether a file is a role definition (not README or ORGANIGRAM).
    fn is_role_file(path: &Path) -> bool {
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("md") {
            return false;
        }
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let name_upper = file_name.to_uppercase();
        !name_upper.starts_with("README") && !name_upper.starts_with("ORGANIGRAM")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_role_file_accepts_agent_md() {
        assert!(RoleLoader::is_role_file(Path::new("CEO_Agent.md")));
        assert!(RoleLoader::is_role_file(Path::new("Support_Agent.md")));
    }

    #[test]
    fn is_role_file_rejects_readme() {
        assert!(!RoleLoader::is_role_file(Path::new("README.md")));
        assert!(!RoleLoader::is_role_file(Path::new("ORGANIGRAM.md")));
    }

    #[test]
    fn is_role_file_rejects_non_md() {
        assert!(!RoleLoader::is_role_file(Path::new("data.json")));
        assert!(!RoleLoader::is_role_file(Path::new("script.sh")));
    }

    #[test]
    fn load_summary_reports_blocking_issues_in_strict_mode() {
        let summary = LoadSummary {
            total_files: 1,
            loaded: 0,
            errors: 0,
            warnings: 1,
            results: vec![RoleLoadResult {
                path: "roles/Test_Agent.md".into(),
                spec: None,
                issues: vec![crate::validator::ValidationIssue {
                    field: "mission".into(),
                    severity: ValidationSeverity::Warning,
                    message: "warning".into(),
                }],
                error: None,
            }],
        };

        assert!(summary.has_blocking_issues(RoleLoadOptions {
            treat_warnings_as_errors: true,
        }));
        assert!(!summary.has_blocking_issues(RoleLoadOptions::default()));
    }
}
