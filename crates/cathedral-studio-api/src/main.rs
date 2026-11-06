use std::path::PathBuf;

use axum::serve;
use cathedral_core::initialize;
use clap::Parser;
use tokio::net::TcpListener;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "cathedral-studio-api",
    version,
    about = "Local authenticated API for Cathedral Studio"
)]
struct Args {
    /// Override Cathedral's local data directory.
    #[arg(long, env = "CATHEDRAL_HOME")]
    home: Option<PathBuf>,
    /// Loopback TCP port. Use 0 to let the operating system choose a free port.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u16))]
    port: u16,
}

fn data_dir(override_path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(path) = override_path {
        return Ok(path);
    }
    let base = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .ok_or_else(|| {
            anyhow::anyhow!("could not determine a platform data directory; pass --home")
        })?;
    Ok(base.join("cathedral"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let root = data_dir(args.home)?;
    initialize(&root)?;
    let root = root.canonicalize()?;

    // Loopback-only binding and a fresh token keep the control API local.
    let token = Uuid::new_v4().to_string();
    let listener = TcpListener::bind(("127.0.0.1", args.port)).await?;
    let address = listener.local_addr()?;
    let app = cathedral_studio_api::router(root, token.clone());

    println!(
        "CATHEDRAL_STUDIO_READY {}",
        serde_json::json!({
            "address": format!("http://{address}"),
            "token": token,
            "api_version": 1
        })
    );

    serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    Ok(())
}
