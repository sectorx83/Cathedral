//! Local authenticated API used by Cathedral Studio.
//!
//! The service binds to loopback only, requires a per-process bearer token,
//! and exposes the same experiment registry, runtime, and snapshot engine as the CLI.

use std::{fs, path::{Component, Path, PathBuf}, time::UNIX_EPOCH};

use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        Method, Request, StatusCode,
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use cathedral_plugin_sdk::{PluginManifest, PluginPermission};
use cathedral_core::{
    create_experiment, delete_experiment, inspect_experiment, list_experiments, manifest::Manifest,
    platform::PlatformInfo, registry::Experiment, runner, snapshot, workspace::Workspace,
    CathedralError,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(Clone)]
struct ApiState {
    data_dir: PathBuf,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl From<CathedralError> for ApiError {
    fn from(error: CathedralError) -> Self {
        let status = match &error {
            CathedralError::InvalidName(_) | CathedralError::Manifest(_) => StatusCode::BAD_REQUEST,
            CathedralError::NotFound(_) => StatusCode::NOT_FOUND,
            CathedralError::AlreadyExists(_) => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginInstallRequest { manifest: PluginManifest }
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginGrantRequest { permissions: Vec<PluginPermission> }
#[derive(Debug, Serialize)]
struct PluginView { manifest: PluginManifest, granted_permissions: Vec<PluginPermission> }

#[derive(Debug, Serialize)]
struct HealthView {
    status: &'static str,
    api_version: u32,
    host_os: &'static str,
    host_arch: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct ExperimentView {
    id: String,
    name: String,
    description: String,
    created_at: i64,
    manifest_version: u32,
    state: &'static str,
}

impl From<Experiment> for ExperimentView {
    fn from(item: Experiment) -> Self {
        Self {
            id: item.id,
            name: item.name,
            description: item.description,
            created_at: item.created_at,
            manifest_version: item.manifest_version,
            state: item.state.as_str(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateExperimentRequest {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunRequest {
    command: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SnapshotView {
    id: String,
    name: String,
    files: usize,
    directories: usize,
    bytes: u64,
    created_at: i64,
}

impl From<snapshot::Summary> for SnapshotView {
    fn from(item: snapshot::Summary) -> Self {
        Self {
            id: item.id,
            name: item.name,
            files: item.files,
            directories: item.directories,
            bytes: item.bytes,
            created_at: item.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct MetricsView {
    pid: u32,
    state: String,
    parent_pid: u32,
    virtual_memory_kb: u64,
    resident_memory_kb: u64,
    user_cpu_ticks: u64,
    system_cpu_ticks: u64,
    read_bytes: u64,
    write_bytes: u64,
    network_rx_bytes: u64,
    network_tx_bytes: u64,
    cgroup_memory_current_bytes: Option<u64>,
    cgroup_cpu_usage_usec: Option<u64>,
    cgroup_process_count: Option<u64>,
}

impl From<runner::ProcessMetrics> for MetricsView {
    fn from(value: runner::ProcessMetrics) -> Self {
        Self {
            pid: value.pid,
            state: value.state,
            parent_pid: value.parent_pid,
            virtual_memory_kb: value.virtual_memory_kb,
            resident_memory_kb: value.resident_memory_kb,
            user_cpu_ticks: value.user_cpu_ticks,
            system_cpu_ticks: value.system_cpu_ticks,
            read_bytes: value.read_bytes,
            write_bytes: value.write_bytes,
            network_rx_bytes: value.network_rx_bytes,
            network_tx_bytes: value.network_tx_bytes,
            cgroup_memory_current_bytes: value.cgroup_memory_current_bytes,
            cgroup_cpu_usage_usec: value.cgroup_cpu_usage_usec,
            cgroup_process_count: value.cgroup_process_count,
        }
    }
}

#[derive(Debug, Serialize)]
struct RunView {
    id: String,
    pid: u32,
    started_at: u64,
    command: Vec<String>,
    state: &'static str,
    timeout_seconds: Option<u64>,
    exit_code: Option<i32>,
    termination_signal: Option<String>,
    timed_out: bool,
    metrics: Option<MetricsView>,
}

fn run_view(record: runner::RunRecord) -> Result<RunView, ApiError> {
    let state = runner::process_state(&record).label();
    let metrics = runner::metrics(&record)?.map(MetricsView::from);
    Ok(RunView {
        id: record.id.clone(),
        pid: record.pid,
        started_at: record.started_at,
        command: record.command.clone(),
        state,
        timeout_seconds: record.timeout_seconds,
        exit_code: runner::exit_status(&record),
        termination_signal: runner::termination_signal(&record),
        timed_out: runner::timed_out(&record),
        metrics,
    })
}

#[derive(Debug, Serialize)]
struct LogsView {
    run_id: String,
    content: String,
    truncated: bool,
}

#[derive(Debug, Deserialize)]
struct PathQuery { #[serde(default)] path: String }
#[derive(Debug, Deserialize)]
struct CompareQuery { a: String, b: String }
#[derive(Debug, Serialize)]
struct FileEntryView { name: String, path: String, kind: &'static str, size: u64, modified_at: Option<u64> }
#[derive(Debug, Serialize)]
struct FileContentView { path: String, content: String, bytes: usize }
#[derive(Debug, Serialize)]
struct DifferenceView { path: String, change: String }
#[derive(Debug, Serialize)]
struct StopView {
    run_id: String,
    stopped: bool,
}

fn experiment_workspace(data_dir: &Path, name: &str) -> Result<(Experiment, PathBuf), ApiError> {
    let experiment = inspect_experiment(data_dir, name)?;
    let workspace = Workspace::new(data_dir)?.validate(Path::new(&experiment.workspace_path))?;
    Ok((experiment, workspace))
}

fn run_root(data_dir: &Path, experiment: &Experiment) -> PathBuf {
    data_dir.join("runs").join(&experiment.id)
}

async fn require_bearer_token(
    State(token): State<String>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let supplied = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    if supplied != Some(token.as_str()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "a valid bearer token is required".to_owned(),
            }),
        )
            .into_response();
    }

    next.run(request).await
}

/// Build the API router. The shipped binary binds it to 127.0.0.1.
pub fn router(data_dir: PathBuf, token: String) -> Router {
    let state = ApiState { data_dir };
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list([
            "tauri://localhost".parse().expect("static origin"),
            "http://tauri.localhost".parse().expect("static origin"),
            "https://tauri.localhost".parse().expect("static origin"),
            "http://localhost:5173".parse().expect("static origin"),
            "http://127.0.0.1:5173".parse().expect("static origin"),
        ]))
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/plugins", get(list_plugins).post(install_plugin))
        .route("/api/v1/plugins/:id", axum::routing::delete(remove_plugin))
        .route("/api/v1/plugins/:id/grants", axum::routing::put(update_plugin_grants))
        .route("/api/v1/experiments", get(list).post(create))
        .route("/api/v1/experiments/:name", get(inspect).delete(remove))
        .route(
            "/api/v1/experiments/:name/snapshots",
            get(list_snapshots).post(create_snapshot),
        )
        .route(
            "/api/v1/experiments/:name/snapshots/:snapshot_id/restore",
            axum::routing::post(restore_snapshot),
        )
        .route("/api/v1/experiments/:name/files", get(list_files))
        .route("/api/v1/experiments/:name/files/content", get(read_file))
        .route("/api/v1/experiments/:name/snapshots/:snapshot_id/verify", get(verify_snapshot))
        .route("/api/v1/experiments/:name/snapshots/compare", get(compare_snapshots))
        .route(
            "/api/v1/experiments/:name/runs",
            get(list_runs).post(start_run),
        )
        .route("/api/v1/experiments/:name/runs/:run_id/logs", get(get_logs))
        .route(
            "/api/v1/experiments/:name/runs/:run_id/stop",
            axum::routing::post(stop_run),
        )
        .with_state(state)
        .layer(middleware::from_fn_with_state(token, require_bearer_token))
        .layer(cors)
}


fn safe_workspace_path(root: &Path, relative: &str) -> Result<PathBuf, ApiError> {
    if relative.contains('\0') || relative.contains('\\') {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid workspace path"));
    }
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || (!relative.is_empty() && relative_path.components().any(|part| !matches!(part, Component::Normal(_))))
    {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "workspace path must be relative"));
    }
    let canonical_root = root.canonicalize().map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let mut candidate = canonical_root.clone();
    for part in relative_path.components() {
        candidate.push(part);
        let metadata = fs::symlink_metadata(&candidate).map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "symbolic links are not exposed by the workspace browser"));
        }
    }
    if !candidate.starts_with(&canonical_root) {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "workspace path escaped its root"));
    }
    Ok(candidate)
}

async fn list_files(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
    Query(query): Query<PathQuery>,
) -> Result<Json<Vec<FileEntryView>>, ApiError> {
    let (_, workspace) = experiment_workspace(&state.data_dir, &name)?;
    let directory = safe_workspace_path(&workspace, &query.path)?;
    if !directory.is_dir() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "requested path is not a directory"));
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory).map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))? {
        let entry = entry.map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if query.path.is_empty() { name.clone() } else { format!("{}/{}", query.path.trim_end_matches('/'), name) };
        let kind = if metadata.file_type().is_symlink() { "symlink" } else if metadata.is_dir() { "directory" } else if metadata.is_file() { "file" } else { "special" };
        let modified_at = metadata.modified().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map(|duration| duration.as_secs());
        entries.push(FileEntryView { name, path, kind, size: metadata.len(), modified_at });
    }
    entries.sort_by(|a, b| (a.kind != "directory", a.name.to_lowercase()).cmp(&(b.kind != "directory", b.name.to_lowercase())));
    Ok(Json(entries))
}

