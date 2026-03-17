use std::path::PathBuf;

use crate::types::{Config, Mode, OutputFormat};

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

    let output_format = match output_format_str.as_str() {
        "markdown" => OutputFormat::Markdown,
        other => {
            eprintln!(
                "ERROR: {}: invalid output_format '{}'. Valid values: \"markdown\"",
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
