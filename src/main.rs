mod config;
mod export;
mod pipeline;
mod types;

use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::iter::ParallelBridge;
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

    // Stream JSON files from a walker thread into rayon workers.
    // This avoids collecting all paths upfront (which is slow on
    // networked filesystems due to stat() calls).
    let (tx, rx) = std::sync::mpsc::channel();
    let input_dir = config.input_dir.clone();
    std::thread::spawn(move || {
        walk_dir(&input_dir, &tx);
    });

    // Process in parallel as files are discovered
    let total = AtomicUsize::new(0);
    let processed = AtomicUsize::new(0);
    let filtered = AtomicUsize::new(0);
    let up_to_date = AtomicUsize::new(0);
    let errored = AtomicUsize::new(0);

    let errors: Vec<String> = rx
        .into_iter()
        .par_bridge()
        .filter_map(|path| {
            total.fetch_add(1, Ordering::Relaxed);
            match pipeline::process_ticket(&path, &config) {
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

    let total = total.load(Ordering::Relaxed);

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

fn walk_dir(dir: &std::path::Path, tx: &std::sync::mpsc::Sender<std::path::PathBuf>) {
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

        // Use file_type() from the dir entry instead of path.is_dir().
        // On Linux this reads d_type from readdir(), avoiding a stat() per entry.
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                eprintln!("WARNING: cannot get file type for {}: {}", entry.path().display(), e);
                continue;
            }
        };

        if ft.is_dir() {
            walk_dir(&entry.path(), tx);
        } else if ft.is_file() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let _ = tx.send(path);
            }
        }
    }
}