async fn read_file(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
    Query(query): Query<PathQuery>,
) -> Result<Json<FileContentView>, ApiError> {
    let (_, workspace) = experiment_workspace(&state.data_dir, &name)?;
    if query.path.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "a file path is required"));
    }
    let path = safe_workspace_path(&workspace, &query.path)?;
    let metadata = fs::metadata(&path).map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    if !metadata.is_file() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "requested path is not a regular file"));
    }
    const MAX_PREVIEW_BYTES: u64 = 262_144;
    if metadata.len() > MAX_PREVIEW_BYTES {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "file preview is limited to 256 KiB"));
    }
    let bytes = fs::read(&path).map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let content = String::from_utf8(bytes).map_err(|_| ApiError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "binary files cannot be previewed"))?;
    Ok(Json(FileContentView { path: query.path, bytes: content.len(), content }))
}

async fn verify_snapshot(
    State(state): State<ApiState>,
    AxumPath((name, snapshot_id)): AxumPath<(String, String)>,
) -> Result<Json<SnapshotView>, ApiError> {
    Ok(Json(snapshot::verify(&state.data_dir, &name, &snapshot_id)?.into()))
}

async fn compare_snapshots(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
    Query(query): Query<CompareQuery>,
) -> Result<Json<Vec<DifferenceView>>, ApiError> {
    let differences = snapshot::compare(&state.data_dir, &name, &query.a, &query.b)?;
    Ok(Json(differences.into_iter().map(|item| DifferenceView { path: item.path, change: item.change }).collect()))
}


