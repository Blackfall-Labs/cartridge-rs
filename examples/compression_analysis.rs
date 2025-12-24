//! Compression Analysis Example
//!
//! Demonstrates the compressed_size field showing actual disk usage vs logical size.
//! Great for understanding compression ratios and space savings.
//!
//! Run with: cargo run --example compression_analysis

use cartridge_rs::Cartridge;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cartridge Compression Analysis ===\n");

    let mut cart = Cartridge::create("compression-demo", "Compression Demo")?;

    // Create files with different compression characteristics
    println!("Creating test files...\n");

    // Highly compressible text (repeated pattern)
    let repeated = "AAAA".repeat(5000); // 20KB of 'A'
    cart.write("compressible/repeated.txt", repeated.as_bytes())?;
    println!("✓ Created repeated.txt (highly compressible)");

    // JSON with structure (moderate compression)
    let json = serde_json::json!({
        "users": (0..100).map(|i| {
            serde_json::json!({
                "id": i,
                "name": format!("User {}", i),
                "email": format!("user{}@example.com", i),
            })
        }).collect::<Vec<_>>(),
    })
    .to_string();
    cart.write("compressible/data.json", json.as_bytes())?;
    println!("✓ Created data.json (moderate compression)");

    // Random-like data (low compression)
    let random_like = (0..10000)
        .map(|i| format!("{:x}", (i as u64).wrapping_mul(16777619)))
        .collect::<Vec<_>>()
        .join(",");
    cart.write("incompressible/random.txt", random_like.as_bytes())?;
    println!("✓ Created random.txt (low compression)");

    // Small file
    cart.write("small/tiny.txt", b"Small file")?;
    println!("✓ Created tiny.txt (small file)\n");

    cart.flush()?;

    // Analyze compression
    println!("=== Compression Analysis ===\n");

    // Get all entries (use "/" for root)
    let compressible = cart.list_entries("compressible")?;
    let incompressible = cart.list_entries("incompressible")?;
    let small = cart.list_entries("small")?;

    let mut all_entries = Vec::new();
    all_entries.extend(compressible);
    all_entries.extend(incompressible);
    all_entries.extend(small);

    let mut files: Vec<_> = all_entries.iter().filter(|e| !e.is_dir).collect();
    files.sort_by_key(|e| e.size.unwrap_or(0));

    println!(
        "{:<30} {:>12} {:>12} {:>10} {:>10}",
        "File", "Logical", "Physical", "Ratio", "Saved"
    );
    println!("{}", "=".repeat(80));

    let mut total_logical = 0u64;
    let mut total_physical = 0u64;

    for entry in &files {
        if let (Some(size), Some(compressed)) = (entry.size, entry.compressed_size) {
            let ratio = if size > 0 {
                (compressed as f64 / size as f64) * 100.0
            } else {
                100.0
            };
            let saved = size.saturating_sub(compressed);
            let saved_pct = if size > 0 {
                (saved as f64 / size as f64) * 100.0
            } else {
                0.0
            };

            println!(
                "{:<30} {:>10} B {:>10} B {:>9.1}% {:>8.1}%",
                entry.path, size, compressed, ratio, saved_pct
            );

            total_logical += size;
            total_physical += compressed;
        }
    }

    println!("{}", "=".repeat(80));
    let overall_ratio = if total_logical > 0 {
        (total_physical as f64 / total_logical as f64) * 100.0
    } else {
        100.0
    };
    let total_saved = total_logical.saturating_sub(total_physical);
    let total_saved_pct = if total_logical > 0 {
        (total_saved as f64 / total_logical as f64) * 100.0
    } else {
        0.0
    };

    println!(
        "{:<30} {:>10} B {:>10} B {:>9.1}% {:>8.1}%",
        "TOTAL", total_logical, total_physical, overall_ratio, total_saved_pct
    );

    println!("\n=== Summary ===");
    println!(
        "Total logical size: {} bytes ({:.2} KB)",
        total_logical,
        total_logical as f64 / 1024.0
    );
    println!(
        "Total physical size: {} bytes ({:.2} KB)",
        total_physical,
        total_physical as f64 / 1024.0
    );
    println!(
        "Space saved: {} bytes ({:.2} KB)",
        total_saved,
        total_saved as f64 / 1024.0
    );
    println!("Overall compression ratio: {:.1}%", overall_ratio);
    println!("Overall space savings: {:.1}%", total_saved_pct);

    // Show container stats
    println!("\n=== Container Stats ===");
    let stats = cart.inner().stats();
    println!("Container blocks: {}", stats.total_blocks);
    println!("Used blocks: {}", stats.used_blocks);
    println!("Free blocks: {}", stats.free_blocks);
    println!("Container size: {} KB", stats.total_blocks * 4);
    println!("Used space: {} KB", stats.used_blocks * 4);

    println!("\n✓ Compression analysis complete!");
    println!("\nNote: Cartridge uses 4KB blocks. Small files and compressed data");
    println!("      are rounded up to the nearest 4KB boundary.");

    // Cleanup
    std::fs::remove_file("compression-demo.cart")?;
    println!("\n(Cleaned up compression-demo.cart)");

    Ok(())
}
