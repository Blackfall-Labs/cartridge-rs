//! Cartridge S3 Server
//!
//! S3-compatible HTTP server backed by Cartridge storage

use cartridge::{S3AclMode, S3FeatureFuses, S3SseMode, S3VersioningMode};
use cartridge::Cartridge;
use cartridge_s3::CartridgeS3Backend;
use clap::Parser;
use parking_lot::RwLock;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

// s3s imports
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as ConnBuilder;
use s3s::service::S3ServiceBuilder;
use tokio::net::TcpListener;

#[derive(Parser, Debug)]
#[command(name = "cartridge-s3-server")]
#[command(about = "S3-compatible HTTP server for Cartridge storage")]
struct Args {
    /// Path to cartridge file
    #[arg(short = 'p', long)]
    cartridge_path: PathBuf,

    /// Number of blocks (required for new cartridges)
    #[arg(short = 'b', long)]
    blocks: Option<usize>,

    /// Bind address
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    host: String,

    /// Port number
    #[arg(short = 'P', long, default_value = "9000")]
    port: u16,

    /// AWS Access Key ID (enables authentication)
    #[arg(long)]
    access_key: Option<String>,

    /// AWS Secret Access Key (required if access_key is set)
    #[arg(long)]
    secret_key: Option<String>,

    /// S3 versioning mode (none, snapshot-backed) [default: none]
    #[arg(long, default_value = "none")]
    s3_versioning: String,

    /// S3 ACL mode (ignore, record, enforce) [default: ignore]
    #[arg(long, default_value = "ignore")]
    s3_acl: String,

    /// S3 SSE mode (ignore, record, transparent) [default: ignore]
    #[arg(long, default_value = "ignore")]
    s3_sse: String,
}

/// Parse S3 versioning mode from CLI string
fn parse_versioning_mode(s: &str) -> Result<S3VersioningMode, String> {
    match s.to_lowercase().as_str() {
        "none" => Ok(S3VersioningMode::None),
        "snapshot-backed" | "snapshot_backed" | "snapshotbacked" => {
            Ok(S3VersioningMode::SnapshotBacked)
        }
        _ => Err(format!(
            "Invalid versioning mode '{}'. Valid options: none, snapshot-backed",
            s
        )),
    }
}

/// Parse S3 ACL mode from CLI string
fn parse_acl_mode(s: &str) -> Result<S3AclMode, String> {
    match s.to_lowercase().as_str() {
        "ignore" => Ok(S3AclMode::Ignore),
        "record" => Ok(S3AclMode::Record),
        "enforce" => Ok(S3AclMode::Enforce),
        _ => Err(format!(
            "Invalid ACL mode '{}'. Valid options: ignore, record, enforce",
            s
        )),
    }
}

/// Parse S3 SSE mode from CLI string
fn parse_sse_mode(s: &str) -> Result<S3SseMode, String> {
    match s.to_lowercase().as_str() {
        "ignore" => Ok(S3SseMode::Ignore),
        "record" => Ok(S3SseMode::Record),
        "transparent" => Ok(S3SseMode::Transparent),
        _ => Err(format!(
            "Invalid SSE mode '{}'. Valid options: ignore, record, transparent",
            s
        )),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let args = Args::parse();

    info!("Starting Cartridge S3 Server");
    info!("Cartridge path: {:?}", args.cartridge_path);

    // Parse S3 fuse configuration
    let versioning_mode = parse_versioning_mode(&args.s3_versioning)?;
    let acl_mode = parse_acl_mode(&args.s3_acl)?;
    let sse_mode = parse_sse_mode(&args.s3_sse)?;

    info!(
        "S3 fuses: versioning={:?}, acl={:?}, sse={:?}",
        versioning_mode, acl_mode, sse_mode
    );

    // Load or create cartridge
    let cartridge = if args.cartridge_path.exists() {
        info!("Opening existing cartridge: {:?}", args.cartridge_path);
        let cart = Cartridge::open(&args.cartridge_path)?;

        // Check if existing cartridge fuses match CLI args
        let existing_fuses = cart.header().get_s3_fuses();
        if existing_fuses.versioning_mode as u8 != versioning_mode as u8
            || existing_fuses.acl_mode as u8 != acl_mode as u8
            || existing_fuses.sse_mode as u8 != sse_mode as u8
        {
            info!(
                "Warning: Existing cartridge has different fuses (versioning={:?}, acl={:?}, sse={:?})",
                existing_fuses.versioning_mode, existing_fuses.acl_mode, existing_fuses.sse_mode
            );
            info!("CLI fuse arguments are ignored for existing cartridges");
        }

        cart
    } else {
        info!("Creating new cartridge: {:?}", args.cartridge_path);

        // Extract slug from filename (remove .cart extension if present)
        let filename = args.cartridge_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("cartridge-s3-data");
        let slug = filename.to_string();
        let title = format!("Cartridge S3 Storage: {}", slug);

        let mut cart = Cartridge::create_at(&args.cartridge_path, &slug, &title)?;

        // Apply S3 fuses to new cartridge
        let fuses = S3FeatureFuses {
            versioning_mode,
            acl_mode,
            sse_mode,
        };
        cart.header_mut().set_s3_fuses(fuses);
        info!("Applied S3 fuses to new cartridge");

        cart
    };

    // Create S3 backend with RwLock for concurrent reads
    let cart_arc = Arc::new(RwLock::new(cartridge));
    let backend = CartridgeS3Backend::new(cart_arc.clone());

    info!("S3 backend initialized");

    // Validate authentication args
    if args.access_key.is_some() != args.secret_key.is_some() {
        return Err("Both --access-key and --secret-key must be provided together".into());
    }

    // Create S3 service using s3s
    let service = {
        let mut builder = S3ServiceBuilder::new(backend);

        // Set up authentication if credentials provided
        if let (Some(access_key), Some(secret_key)) = (&args.access_key, &args.secret_key) {
            use s3s::auth::SimpleAuth;
            let auth = SimpleAuth::from_single(access_key.clone(), secret_key.clone());
            builder.set_auth(auth);
            info!("Authentication enabled for access key: {}", access_key);
        } else {
            info!("Running without authentication (open access)");
        }

        builder.build()
    };

    // Parse bind address
    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;

    info!("Server listening on http://{}", addr);
    info!("Ready to accept S3 API requests");

    // Usage instructions
    info!("Use with: aws --endpoint-url=http://{} s3 ...", addr);

    // Run HTTP server
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

    let http_server = ConnBuilder::new(TokioExecutor::new());

    info!("HTTP server running at http://{}", local_addr);

    loop {
        // Accept connection or wait for Ctrl+C
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((socket, _)) => {
                        let service_clone = service.clone();
                        let http_server_clone = http_server.clone();
                        tokio::spawn(async move {
                            let conn = http_server_clone.serve_connection(TokioIo::new(socket), service_clone);
                            if let Err(e) = conn.await {
                                eprintln!("Connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("Failed to accept connection: {}", e);
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Received Ctrl+C, shutting down...");
                break;
            }
        }
    }

    info!("Shutting down gracefully...");

    // Flush cartridge before exit
    {
        let mut cart = cart_arc.write();
        cart.flush()?;
        info!("Cartridge flushed to disk");
    }

    info!("Server stopped");

    Ok(())
}