fn plugin_root(data_dir: &Path) -> Result<PathBuf, ApiError> {
    let root = data_dir.join("plugins");
    match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() =>
            return Err(ApiError::new(StatusCode::FORBIDDEN, "plugin registry path is not a real directory")),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound =>
            fs::create_dir(&root).map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        Err(error) => return Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())),
    }
    root.canonicalize().map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn plugin_directory(data_dir: &Path, id: &str) -> Result<(PathBuf, PathBuf), ApiError> {
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid plugin identifier"));
    }
    let root = plugin_root(data_dir)?;
    let directory = root.join(id);
    let metadata = fs::symlink_metadata(&directory)
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "plugin not found"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "plugin registry entry is not a real directory"));
    }
    let canonical = directory.canonicalize()
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !canonical.starts_with(&root) {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "plugin registry path escaped its root"));
    }
    Ok((root, canonical))
}

fn read_plugin(directory: &Path) -> Result<PluginView, ApiError> {
    let manifest_path = directory.join("plugin.json");
    let metadata = fs::symlink_metadata(&manifest_path)
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "plugin manifest not found"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "plugin manifest is not a regular file"));
    }
    let bytes = fs::read(&manifest_path)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let manifest: PluginManifest = serde_json::from_slice(&bytes)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, format!("invalid installed plugin manifest: {e}")))?;
    manifest.validate()
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    let grants_path = directory.join("grants.json");
    let granted_permissions = match fs::symlink_metadata(&grants_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() =>
            return Err(ApiError::new(StatusCode::FORBIDDEN, "plugin grants are not a regular file")),
        Ok(_) => serde_json::from_slice(&fs::read(grants_path)
            .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?)
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, format!("invalid plugin grants: {e}")))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())),
    };
    Ok(PluginView { manifest, granted_permissions })
}

