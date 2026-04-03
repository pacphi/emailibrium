//! YAML Parser for Rule Files with Detailed Error Reporting
//!
//! This module provides YAML parsing for email automation rules with:
//! - File size limits (max 100MB)
//! - Detailed error messages with line/column information
//! - Context extraction for parse errors
//! - Support for multiple file merging
//!
//! # Example
//!
//! ```rust,no_run
//! use emailibrium::rules::{RuleParser, parse_yaml_file};
//!
//! # fn example() -> anyhow::Result<()> {
//! // Parse a single YAML file
//! let ruleset = parse_yaml_file("rules/email_filters.yaml")?;
//! println!("Loaded {} rules", ruleset.rules.len());
//!
//! // Parse with custom size limit
//! let parser = RuleParser::with_max_size(10 * 1024 * 1024); // 10MB
//! let ruleset = parser.parse_file("rules/small_rules.yaml")?;
//!
//! // Merge multiple files
//! let parser = RuleParser::new();
//! let merged = parser.parse_files(&[
//!     "rules/work.yaml",
//!     "rules/personal.yaml",
//!     "rules/newsletters.yaml",
//! ])?;
//! # Ok(())
//! # }
//! ```

use super::schema::{MAX_FILE_SIZE_BYTES, RuleSet};
use anyhow::{Context, Result};
use std::path::Path;

/// Parse error with line/column information for debugging.
///
/// This struct provides detailed error context to help users identify
/// and fix syntax errors in their YAML rule files.
#[derive(Debug, Clone)]
pub struct ParseError {
    /// Human-readable error message
    pub message: String,
    /// Line number where error occurred (1-indexed)
    pub line: Option<usize>,
    /// Column number where error occurred (1-indexed)
    pub column: Option<usize>,
    /// Surrounding lines of YAML for context
    pub context: Option<String>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let (Some(line), Some(col)) = (self.line, self.column) {
            write!(f, " at line {}, column {}", line, col)?;
        }
        if let Some(ctx) = &self.context {
            write!(f, "\nContext: {}", ctx)?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}

/// YAML parser for rule files with size limits and error reporting.
///
/// The parser enforces file size limits to prevent memory exhaustion
/// and provides detailed error messages for debugging.
pub struct RuleParser {
    /// Maximum file size to accept (bytes)
    max_size: usize,
}

impl RuleParser {
    /// Create a new parser with default limits (100MB).
    ///
    /// # Examples
    ///
    /// ```
    /// use emailibrium::rules::RuleParser;
    ///
    /// let parser = RuleParser::new();
    /// ```
    pub fn new() -> Self {
        Self {
            max_size: MAX_FILE_SIZE_BYTES,
        }
    }

    /// Create a parser with custom size limit.
    ///
    /// # Arguments
    ///
    /// * `max_size` - Maximum file size in bytes
    ///
    /// # Examples
    ///
    /// ```
    /// use emailibrium::rules::RuleParser;
    ///
    /// // Accept files up to 10MB
    /// let parser = RuleParser::with_max_size(10 * 1024 * 1024);
    /// ```
    pub fn with_max_size(max_size: usize) -> Self {
        Self { max_size }
    }

    /// Parse YAML from a string.
    ///
    /// # Arguments
    ///
    /// * `yaml` - YAML content as a string
    ///
    /// # Returns
    ///
    /// Returns a [`RuleSet`] containing all parsed rules.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - YAML content exceeds maximum size limit
    /// - YAML syntax is invalid
    /// - Rule validation fails
    ///
    /// # Examples
    ///
    /// ```
    /// use emailibrium::rules::RuleParser;
    ///
    /// let parser = RuleParser::new();
    /// let yaml = r#"
    /// version: "1.0"
    /// rules:
    ///   - id: "test"
    ///     name: "Test Rule"
    ///     conditions:
    ///       field: subject
    ///       operator: contains
    ///       value: "test"
    ///     actions:
    ///       - type: star
    /// "#;
    /// let ruleset = parser.parse_str(yaml)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn parse_str(&self, yaml: &str) -> Result<RuleSet> {
        // Check size limit
        if yaml.len() > self.max_size {
            anyhow::bail!(
                "YAML content size {} exceeds maximum {} bytes",
                yaml.len(),
                self.max_size
            );
        }

        // Parse YAML to RuleSet
        let ruleset: RuleSet =
            serde_yaml_ng::from_str(yaml).context("Failed to parse YAML content")?;

        // Validate the ruleset
        ruleset
            .validate()
            .map_err(|e| anyhow::anyhow!("Ruleset validation failed: {}", e))?;

        Ok(ruleset)
    }

