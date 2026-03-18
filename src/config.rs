use std::path::PathBuf;

use chrono::NaiveDate;
use regex::Regex;

use crate::types::{Config, FilterConfig, Mode, OutputFormat, PiiFilter};

pub fn load_config(path: &str) -> Config {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ERROR: cannot read {}: {}", path, e);
            std::process::exit(1);
        }
    };

    let value: toml::Value = match content.parse() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("ERROR: {}: parse error: {}", path, e);
            std::process::exit(1);
        }
    };

    let table = match value.as_table() {
        Some(t) => t,
        None => {
            eprintln!("ERROR: {}: expected a TOML table at root", path);
            std::process::exit(1);
        }
    };

    let input_dir = require_str(table, "input_dir", path);
    let output_dir = require_str(table, "output_dir", path);
    let output_format_str = require_str(table, "output_format", path);
    let mode_str = require_str(table, "mode", path);
    let symlink_attachments = require_bool(table, "symlink_attachments", path);
    let pii_filter_str = require_str(table, "pii_filter", path);

    let output_format = match output_format_str.as_str() {
        "markdown" => OutputFormat::Markdown,
        "json" => OutputFormat::Json,
        other => {
            eprintln!(
                "ERROR: {}: invalid output_format '{}'. Valid values: \"markdown\", \"json\"",
                path, other
            );
            std::process::exit(1);
        }
    };

    let mode = match mode_str.as_str() {
        "update" => Mode::Update,
        "replace" => Mode::Replace,
        other => {
            eprintln!(
                "ERROR: {}: invalid mode '{}'. Valid values: \"update\", \"replace\"",
                path, other
            );
            std::process::exit(1);
        }
    };

    let pii_filter = match pii_filter_str.as_str() {
        "all" => PiiFilter::All,
        "asker" => PiiFilter::Asker,
        "none" => PiiFilter::None,
        other => {
            eprintln!(
                "ERROR: {}: invalid pii_filter '{}'. Valid values: \"all\", \"asker\", \"none\"",
                path, other
            );
            std::process::exit(1);
        }
    };

    let deterministic_pii = require_bool(table, "deterministic_pii", path);

    let filter = parse_filter_config(table, path);

    let input_path = PathBuf::from(&input_dir);
    if !input_path.is_dir() {
        eprintln!(
            "ERROR: {}: input_dir '{}' does not exist or is not a directory",
            path, input_dir
        );
        std::process::exit(1);
    }

    Config {
        input_dir: input_path,
        output_dir: PathBuf::from(output_dir),
        output_format,
        mode,
        symlink_attachments,
        pii_filter,
        deterministic_pii,
        filter,
    }
}

fn require_str(table: &toml::Table, key: &str, config_path: &str) -> String {
    match table.get(key).and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            eprintln!(
                "ERROR: {}: missing required field '{}'",
                config_path, key
            );
            std::process::exit(1);
        }
    }
}

fn require_bool(table: &toml::Table, key: &str, config_path: &str) -> bool {
    match table.get(key).and_then(|v| v.as_bool()) {
        Some(b) => b,
        None => {
            eprintln!(
                "ERROR: {}: missing required boolean field '{}'",
                config_path, key
            );
            std::process::exit(1);
        }
    }
}

fn require_str_array(table: &toml::Table, key: &str, config_path: &str) -> Vec<String> {
    match table.get(key).and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .enumerate()
            .map(|(i, v)| match v.as_str() {
                Some(s) => s.to_string(),
                None => {
                    eprintln!(
                        "ERROR: {}: {}[{}] is not a string",
                        config_path, key, i
                    );
                    std::process::exit(1);
                }
            })
            .collect(),
        None => {
            eprintln!(
                "ERROR: {}: missing required array field '{}'",
                config_path, key
            );
            std::process::exit(1);
        }
    }
}

fn parse_optional_regex(table: &toml::Table, key: &str, config_path: &str) -> Option<Regex> {
    let s = require_str(table, key, config_path);
    if s.is_empty() {
        None
    } else {
        match Regex::new(&s) {
            Ok(re) => Some(re),
            Err(e) => {
                eprintln!(
                    "ERROR: {}: invalid regex for '{}': {}",
                    config_path, key, e
                );
                std::process::exit(1);
            }
        }
    }
}

fn parse_filter_config(root: &toml::Table, config_path: &str) -> FilterConfig {
    let table = match root.get("filter").and_then(|v| v.as_table()) {
        Some(t) => t,
        None => {
            eprintln!(
                "ERROR: {}: missing required [filter] section",
                config_path
            );
            std::process::exit(1);
        }
    };

    let min_created_date_str = require_str(table, "min_created_date", config_path);
    let min_created_date = if min_created_date_str.is_empty() {
        None
    } else {
        match NaiveDate::parse_from_str(&min_created_date_str, "%Y-%m-%d") {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!(
                    "ERROR: {}: filter.min_created_date '{}': {}",
                    config_path, min_created_date_str, e
                );
                std::process::exit(1);
            }
        }
    };

    let exclude_contact_types = require_str_array(table, "exclude_contact_types", config_path)
        .into_iter()
        .map(|s| s.to_lowercase())
        .collect();

    let include_close_codes = require_str_array(table, "include_close_codes", config_path);

    let require_closed_or_resolved =
        require_bool(table, "require_closed_or_resolved", config_path);

    let exclude_created_by = parse_optional_regex(table, "exclude_created_by", config_path);
    let exclude_assignment_group =
        parse_optional_regex(table, "exclude_assignment_group", config_path);

    FilterConfig {
        min_created_date,
        exclude_contact_types,
        include_close_codes,
        require_closed_or_resolved,
        exclude_created_by,
        exclude_assignment_group,
    }
}
