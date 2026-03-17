mod config;
mod export;
mod pipeline;
mod types;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;

use types::{Mode, TicketResult};

fn main() {
    let config = config::load_config("config.toml");

    // In replace mode, wipe the output directory
    if matches!(config.mode, Mode::Replace) && config.output_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&config.output_dir) {
            eprintln!(
                "ERROR: cannot remove output directory {}: {}",
                config.output_dir.display(),
                e
            );
            std::process::exit(1);
        }
    }

    // Discover all JSON files
    let json_files = discover_json_files(&config.input_dir);
    let total = json_files.len();

    if total == 0 {
        eprintln!("ERROR: no JSON files found in {}", config.input_dir.display());
        std::process::exit(1);
    }

    println!("Found {} ticket files", total);

    // Process in parallel
    let processed = AtomicUsize::new(0);
    let filtered = AtomicUsize::new(0);
    let up_to_date = AtomicUsize::new(0);
    let errored = AtomicUsize::new(0);

    let errors: Vec<String> = json_files
        .par_iter()
        .filter_map(|path| {
            match pipeline::process_ticket(path, &config) {
                Ok(TicketResult::Processed) => {
                    processed.fetch_add(1, Ordering::Relaxed);
                    None
                }
                Ok(TicketResult::Filtered) => {
                    filtered.fetch_add(1, Ordering::Relaxed);
                    None
                }
                Ok(TicketResult::UpToDate) => {
                    up_to_date.fetch_add(1, Ordering::Relaxed);
                    None
                }
                Err(e) => {
                    errored.fetch_add(1, Ordering::Relaxed);
                    eprintln!("ERROR: {}", e);
                    Some(e)
                }
            }
        })
        .collect();

    // Summary
    println!();
    println!("=== Summary ===");
    println!("Total files:   {}", total);
    println!("Processed:     {}", processed.load(Ordering::Relaxed));
    println!("Filtered:      {}", filtered.load(Ordering::Relaxed));
    println!("Up-to-date:    {}", up_to_date.load(Ordering::Relaxed));
    println!("Errors:        {}", errored.load(Ordering::Relaxed));

    if !errors.is_empty() {
        eprintln!();
        eprintln!("=== Errors ===");
        for e in &errors {
            eprintln!("  {}", e);
        }
        std::process::exit(1);
    }
}

fn discover_json_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_dir(dir, &mut files);
    files
}

fn walk_dir(dir: &std::path::Path, files: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("WARNING: cannot read directory {}: {}", dir.display(), e);
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("WARNING: directory entry error: {}", e);
                continue;
            }
        };

        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "json") {
            files.push(path);
        }
    }
}