async fn list_plugins(State(state): State<ApiState>) -> Result<Json<Vec<PluginView>>, ApiError> {
    let root = plugin_root(&state.data_dir)?;
    let mut plugins = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))? {
        let entry = entry.map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() { continue; }
        plugins.push(read_plugin(&entry.path())?);
    }
    plugins.sort_by(|a, b| a.manifest.name.to_lowercase().cmp(&b.manifest.name.to_lowercase()));
    Ok(Json(plugins))
}

async fn install_plugin(
    State(state): State<ApiState>,
    Json(request): Json<PluginInstallRequest>,
) -> Result<(StatusCode, Json<PluginView>), ApiError> {
    request.manifest.validate()
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    let root = plugin_root(&state.data_dir)?;
    let directory = root.join(&request.manifest.id);
    fs::create_dir(&directory)
        .map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists {
            ApiError::new(StatusCode::CONFLICT, "plugin is already installed")
        } else { ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()) })?;
    let result = (|| -> Result<(), ApiError> {
        let manifest = serde_json::to_vec_pretty(&request.manifest)
            .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        fs::write(directory.join("plugin.json"), manifest)
            .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        fs::write(directory.join("grants.json"), b"[]")
            .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&directory);
        return Err(error);
    }
    Ok((StatusCode::CREATED, Json(read_plugin(&directory)?)))
}

async fn update_plugin_grants(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<PluginGrantRequest>,
) -> Result<Json<PluginView>, ApiError> {
    let (_, directory) = plugin_directory(&state.data_dir, &id)?;
    let mut plugin = read_plugin(&directory)?;
    for permission in &request.permissions {
        if !plugin.manifest.permissions.contains(permission) {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, "cannot grant a permission not declared by the plugin"));
        }
    }
    let unique: std::collections::BTreeSet<_> = request.permissions.iter().collect();
    if unique.len() != request.permissions.len() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "duplicate permissions are not allowed"));
    }
    let temporary = directory.join("grants.json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(&request.permissions)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    fs::rename(&temporary, directory.join("grants.json"))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    plugin.granted_permissions = request.permissions;
    Ok(Json(plugin))
}