    /// Parse YAML from a file
    pub fn parse_file<P: AsRef<Path>>(&self, path: P) -> Result<RuleSet> {
        let path = path.as_ref();

        // Check file exists
        if !path.exists() {
            anyhow::bail!("File not found: {}", path.display());
        }

        // Check file size before reading
        let metadata = std::fs::metadata(path)
            .context(format!("Failed to read file metadata: {}", path.display()))?;

        if metadata.len() > self.max_size as u64 {
            anyhow::bail!(
                "File size {} exceeds maximum {} bytes",
                metadata.len(),
                self.max_size
            );
        }

        // Read file content
        let content = std::fs::read_to_string(path)
            .context(format!("Failed to read file: {}", path.display()))?;

        // Parse content
        self.parse_str(&content)
            .context(format!("Failed to parse file: {}", path.display()))
    }

    /// Parse multiple YAML files and merge into single RuleSet
    pub fn parse_files<P: AsRef<Path>>(&self, paths: &[P]) -> Result<RuleSet> {
        let mut merged = RuleSet::new();

        for path in paths {
            let ruleset = self.parse_file(path)?;
            merged.rules.extend(ruleset.rules);
        }

        // Validate merged ruleset
        merged
            .validate()
            .map_err(|e| anyhow::anyhow!("Merged ruleset validation failed: {}", e))?;

        Ok(merged)
    }

    /// Parse YAML and return detailed error information
    pub fn parse_with_errors(&self, yaml: &str) -> Result<RuleSet, ParseError> {
        // Check size limit
        if yaml.len() > self.max_size {
            return Err(ParseError {
                message: format!(
                    "YAML content size {} exceeds maximum {} bytes",
                    yaml.len(),
                    self.max_size
                ),
                line: None,
                column: None,
                context: None,
            });
        }

        // Try to parse YAML directly (not through parse_str to get better errors)
        let ruleset: RuleSet = match serde_yaml_ng::from_str(yaml) {
            Ok(r) => r,
            Err(e) => {
                // Extract line/column info from serde_yaml error
                let error_msg = e.to_string();
                let location = e.location();
                let (line, column) = if let Some(loc) = location {
                    (Some(loc.line()), Some(loc.column()))
                } else {
                    Self::extract_location(&error_msg)
                };

                return Err(ParseError {
                    message: error_msg.clone(),
                    line,
                    column,
                    context: Self::extract_context(yaml, line),
                });
            }
        };

        // Validate the ruleset
        if let Err(e) = ruleset.validate() {
            // For validation errors, we don't have line info
            return Err(ParseError {
                message: format!("Ruleset validation failed: {}", e),
                line: None,
                column: None,
                context: None,
            });
        }

        Ok(ruleset)
    }

    /// Extract line and column from error message
    fn extract_location(error_msg: &str) -> (Option<usize>, Option<usize>) {
        // Try to parse "at line X column Y" pattern
        let line_re = match regex::Regex::new(r"line (\d+)") {
            Ok(re) => re,
            Err(_) => return (None, None),
        };
        let col_re = match regex::Regex::new(r"column (\d+)") {
            Ok(re) => re,
            Err(_) => return (None, None),
        };

        let line = line_re
            .captures(error_msg)
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse().ok());

        let column = col_re
            .captures(error_msg)
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse().ok());

        (line, column)
    }

    /// Extract context lines around error location
    fn extract_context(yaml: &str, line_num: Option<usize>) -> Option<String> {
        let line_num = line_num?;
        let lines: Vec<&str> = yaml.lines().collect();

        if line_num == 0 || line_num > lines.len() {
            return None;
        }

        let start = line_num.saturating_sub(3);
        let end = (line_num + 2).min(lines.len());

        let mut context = String::new();
        for (idx, line) in lines[start..end].iter().enumerate() {
            let current_line = start + idx + 1;
            let marker = if current_line == line_num { ">" } else { " " };
            context.push_str(&format!("{} {:4} | {}\n", marker, current_line, line));
        }

        Some(context)
    }

    /// Validate YAML syntax without full parsing
    pub fn validate_syntax(&self, yaml: &str) -> Result<()> {
        // Try to parse as generic YAML first
        let _: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(yaml).context("Invalid YAML syntax")?;
        Ok(())
    }
}

impl Default for RuleParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to parse YAML string
pub fn parse_yaml(yaml: &str) -> Result<RuleSet> {
    RuleParser::new().parse_str(yaml)
}

/// Helper function to parse YAML file
pub fn parse_yaml_file<P: AsRef<Path>>(path: P) -> Result<RuleSet> {
    RuleParser::new().parse_file(path)
}
