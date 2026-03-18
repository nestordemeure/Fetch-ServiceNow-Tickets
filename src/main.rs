mod config;
mod export;
mod pii;
mod pipeline;
mod types;

use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;
use walkdir::WalkDir;

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

    // Discover all JSON files. walkdir uses d_type from readdir() on Linux,
    // avoiding a stat() syscall per entry — critical on networked filesystems.
    let json_files: Vec<_> = WalkDir::new(&config.input_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "json"))
        .map(|e| e.into_path())
        .collect();

    let total = json_files.len();

    if total == 0 {
        eprintln!("ERROR: no JSON files found in {}", config.input_dir.display());
        std::process::exit(1);
    }

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

    if total == 0 {
        eprintln!("ERROR: no JSON files found in {}", config.input_dir.display());
        std::process::exit(1);
    }

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
