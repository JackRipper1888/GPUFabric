//! Model download CLI tool
//! 
//! Usage:
//!   cargo run --bin model_download -- <url> [output_path]
//! 
//! Examples:
//!   cargo run --bin model_download -- https://example.com/model.gguf
//!   cargo run --bin model_download -- https://example.com/model.gguf ./downloaded_model.gguf

use clap::{Arg, Command};
use gpuf_c::util::model_downloader::{ModelDownloader, DownloadConfig, DownloadProgress};
use gpuf_c::util;
use std::path::PathBuf;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    util::init_logging();
    let matches = Command::new("model_download")
        .version("1.0")
        .about("Download model files with parallel downloading and resume support")
        .arg(
            Arg::new("url")
                .help("URL of the model file to download")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("output")
                .help("Output path for the downloaded file")
                .index(2),
        )
        .arg(
            Arg::new("chunks")
                .long("chunks")
                .short('c')
                .help("Number of parallel download chunks")
                .value_parser(clap::value_parser!(usize))
                .default_value("4"),
        )
        .arg(
            Arg::new("chunk-size")
                .long("chunk-size")
                .short('s')
                .help("Chunk size in MB")
                .value_parser(clap::value_parser!(usize))
                .default_value("8"),
        )
        .arg(
            Arg::new("checksum")
                .long("checksum")
                .short('x')
                .help("SHA256 checksum for verification"),
        )
        .arg(
            Arg::new("no-resume")
                .long("no-resume")
                .help("Disable resume functionality")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    let url = matches.get_one::<String>("url").unwrap();
    let output_path = matches
        .get_one::<String>("output")
        .map(|s| PathBuf::from(s))
        .unwrap_or_else(|| {
            // Extract filename from URL
            let filename = url
                .split('/')
                .last()
                .unwrap_or("downloaded_model.bin");
            PathBuf::from(filename)
        });

    let parallel_chunks = *matches.get_one::<usize>("chunks").unwrap();
    let chunk_size_mb = *matches.get_one::<usize>("chunk-size").unwrap();
    let checksum = matches.get_one::<String>("checksum").cloned();
    let resume = !matches.get_flag("no-resume");

    println!("GPUFabric Model Downloader");
    println!("URL: {}", url);
    println!("Output: {:?}", output_path);
    println!("Parallel chunks: {}", parallel_chunks);
    println!("Chunk size: {} MB", chunk_size_mb);
    println!("Resume: {}", if resume { "Enabled" } else { "Disabled" });
    if checksum.is_some() {
        println!("Checksum verification: Enabled");
    }
    println!();

    let config = DownloadConfig {
        url: url.clone(),
        output_path: output_path.clone(),
        parallel_chunks,
        chunk_size: chunk_size_mb * 1024 * 1024,
        expected_size: None,
        checksum,
        resume,
    };

    let mut downloader = ModelDownloader::new(config);
    
    // Set up progress tracking
    let start_time = std::time::Instant::now();
    downloader.set_progress_callback(move |progress: DownloadProgress| {
        let percentage = progress.percentage * 100.0;
        let downloaded_mb = progress.downloaded_bytes / (1024 * 1024);
        let total_mb = progress.total_bytes / (1024 * 1024);
        let speed_mbps = progress.speed_bps / (1024 * 1024);
        
        // Clear line and print progress
        print!(
            "\rProgress: {:.1}% ({}/{} MB) - {:.1} MB/s",
            percentage, downloaded_mb, total_mb, speed_mbps
        );
        
        if let Some(eta) = progress.eta_seconds {
            let eta_minutes = eta / 60;
            let eta_seconds = eta % 60;
            print!(" - ETA: {}:{:02}", eta_minutes, eta_seconds);
        }
        
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
    });

    // Start download
    println!("Starting download...");
    match downloader.download().await {
        Ok(_) => {
            println!();
            println!("Download completed successfully!");
            
            // Show file info
            match std::fs::metadata(&output_path) {
                Ok(metadata) => {
                    let file_size_mb = metadata.len() / (1024 * 1024);
                    let elapsed_seconds = start_time.elapsed().as_secs();
                    let avg_speed_mbps = if elapsed_seconds > 0 {
                        metadata.len() / (1024 * 1024) / elapsed_seconds
                    } else {
                        0
                    };
                    
                    println!("File size: {} MB", file_size_mb);
                    println!("Time elapsed: {} seconds", elapsed_seconds);
                    println!("Average speed: {} MB/s", avg_speed_mbps);
                    println!("File saved to: {:?}", output_path);
                }
                Err(e) => {
                    println!("Warning: Could not get file metadata: {}", e);
                    println!("Expected file location: {:?}", output_path);
                }
            }
        }
        Err(e) => {
            println!();
            eprintln!("Download failed: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}
