use axum::serve;
use cathedral_core::initialize;
use cathedral_studio_api::router;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{Manager, State};
use tokio::net::TcpListener;
use uuid::Uuid;

#[derive(Clone, Serialize)]
struct Connection {
    address: String,
    token: String,
}

struct ServerState(Connection);

#[tauri::command]
fn studio_connection(state: State<'_, ServerState>) -> Connection {
    state.0.clone()
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![studio_connection])
        .setup(|app| {
            let base = dirs::data_local_dir()
                .or_else(dirs::data_dir)
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "could not determine a platform data directory"))?;
            let data_dir: PathBuf = base.join("cathedral");
            initialize(&data_dir)?;
            let root = data_dir.canonicalize()?;
            let token = Uuid::new_v4().to_string();
            let (listener, address) = tauri::async_runtime::block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
                let address = listener.local_addr()?;
                Ok::<_, std::io::Error>((listener, address))
            })?;
            let api = router(root, token.clone());
            tauri::async_runtime::spawn(async move {
                if let Err(error) = serve(listener, api).await {
                    eprintln!("Cathedral Studio API stopped: {error}");
                }
            });
            app.manage(ServerState(Connection {
                address: format!("http://{address}"),
                token,
            }));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to launch Cathedral Studio");
}