async fn remove_plugin(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    let (_, directory) = plugin_directory(&state.data_dir, &id)?;
    fs::remove_dir_all(directory)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn health(State(_state): State<ApiState>) -> Json<HealthView> {
    let host = PlatformInfo::detect();
    Json(HealthView {
        status: "ok",
        api_version: 1,
        host_os: host.operating_system,
        host_arch: host.architecture,
    })
}

async fn list(State(state): State<ApiState>) -> Result<Json<Vec<ExperimentView>>, ApiError> {
    let items = list_experiments(&state.data_dir)?;
    Ok(Json(items.into_iter().map(ExperimentView::from).collect()))
}

async fn create(
    State(state): State<ApiState>,
    Json(request): Json<CreateExperimentRequest>,
) -> Result<(StatusCode, Json<ExperimentView>), ApiError> {
    let mut manifest = Manifest::minimal(request.name.clone());
    manifest.experiment.description = request.description;
    let item = create_experiment(&state.data_dir, &request.name, manifest)?;
    Ok((StatusCode::CREATED, Json(item.into())))
}

async fn inspect(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<ExperimentView>, ApiError> {
    Ok(Json(inspect_experiment(&state.data_dir, &name)?.into()))
}

async fn remove(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    delete_experiment(&state.data_dir, &name)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_snapshots(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<Vec<SnapshotView>>, ApiError> {
    let items = snapshot::list(&state.data_dir, &name)?;
    Ok(Json(items.into_iter().map(SnapshotView::from).collect()))
}

async fn create_snapshot(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> Result<(StatusCode, Json<SnapshotView>), ApiError> {
    let item = snapshot::create(&state.data_dir, &name)?;
    Ok((StatusCode::CREATED, Json(item.into())))
}

async fn restore_snapshot(
    State(state): State<ApiState>,
    AxumPath((name, snapshot_id)): AxumPath<(String, String)>,
) -> Result<StatusCode, ApiError> {
    snapshot::restore(&state.data_dir, &name, &snapshot_id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_runs(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<Vec<RunView>>, ApiError> {
    let (experiment, _) = experiment_workspace(&state.data_dir, &name)?;
    let records = runner::list_runs(&run_root(&state.data_dir, &experiment))?;
    let mut views = Vec::with_capacity(records.len());
    for record in records {
        views.push(run_view(record)?);
    }
    Ok(Json(views))
}

async fn start_run(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
    Json(request): Json<RunRequest>,
) -> Result<(StatusCode, Json<RunView>), ApiError> {
    let (experiment, workspace) = experiment_workspace(&state.data_dir, &name)?;
    let record = runner::launch(
        &workspace,
        &run_root(&state.data_dir, &experiment),
        &request.command,
    )?;
    Ok((StatusCode::CREATED, Json(run_view(record)?)))
}

async fn get_logs(
    State(state): State<ApiState>,
    AxumPath((name, run_id)): AxumPath<(String, String)>,
) -> Result<Json<LogsView>, ApiError> {
    let (experiment, _) = experiment_workspace(&state.data_dir, &name)?;
    let content = runner::logs(&run_root(&state.data_dir, &experiment), &run_id)?;
    const MAX_LOG_BYTES: usize = 1_048_576;
    let truncated = content.len() > MAX_LOG_BYTES;
    let content = if truncated {
        let mut start = content.len() - MAX_LOG_BYTES;
        while !content.is_char_boundary(start) {
            start += 1;
        }
        content[start..].to_owned()
    } else {
        content
    };
    Ok(Json(LogsView {
        run_id,
        content,
        truncated,
    }))
}

async fn stop_run(
    State(state): State<ApiState>,
    AxumPath((name, run_id)): AxumPath<(String, String)>,
) -> Result<Json<StopView>, ApiError> {
    let (experiment, _) = experiment_workspace(&state.data_dir, &name)?;
    runner::stop(&run_root(&state.data_dir, &experiment), &run_id)?;
    Ok(Json(StopView {
        run_id,
        stopped: true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use cathedral_core::initialize;
    use tempfile::tempdir;
    use tower::ServiceExt;

    fn test_router(path: &Path) -> Router {
        initialize(path).unwrap();
        router(path.to_path_buf(), "studio-test-token".to_owned())
    }

    #[tokio::test]
    async fn api_rejects_requests_without_a_bearer_token() {
        let temp = tempdir().unwrap();
        let response = test_router(temp.path())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_accepts_a_valid_token_and_reports_health() {
        let temp = tempdir().unwrap();
        let response = test_router(temp.path())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .header(AUTHORIZATION, "Bearer studio-test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }


    #[tokio::test]
    async fn workspace_browser_lists_and_previews_files() {
        let temp = tempdir().unwrap();
        let experiment = cathedral_core::create_experiment(
            temp.path(),
            "browser-lab",
            Manifest::minimal("browser-lab"),
        )
        .unwrap();
        std::fs::write(Path::new(&experiment.workspace_path).join("notes.txt"), "hello Cathedral").unwrap();
        let app = test_router(temp.path());
        let listing = app.clone().oneshot(
            Request::builder()
                .uri("/api/v1/experiments/browser-lab/files")
                .header(AUTHORIZATION, "Bearer studio-test-token")
                .body(Body::empty())
                .unwrap(),
        ).await.unwrap();
        assert_eq!(listing.status(), StatusCode::OK);
        let body = axum::body::to_bytes(listing.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8(body.to_vec()).unwrap().contains("notes.txt"));

        let preview = app.oneshot(
            Request::builder()
                .uri("/api/v1/experiments/browser-lab/files/content?path=notes.txt")
                .header(AUTHORIZATION, "Bearer studio-test-token")
                .body(Body::empty())
                .unwrap(),
        ).await.unwrap();
        assert_eq!(preview.status(), StatusCode::OK);
        let body = axum::body::to_bytes(preview.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8(body.to_vec()).unwrap().contains("hello Cathedral"));
    }

    #[tokio::test]
    async fn workspace_browser_rejects_parent_traversal() {
        let temp = tempdir().unwrap();
        cathedral_core::create_experiment(
            temp.path(),
            "browser-safe",
            Manifest::minimal("browser-safe"),
        ).unwrap();
        let response = test_router(temp.path()).oneshot(
            Request::builder()
                .uri("/api/v1/experiments/browser-safe/files?path=..%2F..")
                .header(AUTHORIZATION, "Bearer studio-test-token")
                .body(Body::empty())
                .unwrap(),
        ).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn snapshot_verification_and_comparison_are_exposed() {
        let temp = tempdir().unwrap();
        let experiment = cathedral_core::create_experiment(
            temp.path(),
            "snapshot-api",
            Manifest::minimal("snapshot-api"),
        ).unwrap();
        let workspace = Path::new(&experiment.workspace_path);
        std::fs::write(workspace.join("value.txt"), "first").unwrap();
        let first = snapshot::create(temp.path(), "snapshot-api").unwrap();
        std::fs::write(workspace.join("value.txt"), "second").unwrap();
        let second = snapshot::create(temp.path(), "snapshot-api").unwrap();
        let app = test_router(temp.path());
        let verified = app.clone().oneshot(
            Request::builder()
                .uri(format!("/api/v1/experiments/snapshot-api/snapshots/{}/verify", first.id))
                .header(AUTHORIZATION, "Bearer studio-test-token")
                .body(Body::empty())
                .unwrap(),
        ).await.unwrap();
        assert_eq!(verified.status(), StatusCode::OK);
        let compared = app.oneshot(
            Request::builder()
                .uri(format!("/api/v1/experiments/snapshot-api/snapshots/compare?a={}&b={}", first.id, second.id))
                .header(AUTHORIZATION, "Bearer studio-test-token")
                .body(Body::empty())
                .unwrap(),
        ).await.unwrap();
        assert_eq!(compared.status(), StatusCode::OK);
        let body = axum::body::to_bytes(compared.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8(body.to_vec()).unwrap().contains("changed"));
    }

    #[tokio::test]
    async fn plugin_registry_installs_and_revokes_declared_permissions() {
        let temp = tempdir().unwrap();
        let app = test_router(temp.path());
        let manifest = PluginManifest {
            schema_version: 1,
            id: "sample-inspector".to_owned(),
            name: "Sample Inspector".to_owned(),
            version: "1.0.0".to_owned(),
            api_version: 1,
            description: "Reads experiment metadata".to_owned(),
            permissions: vec![PluginPermission::ReadExperiments, PluginPermission::ReadRunLogs],
        };
        let response = app.clone().oneshot(
            Request::builder().method(Method::POST).uri("/api/v1/plugins")
                .header(AUTHORIZATION, "Bearer studio-test-token")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&serde_json::json!({"manifest": manifest})).unwrap())).unwrap()
        ).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let grant = app.clone().oneshot(
            Request::builder().method(Method::PUT).uri("/api/v1/plugins/sample-inspector/grants")
                .header(AUTHORIZATION, "Bearer studio-test-token")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"permissions":["read_experiments"]}"#)).unwrap()
        ).await.unwrap();
        assert_eq!(grant.status(), StatusCode::OK);
        let forbidden_grant = app.clone().oneshot(
            Request::builder().method(Method::PUT).uri("/api/v1/plugins/sample-inspector/grants")
                .header(AUTHORIZATION, "Bearer studio-test-token")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"permissions":["restore_snapshots"]}"#)).unwrap()
        ).await.unwrap();
        assert_eq!(forbidden_grant.status(), StatusCode::BAD_REQUEST);
        let removed = app.oneshot(
            Request::builder().method(Method::DELETE).uri("/api/v1/plugins/sample-inspector")
                .header(AUTHORIZATION, "Bearer studio-test-token")
                .body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(removed.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn api_creates_an_experiment() {
        let temp = tempdir().unwrap();
        let response = test_router(temp.path())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/experiments")
                    .header(AUTHORIZATION, "Bearer studio-test-token")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"name":"studio-lab","description":"Created by Studio"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }
}
