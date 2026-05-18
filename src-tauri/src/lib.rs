use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::Utc;
use regex::Regex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::c_void,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    ptr,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use tauri::{Manager, PhysicalSize, Size, WindowEvent};
use walkdir::WalkDir;

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::LocalFree;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts, INTERNET_DEFAULT_HTTPS_PORT,
    INTERNET_DEFAULT_HTTP_PORT, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_ADDREQ_FLAG_ADD,
    WINHTTP_FLAG_SECURE, WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "wmv", "mov", "ts", "m4v"];
const INCOMPLETE_EXTENSIONS: &[&str] = &["xltd", "part", "tmp", "!qb", "crdownload"];
const DOWNLOAD_FOLDER: &str = "_downloading";
const APPRECIATION_FOLDER: &str = "_appreciation";
const REVIEW_FOLDER: &str = "_duplicates_review";
const MIN_WINDOW_WIDTH: u32 = 1248;
const MIN_WINDOW_HEIGHT: u32 = 816;
const WINDOW_STATE_FILE: &str = "window-state.json";
const COVER_FOLDER: &str = "covers";
const DEFAULT_COVER_CAPTURE_PERCENT: f64 = 10.0;
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

static APP_RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

struct AppState {
    db: Mutex<Connection>,
    data_dir: PathBuf,
    cover_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct AppWindowSize {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Default)]
struct MetadataSession {
    cookie_header: String,
    source: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct LibraryFile {
    id: i64,
    name: String,
    path: String,
    parent: String,
    extension: String,
    size_bytes: i64,
    size_gb: f64,
    modified: String,
    #[serde(default)]
    modified_unix: i64,
    depth: usize,
    media_code: Option<String>,
    #[serde(default)]
    cover_path: Option<String>,
    is_root_file: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct DuplicateGroup {
    key: String,
    count: usize,
    total_size_gb: f64,
    files: Vec<LibraryFile>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct NumericDuplicateGroup {
    folder: String,
    number: String,
    count: usize,
    files: Vec<LibraryFile>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct MoveSuggestion {
    file: LibraryFile,
    target_folder: String,
    reason: String,
    confidence: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct RenamePreview {
    id: String,
    path: String,
    directory: String,
    current_name: String,
    new_name: String,
    target_path: String,
    rules: Vec<String>,
    status: String,
    reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct DownloadMovePlan {
    id: String,
    source_path: String,
    source_name: String,
    item_type: String,
    folder: String,
    folder_path: String,
    target_file_name: String,
    target_path: String,
    target_relative: String,
    file_count: usize,
    has_xltd: bool,
    status: String,
    reason: String,
    files: Vec<LibraryFile>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ActorVideo {
    path: String,
    name: String,
    size_gb: f64,
    modified: String,
    #[serde(default)]
    modified_unix: i64,
    media_code: Option<String>,
    #[serde(default)]
    cover_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ActorSummary {
    name: String,
    path: String,
    file_count: usize,
    total_size_gb: f64,
    modified: String,
    modified_unix: i64,
    roles: Vec<String>,
    videos: Vec<ActorVideo>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ActorFolderSnapshot {
    path: String,
    modified_unix: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ProcessingOptions {
    move_to_appreciation: bool,
    rename_enabled: bool,
    uppercase: bool,
    normalize_dash: bool,
    #[serde(default)]
    normalize_uncensored_suffix: bool,
    remove_at_prefix: bool,
    skip_if_exists: bool,
    #[serde(default)]
    delete_source_folder_after_move: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct DownloadProcessingRequest {
    id: String,
    source_path: String,
    target_path: String,
    status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct StaleFolder {
    folder: String,
    path: String,
    latest_file: String,
    latest_modified: String,
    days_since: i64,
    is_stale: bool,
    file_count: usize,
    suggestion: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct LargestFile {
    rank: usize,
    file: LibraryFile,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct OperationLog {
    id: i64,
    timestamp: String,
    action: String,
    source_path: String,
    target_path: String,
    status: String,
    message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct OperationResult {
    success: usize,
    skipped: usize,
    failed: usize,
    logs: Vec<OperationLog>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ArchiveLookup {
    code: String,
    status: String,
    actor_name: Option<String>,
    actor_path: Option<String>,
    avatar_path: Option<String>,
    source_url: Option<String>,
    message: String,
    searched_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ArchiveMovePreview {
    source_path: String,
    target_path: String,
    actor_name: String,
    status: String,
    message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ActorProfile {
    actor_name: String,
    actor_path: String,
    avatar_path: Option<String>,
    avatar_source: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct MetadataSessionStatus {
    site_host: String,
    has_cookie: bool,
    source: Option<String>,
    status: String,
    updated_at: Option<String>,
    message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct RenameRequest {
    path: String,
    target_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct LibraryStats {
    root_path: String,
    file_count: usize,
    folder_count: usize,
    root_file_count: usize,
    video_count: usize,
    total_size_gb: f64,
    scanned_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ScanReport {
    stats: LibraryStats,
    #[serde(default)]
    actors: Vec<ActorSummary>,
    #[serde(default)]
    actor_snapshots: Vec<ActorFolderSnapshot>,
    #[serde(default)]
    appreciation_path: String,
    #[serde(default)]
    appreciation_videos: Vec<LibraryFile>,
    #[serde(default)]
    appreciation_snapshot_unix: i64,
    root_files: Vec<LibraryFile>,
    name_duplicates: Vec<DuplicateGroup>,
    code_duplicates: Vec<DuplicateGroup>,
    #[serde(default)]
    numeric_duplicates: Vec<NumericDuplicateGroup>,
    move_suggestions: Vec<MoveSuggestion>,
    largest_folders: Vec<FolderSummary>,
    #[serde(default)]
    largest_files: Vec<LargestFile>,
    #[serde(default)]
    rename_previews: Vec<RenamePreview>,
    #[serde(default)]
    download_move_plans: Vec<DownloadMovePlan>,
    #[serde(default)]
    stale_folders: Vec<StaleFolder>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct FolderSummary {
    name: String,
    path: String,
    file_count: usize,
    size_gb: f64,
    modified: String,
    #[serde(default)]
    modified_unix: i64,
}

#[tauri::command]
fn scan_library(
    root_path: String,
    download_path: Option<String>,
    appreciation_path: Option<String>,
    cover_percent: Option<f64>,
    options: Option<ProcessingOptions>,
    state: tauri::State<'_, AppState>,
) -> Result<ScanReport, String> {
    let root = PathBuf::from(root_path);
    let processing_options = options.unwrap_or_else(default_processing_options);
    let report = scan_root(
        &root,
        download_path.as_deref(),
        appreciation_path.as_deref(),
        &processing_options,
        normalize_cover_capture_percent(cover_percent),
        &state.cover_dir,
    )
    .map_err(|error| error.to_string())?;

    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    persist_report(&db, &report).map_err(|error| error.to_string())?;

    Ok(report)
}

#[tauri::command]
fn load_latest_report(state: tauri::State<'_, AppState>) -> Result<Option<ScanReport>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;

    load_report(&db).map_err(|error| error.to_string())
}

#[tauri::command]
async fn ensure_video_cover(
    root_path: String,
    file_path: String,
    cover_percent: Option<f64>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let file =
        ensure_existing_file_under_root(&root, &file_path).map_err(|error| error.to_string())?;
    let extension = file
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !is_video_extension(&extension) {
        return Ok(None);
    }

    let cover_dir = state.cover_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let metadata = fs::metadata(&file).ok()?;
        ensure_cover_for_video(
            &file,
            &metadata,
            &cover_dir,
            normalize_cover_capture_percent(cover_percent),
        )
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn load_cached_library(
    root_path: String,
    download_path: Option<String>,
    appreciation_path: Option<String>,
    cover_percent: Option<f64>,
    options: Option<ProcessingOptions>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<ScanReport>, String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let processing_options = options.unwrap_or_else(default_processing_options);
    let mut report = {
        let db = state
            .db
            .lock()
            .map_err(|_| "Database lock is poisoned".to_string())?;
        load_report(&db).map_err(|error| error.to_string())?
    };

    let Some(mut report) = report.take() else {
        return Ok(None);
    };

    let cached_root = PathBuf::from(&report.stats.root_path);
    if !paths_same_ignore_case(&cached_root, &root) {
        return Ok(None);
    }

    let changed = refresh_cached_report(
        &mut report,
        &root,
        download_path.as_deref(),
        appreciation_path.as_deref(),
        &processing_options,
        normalize_cover_capture_percent(cover_percent),
        &state.cover_dir,
    )
    .map_err(|error| error.to_string())?;

    if changed {
        let db = state
            .db
            .lock()
            .map_err(|_| "Database lock is poisoned".to_string())?;
        persist_report(&db, &report).map_err(|error| error.to_string())?;
    }

    Ok(Some(report))
}

#[tauri::command]
fn load_operation_logs(state: tauri::State<'_, AppState>) -> Result<Vec<OperationLog>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;

    load_logs(&db, 100).map_err(|error| error.to_string())
}

#[tauri::command]
fn preview_download_move_plan(
    root_path: String,
    download_path: Option<String>,
    appreciation_path: Option<String>,
    options: ProcessingOptions,
) -> Result<Vec<DownloadMovePlan>, String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let download_dir = resolve_configured_dir(&root, download_path.as_deref(), DOWNLOAD_FOLDER)
        .map_err(|error| error.to_string())?;
    let appreciation_dir =
        resolve_configured_dir(&root, appreciation_path.as_deref(), APPRECIATION_FOLDER)
            .map_err(|error| error.to_string())?;
    Ok(build_download_move_plans(
        &download_dir,
        &appreciation_dir,
        &options,
    ))
}

#[tauri::command]
fn open_media_file(root_path: String, file_path: String) -> Result<(), String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let file =
        ensure_existing_file_under_root(&root, &file_path).map_err(|error| error.to_string())?;
    open_with_system(&file).map_err(|error| error.to_string())
}

#[tauri::command]
fn open_file_location(root_path: String, file_path: String) -> Result<(), String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let file =
        ensure_existing_file_under_root(&root, &file_path).map_err(|error| error.to_string())?;
    reveal_in_file_manager(&file).map_err(|error| error.to_string())
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    open_external(&url).map_err(|error| error.to_string())
}

#[tauri::command]
fn execute_rename_plan(
    root_path: String,
    operations: Vec<RenameRequest>,
    state: tauri::State<'_, AppState>,
) -> Result<OperationResult, String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let mut result = OperationResult::default();

    for operation in operations {
        let outcome = rename_one(&root, &operation.path, &operation.target_path);
        push_outcome(
            &mut result,
            "rename",
            &operation.path,
            &operation.target_path,
            outcome,
        );
    }

    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    persist_logs(&db, &mut result.logs).map_err(|error| error.to_string())?;

    Ok(result)
}

#[tauri::command]
fn execute_review_move(
    root_path: String,
    file_paths: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<OperationResult, String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let review_dir = root.join(REVIEW_FOLDER);
    fs::create_dir_all(&review_dir).map_err(|error| error.to_string())?;
    let mut result = OperationResult::default();

    for file_path in file_paths {
        let target = PathBuf::from(&file_path)
            .file_name()
            .map(|name| review_dir.join(name))
            .unwrap_or_else(|| review_dir.join("unknown"));
        let target_text = target.to_string_lossy().to_string();
        let outcome = move_one_file(&root, &file_path, &target_text);
        push_outcome(
            &mut result,
            "move_to_duplicate_review",
            &file_path,
            &target_text,
            outcome,
        );
    }

    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    persist_logs(&db, &mut result.logs).map_err(|error| error.to_string())?;

    Ok(result)
}

#[tauri::command]
fn execute_download_move_plan(
    root_path: String,
    download_path: Option<String>,
    appreciation_path: Option<String>,
    requests: Vec<DownloadProcessingRequest>,
    options: ProcessingOptions,
    state: tauri::State<'_, AppState>,
) -> Result<OperationResult, String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let download_dir = resolve_configured_dir(&root, download_path.as_deref(), DOWNLOAD_FOLDER)
        .map_err(|error| error.to_string())?;
    let appreciation_dir =
        resolve_configured_dir(&root, appreciation_path.as_deref(), APPRECIATION_FOLDER)
            .map_err(|error| error.to_string())?;
    let mut result = OperationResult::default();

    for request in requests {
        if request.status != "ready" {
            push_outcome(
                &mut result,
                "move_to_appreciation",
                &request.source_path,
                &request.target_path,
                Err(anyhow::anyhow!("该项状态不是 ready，已跳过")),
            );
            continue;
        }
        let outcome =
            process_download_item(&root, &download_dir, &appreciation_dir, &request, &options);
        push_outcome(
            &mut result,
            "move_to_appreciation",
            &request.source_path,
            &request.target_path,
            outcome,
        );
    }

    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    persist_logs(&db, &mut result.logs).map_err(|error| error.to_string())?;

    Ok(result)
}

#[tauri::command]
fn get_archive_lookup(
    code: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<ArchiveLookup>, String> {
    let normalized_code = normalize_archive_code(&code).map_err(|error| error.to_string())?;
    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    load_archive_lookup(&db, &normalized_code).map_err(|error| error.to_string())
}

#[tauri::command]
fn search_archive_metadata(
    root_path: String,
    code: String,
    site_url: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<ArchiveLookup, String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let normalized_code = normalize_archive_code(&code).map_err(|error| error.to_string())?;
    let actors = read_direct_folders(&root)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|folder| !is_system_folder(&folder.name))
        .collect::<Vec<_>>();
    if normalized_code.starts_with("FC2PPV-") {
        let actor_path = actors
            .iter()
            .find(|actor| actor.name.eq_ignore_ascii_case("fc2"))
            .map(|actor| actor.path.clone())
            .unwrap_or_else(|| root.join("fc2").to_string_lossy().to_string());
        let result = ArchiveLookup {
            code: normalized_code,
            status: "searched".to_string(),
            actor_name: Some("fc2".to_string()),
            actor_path: Some(actor_path),
            avatar_path: None,
            source_url: None,
            message: "FC2 番号自动归档到 fc2 文件夹".to_string(),
            searched_at: now_string(),
        };
        let db = state
            .db
            .lock()
            .map_err(|_| "Database lock is poisoned".to_string())?;
        persist_archive_lookup(&db, &result).map_err(|error| error.to_string())?;
        return Ok(result);
    }
    let site = site_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("www.javbus.com");
    let site_host = normalize_site_host(site);
    let session = {
        let db = state
            .db
            .lock()
            .map_err(|_| "Database lock is poisoned".to_string())?;
        load_metadata_session(&db, &site_host).map_err(|error| error.to_string())?
    };

    let result = lookup_javbus_metadata(
        site,
        &normalized_code,
        &actors,
        &state.data_dir,
        session.as_ref(),
    )
    .unwrap_or_else(|error| ArchiveLookup {
        code: normalized_code.clone(),
        status: "searched".to_string(),
        actor_name: None,
        actor_path: None,
        avatar_path: None,
        source_url: Some(build_metadata_search_url(site, &normalized_code)),
        message: format!("联网搜索失败：{}", error),
        searched_at: now_string(),
    });

    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    if result.message.contains("年龄验证页") {
        touch_metadata_session_status(
            &db,
            &site_host,
            "blocked",
            "JavBus 返回年龄验证页，请先在设置中导入浏览器会话，或打开验证页完成验证后再导入",
        )
        .map_err(|error| error.to_string())?;
    }
    persist_archive_lookup(&db, &result).map_err(|error| error.to_string())?;
    if let (Some(actor_name), Some(actor_path)) = (&result.actor_name, &result.actor_path) {
        persist_actor_profile(
            &db,
            &ActorProfile {
                actor_name: actor_name.clone(),
                actor_path: actor_path.clone(),
                avatar_path: result.avatar_path.clone(),
                avatar_source: result.source_url.clone(),
                updated_at: result.searched_at.clone(),
            },
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(result)
}

#[tauri::command]
fn preview_archive_move(
    root_path: String,
    appreciation_path: Option<String>,
    video_path: String,
    actor_path: String,
) -> Result<ArchiveMovePreview, String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let appreciation_dir =
        resolve_configured_dir(&root, appreciation_path.as_deref(), APPRECIATION_FOLDER)
            .map_err(|error| error.to_string())?;
    build_archive_move_preview(&root, &appreciation_dir, &video_path, &actor_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn execute_archive_move(
    root_path: String,
    appreciation_path: Option<String>,
    video_path: String,
    actor_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<OperationResult, String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let appreciation_dir =
        resolve_configured_dir(&root, appreciation_path.as_deref(), APPRECIATION_FOLDER)
            .map_err(|error| error.to_string())?;
    let preview = build_archive_move_preview(&root, &appreciation_dir, &video_path, &actor_path)
        .map_err(|error| error.to_string())?;
    let mut result = OperationResult::default();

    let outcome = archive_move_from_preview(&root, &preview);
    push_outcome(
        &mut result,
        "archive_to_actor",
        &preview.source_path,
        &preview.target_path,
        outcome,
    );

    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    persist_logs(&db, &mut result.logs).map_err(|error| error.to_string())?;

    Ok(result)
}

#[tauri::command]
fn execute_archive_move_to_new_actor(
    root_path: String,
    appreciation_path: Option<String>,
    video_path: String,
    actor_name: String,
    state: tauri::State<'_, AppState>,
) -> Result<OperationResult, String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let appreciation_dir =
        resolve_configured_dir(&root, appreciation_path.as_deref(), APPRECIATION_FOLDER)
            .map_err(|error| error.to_string())?;
    let actor_dir = actor_dir_for_name(&root, &actor_name).map_err(|error| error.to_string())?;
    let actor_path = actor_dir.to_string_lossy().to_string();
    let preview = build_archive_move_preview(&root, &appreciation_dir, &video_path, &actor_path)
        .map_err(|error| error.to_string())?;
    let mut result = OperationResult::default();

    let outcome = archive_move_from_preview(&root, &preview);
    push_outcome(
        &mut result,
        "archive_to_new_actor",
        &preview.source_path,
        &preview.target_path,
        outcome,
    );

    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    if result.success > 0 {
        let actor_name = actor_dir
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("fc2")
            .to_string();
        persist_actor_profile(
            &db,
            &ActorProfile {
                actor_name,
                actor_path,
                avatar_path: None,
                avatar_source: None,
                updated_at: now_string(),
            },
        )
        .map_err(|error| error.to_string())?;
    }
    persist_logs(&db, &mut result.logs).map_err(|error| error.to_string())?;

    Ok(result)
}

#[tauri::command]
fn delete_archive_video(
    root_path: String,
    appreciation_path: Option<String>,
    video_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<OperationResult, String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let appreciation_dir =
        resolve_configured_dir(&root, appreciation_path.as_deref(), APPRECIATION_FOLDER)
            .map_err(|error| error.to_string())?;
    let mut result = OperationResult::default();
    let outcome = delete_archive_file(&root, &appreciation_dir, &video_path);
    push_outcome(
        &mut result,
        "delete_archive_video",
        &video_path,
        "",
        outcome,
    );

    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    persist_logs(&db, &mut result.logs).map_err(|error| error.to_string())?;

    Ok(result)
}
#[tauri::command]
fn load_actor_profiles(state: tauri::State<'_, AppState>) -> Result<Vec<ActorProfile>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    load_profiles(&db).map_err(|error| error.to_string())
}

#[tauri::command]
fn load_metadata_session_status(
    site_url: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<MetadataSessionStatus, String> {
    let site_host = normalize_site_host(site_url.as_deref().unwrap_or("www.javbus.com"));
    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    load_metadata_session_status_row(&db, &site_host)
        .map(|session| metadata_session_status_from_row(&site_host, session))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn import_browser_metadata_session(
    site_url: Option<String>,
    browser: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<MetadataSessionStatus, String> {
    let site_host = normalize_site_host(site_url.as_deref().unwrap_or("www.javbus.com"));
    let session = import_browser_session_for_site(&site_host, browser.as_deref(), &state.data_dir)
        .map_err(|error| error.to_string())?;
    let status = MetadataSessionStatus {
        site_host: site_host.clone(),
        has_cookie: true,
        source: Some(session.source.clone()),
        status: "verified".to_string(),
        updated_at: Some(session.updated_at.clone()),
        message: "浏览器会话导入成功，可用于 JavBus 联网识别".to_string(),
    };
    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    persist_metadata_session(
        &db,
        &site_host,
        &session.cookie_header,
        &session.source,
        "verified",
        &status.message,
        &session.updated_at,
    )
    .map_err(|error| error.to_string())?;
    Ok(status)
}

#[tauri::command]
fn clear_metadata_session(
    site_url: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let site_host = normalize_site_host(site_url.as_deref().unwrap_or("www.javbus.com"));
    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    delete_metadata_session(&db, &site_host).map_err(|error| error.to_string())
}

#[tauri::command]
fn save_manual_metadata_session(
    site_url: Option<String>,
    cookie_header: String,
    state: tauri::State<'_, AppState>,
) -> Result<MetadataSessionStatus, String> {
    let site_host = normalize_site_host(site_url.as_deref().unwrap_or("www.javbus.com"));
    let normalized_cookie = normalize_cookie_header_value(&cookie_header);
    if normalized_cookie.is_empty() {
        return Err("cookie 不能为空".to_string());
    }
    let session = MetadataSession {
        cookie_header: normalized_cookie,
        source: "manual".to_string(),
        updated_at: now_string(),
    };
    let probe_url = build_metadata_search_url(&site_host, "IPZZ-832");
    let probe_html = http_get_text_with_session(&probe_url, Some(&session))
        .map_err(|error| error.to_string())?;
    let status = if is_age_verification_page(&probe_html) {
        MetadataSessionStatus {
            site_host: site_host.clone(),
            has_cookie: true,
            source: Some(session.source.clone()),
            status: "blocked".to_string(),
            updated_at: Some(session.updated_at.clone()),
            message: "已保存 cookie，但站点仍返回年龄验证页".to_string(),
        }
    } else {
        MetadataSessionStatus {
            site_host: site_host.clone(),
            has_cookie: true,
            source: Some(session.source.clone()),
            status: "verified".to_string(),
            updated_at: Some(session.updated_at.clone()),
            message: "手动会话保存成功，可用于 JavBus 联网识别".to_string(),
        }
    };
    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    persist_metadata_session(
        &db,
        &site_host,
        &session.cookie_header,
        &session.source,
        &status.status,
        &status.message,
        &session.updated_at,
    )
    .map_err(|error| error.to_string())?;
    Ok(status)
}

#[tauri::command]
fn refresh_actor_avatar(
    root_path: String,
    actor_path: String,
    site_url: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<ActorProfile, String> {
    let root = canonical_root(&root_path).map_err(|error| error.to_string())?;
    let actor_dir =
        ensure_existing_dir_under_root(&root, &actor_path).map_err(|error| error.to_string())?;
    if is_reserved_actor_dir(&root, &actor_dir) {
        return Err("系统目录不能作为演员目录".to_string());
    }
    let actor_name = actor_dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let site = site_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("www.javbus.com");
    let site_host = normalize_site_host(site);
    let session = {
        let db = state
            .db
            .lock()
            .map_err(|_| "Database lock is poisoned".to_string())?;
        load_metadata_session(&db, &site_host).map_err(|error| error.to_string())?
    };
    let source_url = build_actor_search_url(site, &actor_name);
    let html = http_get_text_with_session(&source_url, session.as_ref())
        .map_err(|error| error.to_string())?;
    if is_age_verification_page(&html) {
        let db = state
            .db
            .lock()
            .map_err(|_| "Database lock is poisoned".to_string())?;
        touch_metadata_session_status(
            &db,
            &site_host,
            "blocked",
            "JavBus 返回年龄验证页，请先在设置中导入浏览器会话，或打开验证页完成验证后再导入",
        )
        .map_err(|error| error.to_string())?;
        return Err(
            "JavBus 返回年龄验证页，请先在设置中导入浏览器会话，或打开验证页完成验证后再导入"
                .to_string(),
        );
    }
    let avatar_url = extract_searchstar_avatar_url(&html, &source_url)
        .or_else(|| extract_actor_avatar_url(&html, &source_url))
        .or_else(|| extract_first_image_url(&html, &source_url))
        .ok_or_else(|| "没有解析到头像图片".to_string())?;
    let avatar_path = download_avatar(
        &avatar_url,
        &state.data_dir,
        &actor_path,
        session.as_ref(),
        Some(&source_url),
    )
    .map_err(|error| error.to_string())?;
    let profile = ActorProfile {
        actor_name,
        actor_path,
        avatar_path: Some(avatar_path),
        avatar_source: Some(avatar_url),
        updated_at: now_string(),
    };
    let db = state
        .db
        .lock()
        .map_err(|_| "Database lock is poisoned".to_string())?;
    persist_actor_profile(&db, &profile).map_err(|error| error.to_string())?;
    Ok(profile)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.handle().path().app_data_dir()?;
            let cover_dir = app.handle().path().app_cache_dir()?.join(COVER_FOLDER);
            if let Ok(resource_dir) = app.handle().path().resource_dir() {
                let _ = APP_RESOURCE_DIR.set(resource_dir);
            }
            fs::create_dir_all(&data_dir)?;
            fs::create_dir_all(&cover_dir)?;
            let db = open_database(&data_dir)?;
            migrate_database(&db)?;
            restore_window_size(&app.handle().clone(), &data_dir);
            if let Some(icon) = app.default_window_icon().cloned() {
                for window in app.webview_windows().values() {
                    let _ = window.set_icon(icon.clone());
                }
            }
            app.manage(AppState {
                db: Mutex::new(db),
                data_dir,
                cover_dir,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan_library,
            ensure_video_cover,
            load_latest_report,
            load_cached_library,
            load_operation_logs,
            preview_download_move_plan,
            open_media_file,
            open_file_location,
            open_external_url,
            execute_rename_plan,
            execute_review_move,
            execute_download_move_plan,
            get_archive_lookup,
            search_archive_metadata,
            preview_archive_move,
            execute_archive_move,
            execute_archive_move_to_new_actor,
            delete_archive_video,
            load_actor_profiles,
            load_metadata_session_status,
            import_browser_metadata_session,
            clear_metadata_session,
            save_manual_metadata_session,
            refresh_actor_avatar
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Kawa Library");
}

fn open_database(data_dir: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    let db_path = data_dir.join("kawa-library.sqlite");
    let db = Connection::open(db_path)?;
    Ok(db)
}

fn migrate_database(db: &Connection) -> Result<()> {
    db.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS reports (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            root_path TEXT NOT NULL,
            scanned_at TEXT NOT NULL,
            payload TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS operation_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            action TEXT NOT NULL,
            source_path TEXT NOT NULL,
            target_path TEXT NOT NULL,
            status TEXT NOT NULL,
            message TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS archive_lookups (
            code TEXT PRIMARY KEY,
            status TEXT NOT NULL,
            actor_name TEXT,
            actor_path TEXT,
            avatar_path TEXT,
            source_url TEXT,
            message TEXT NOT NULL,
            searched_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS actor_profiles (
            actor_path TEXT PRIMARY KEY,
            actor_name TEXT NOT NULL,
            avatar_path TEXT,
            avatar_source TEXT,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS metadata_sessions (
            site_host TEXT PRIMARY KEY,
            cookie_header TEXT NOT NULL,
            source TEXT NOT NULL,
            status TEXT NOT NULL,
            message TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        "#,
    )?;
    Ok(())
}

fn persist_report(db: &Connection, report: &ScanReport) -> Result<()> {
    let payload = serde_json::to_string(report)?;
    db.execute(
        r#"
        INSERT INTO reports (id, root_path, scanned_at, payload)
        VALUES (1, ?1, ?2, ?3)
        ON CONFLICT(id) DO UPDATE SET
            root_path = excluded.root_path,
            scanned_at = excluded.scanned_at,
            payload = excluded.payload
        "#,
        params![report.stats.root_path, report.stats.scanned_at, payload],
    )?;
    Ok(())
}

fn load_report(db: &Connection) -> Result<Option<ScanReport>> {
    let mut stmt = db.prepare("SELECT payload FROM reports WHERE id = 1")?;
    let mut rows = stmt.query([])?;

    if let Some(row) = rows.next()? {
        let payload: String = row.get(0)?;
        let report = serde_json::from_str(&payload)?;
        Ok(Some(report))
    } else {
        Ok(None)
    }
}

fn persist_logs(db: &Connection, logs: &mut [OperationLog]) -> Result<()> {
    for log in logs.iter_mut() {
        db.execute(
            r#"
            INSERT INTO operation_logs (timestamp, action, source_path, target_path, status, message)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                log.timestamp,
                log.action,
                log.source_path,
                log.target_path,
                log.status,
                log.message
            ],
        )?;
        log.id = db.last_insert_rowid();
    }
    Ok(())
}

fn load_logs(db: &Connection, limit: usize) -> Result<Vec<OperationLog>> {
    let mut stmt = db.prepare(
        r#"
        SELECT id, timestamp, action, source_path, target_path, status, message
        FROM operation_logs
        ORDER BY id DESC
        LIMIT ?1
        "#,
    )?;

    let rows = stmt.query_map([limit as i64], |row| {
        Ok(OperationLog {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            action: row.get(2)?,
            source_path: row.get(3)?,
            target_path: row.get(4)?,
            status: row.get(5)?,
            message: row.get(6)?,
        })
    })?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to load operation logs")
}

fn load_archive_lookup(db: &Connection, code: &str) -> Result<Option<ArchiveLookup>> {
    let mut stmt = db.prepare(
        r#"
        SELECT code, status, actor_name, actor_path, avatar_path, source_url, message, searched_at
        FROM archive_lookups
        WHERE code = ?1
        "#,
    )?;
    let mut rows = stmt.query([code])?;
    if let Some(row) = rows.next()? {
        Ok(Some(ArchiveLookup {
            code: row.get(0)?,
            status: row.get(1)?,
            actor_name: row.get(2)?,
            actor_path: row.get(3)?,
            avatar_path: row.get(4)?,
            source_url: row.get(5)?,
            message: row.get(6)?,
            searched_at: row.get(7)?,
        }))
    } else {
        Ok(None)
    }
}

fn persist_archive_lookup(db: &Connection, lookup: &ArchiveLookup) -> Result<()> {
    db.execute(
        r#"
        INSERT INTO archive_lookups
            (code, status, actor_name, actor_path, avatar_path, source_url, message, searched_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(code) DO UPDATE SET
            status = excluded.status,
            actor_name = excluded.actor_name,
            actor_path = excluded.actor_path,
            avatar_path = excluded.avatar_path,
            source_url = excluded.source_url,
            message = excluded.message,
            searched_at = excluded.searched_at
        "#,
        params![
            lookup.code,
            lookup.status,
            lookup.actor_name,
            lookup.actor_path,
            lookup.avatar_path,
            lookup.source_url,
            lookup.message,
            lookup.searched_at
        ],
    )?;
    Ok(())
}

fn load_profiles(db: &Connection) -> Result<Vec<ActorProfile>> {
    let mut stmt = db.prepare(
        r#"
        SELECT actor_name, actor_path, avatar_path, avatar_source, updated_at
        FROM actor_profiles
        ORDER BY actor_name
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ActorProfile {
            actor_name: row.get(0)?,
            actor_path: row.get(1)?,
            avatar_path: row.get(2)?,
            avatar_source: row.get(3)?,
            updated_at: row.get(4)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to load actor profiles")
}

fn persist_actor_profile(db: &Connection, profile: &ActorProfile) -> Result<()> {
    db.execute(
        r#"
        INSERT INTO actor_profiles
            (actor_name, actor_path, avatar_path, avatar_source, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(actor_path) DO UPDATE SET
            actor_name = excluded.actor_name,
            avatar_path = COALESCE(excluded.avatar_path, actor_profiles.avatar_path),
            avatar_source = COALESCE(excluded.avatar_source, actor_profiles.avatar_source),
            updated_at = excluded.updated_at
        "#,
        params![
            profile.actor_name,
            profile.actor_path,
            profile.avatar_path,
            profile.avatar_source,
            profile.updated_at
        ],
    )?;
    Ok(())
}

fn load_metadata_session(db: &Connection, site_host: &str) -> Result<Option<MetadataSession>> {
    let mut stmt = db.prepare(
        r#"
        SELECT cookie_header, source, updated_at
        FROM metadata_sessions
        WHERE site_host = ?1
        "#,
    )?;
    let mut rows = stmt.query([site_host])?;
    if let Some(row) = rows.next()? {
        Ok(Some(MetadataSession {
            cookie_header: row.get(0)?,
            source: row.get(1)?,
            updated_at: row.get(2)?,
        }))
    } else {
        Ok(None)
    }
}

fn load_metadata_session_status_row(
    db: &Connection,
    site_host: &str,
) -> Result<Option<(String, String, String, String)>> {
    let mut stmt = db.prepare(
        r#"
        SELECT source, status, message, updated_at
        FROM metadata_sessions
        WHERE site_host = ?1
        "#,
    )?;
    let mut rows = stmt.query([site_host])?;
    if let Some(row) = rows.next()? {
        Ok(Some((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
    } else {
        Ok(None)
    }
}

fn metadata_session_status_from_row(
    site_host: &str,
    row: Option<(String, String, String, String)>,
) -> MetadataSessionStatus {
    if let Some((source, status, message, updated_at)) = row {
        MetadataSessionStatus {
            site_host: site_host.to_string(),
            has_cookie: true,
            source: Some(source),
            status,
            updated_at: Some(updated_at),
            message,
        }
    } else {
        MetadataSessionStatus {
            site_host: site_host.to_string(),
            has_cookie: false,
            source: None,
            status: "missing".to_string(),
            updated_at: None,
            message: "未导入浏览器会话".to_string(),
        }
    }
}

fn persist_metadata_session(
    db: &Connection,
    site_host: &str,
    cookie_header: &str,
    source: &str,
    status: &str,
    message: &str,
    updated_at: &str,
) -> Result<()> {
    db.execute(
        r#"
        INSERT INTO metadata_sessions
            (site_host, cookie_header, source, status, message, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(site_host) DO UPDATE SET
            cookie_header = excluded.cookie_header,
            source = excluded.source,
            status = excluded.status,
            message = excluded.message,
            updated_at = excluded.updated_at
        "#,
        params![
            site_host,
            cookie_header,
            source,
            status,
            message,
            updated_at
        ],
    )?;
    Ok(())
}

fn touch_metadata_session_status(
    db: &Connection,
    site_host: &str,
    status: &str,
    message: &str,
) -> Result<()> {
    db.execute(
        r#"
        UPDATE metadata_sessions
        SET status = ?2, message = ?3, updated_at = ?4
        WHERE site_host = ?1
        "#,
        params![site_host, status, message, now_string()],
    )?;
    Ok(())
}

fn delete_metadata_session(db: &Connection, site_host: &str) -> Result<()> {
    db.execute(
        "DELETE FROM metadata_sessions WHERE site_host = ?1",
        [site_host],
    )?;
    Ok(())
}

fn scan_root(
    root: &Path,
    download_path: Option<&str>,
    appreciation_path: Option<&str>,
    processing_options: &ProcessingOptions,
    cover_percent: f64,
    cover_dir: &Path,
) -> Result<ScanReport> {
    if !root.exists() {
        anyhow::bail!("目录不存在：{}", root.display());
    }
    if !root.is_dir() {
        anyhow::bail!("这不是文件夹：{}", root.display());
    }

    let root = root
        .canonicalize()
        .with_context(|| format!("无法解析目录：{}", root.display()))?;
    let code_regexes = build_code_regexes()?;
    let number_regex = Regex::new(r"\d{3,}")?;
    let mut files = Vec::new();
    let mut folder_map: HashMap<String, FolderSummaryBuilder> = HashMap::new();
    let direct_folders = read_direct_folders(&root)?;
    let mut folder_count = 0usize;
    let mut total_size: i64 = 0;
    let mut next_id = 1i64;

    for entry in WalkDir::new(&root).follow_links(false) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();

        if path == root {
            continue;
        }

        if entry.file_type().is_dir() {
            folder_count += 1;
            continue;
        }

        if !entry.file_type().is_file() {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        let size_bytes = metadata.len().min(i64::MAX as u64) as i64;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();

        let parent_path = path.parent().unwrap_or(&root);
        let parent = parent_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("sakana")
            .to_string();

        let depth = path
            .strip_prefix(&root)
            .ok()
            .map(|relative| relative.components().count().saturating_sub(1))
            .unwrap_or(0);

        let is_root_file = parent_path == root;
        let modified_time = metadata.modified().ok();
        let modified_unix = modified_time.map(system_time_to_unix).unwrap_or_default();
        let modified = modified_time
            .map(format_system_time)
            .unwrap_or_else(|| "未知".to_string());

        if !is_root_file {
            let key = top_level_folder_key(&root, path);
            let builder = folder_map
                .entry(key.0.clone())
                .or_insert_with(|| FolderSummaryBuilder {
                    name: key.0,
                    path: key.1,
                    file_count: 0,
                    size_bytes: 0,
                    modified: modified.clone(),
                    modified_unix,
                });
            builder.file_count += 1;
            builder.size_bytes += size_bytes;
            if modified_unix > builder.modified_unix {
                builder.modified = modified.clone();
                builder.modified_unix = modified_unix;
            }
        }

        total_size += size_bytes;
        let is_video = is_video_extension(&extension);
        let media_code = if is_video {
            extract_media_code(&name, &code_regexes)
        } else {
            None
        };
        let cover_path = if is_video {
            Some(
                cover_path_for_video(path, &metadata, cover_dir, cover_percent)
                    .to_string_lossy()
                    .to_string(),
            )
        } else {
            None
        };

        files.push(LibraryFile {
            id: next_id,
            name,
            path: path.to_string_lossy().to_string(),
            parent,
            extension,
            size_bytes,
            size_gb: bytes_to_gb(size_bytes),
            modified,
            modified_unix,
            depth,
            media_code,
            cover_path,
            is_root_file,
        });
        next_id += 1;
    }

    let root_files = files
        .iter()
        .filter(|file| file.is_root_file && file.name != "查重.txt")
        .cloned()
        .collect::<Vec<_>>();

    let name_duplicates = duplicate_groups(
        &files,
        |file| Some(file.name.to_ascii_uppercase()),
        |file| is_video_extension(&file.extension),
    );

    let code_duplicates = duplicate_groups(
        &files,
        |file| file.media_code.clone(),
        |file| file.media_code.is_some(),
    );

    let numeric_duplicates = numeric_duplicate_groups(&files, &number_regex);
    let move_suggestions = build_move_suggestions(&root_files, &files);
    let largest_files = largest_direct_files(&files, 10);
    let rename_previews = build_rename_previews(&root, &files);
    let download_dir = resolve_configured_dir(&root, download_path, DOWNLOAD_FOLDER)?;
    let appreciation_dir = resolve_configured_dir(&root, appreciation_path, APPRECIATION_FOLDER)?;
    let excluded_actor_folders =
        configured_top_level_folders(&root, &[download_dir.as_path(), appreciation_dir.as_path()]);
    let download_move_plans =
        build_download_move_plans(&download_dir, &appreciation_dir, processing_options);
    let actors = build_actor_summaries(&root, &files, &direct_folders, &excluded_actor_folders);
    let actor_snapshots = snapshot_actor_folders(&direct_folders, &excluded_actor_folders);
    let mut appreciation_videos = files
        .iter()
        .filter(|file| {
            is_video_extension(&file.extension)
                && PathBuf::from(&file.path).starts_with(&appreciation_dir)
        })
        .cloned()
        .collect::<Vec<_>>();
    appreciation_videos.sort_by(|a, b| b.modified_unix.cmp(&a.modified_unix));
    let appreciation_snapshot_unix = directory_modified_unix(&appreciation_dir);
    let stale_folders = build_stale_folders(&files, &direct_folders);

    let mut largest_folders = folder_map
        .into_values()
        .map(FolderSummaryBuilder::finish)
        .collect::<Vec<_>>();
    largest_folders.sort_by(|a, b| {
        b.size_gb
            .partial_cmp(&a.size_gb)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    largest_folders.truncate(20);

    let stats = LibraryStats {
        root_path: root.to_string_lossy().to_string(),
        file_count: files.len(),
        folder_count,
        root_file_count: root_files.len(),
        video_count: files
            .iter()
            .filter(|file| is_video_extension(&file.extension))
            .count(),
        total_size_gb: bytes_to_gb(total_size),
        scanned_at: Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    };

    Ok(ScanReport {
        stats,
        actors,
        actor_snapshots,
        appreciation_path: appreciation_dir.to_string_lossy().to_string(),
        appreciation_videos,
        appreciation_snapshot_unix,
        root_files,
        name_duplicates,
        code_duplicates,
        numeric_duplicates,
        move_suggestions,
        largest_folders,
        largest_files,
        rename_previews,
        download_move_plans,
        stale_folders,
    })
}

fn refresh_cached_report(
    report: &mut ScanReport,
    root: &Path,
    download_path: Option<&str>,
    appreciation_path: Option<&str>,
    processing_options: &ProcessingOptions,
    cover_percent: f64,
    cover_dir: &Path,
) -> Result<bool> {
    let download_dir = resolve_configured_dir(root, download_path, DOWNLOAD_FOLDER)?;
    let appreciation_dir = resolve_configured_dir(root, appreciation_path, APPRECIATION_FOLDER)?;
    let direct_folders = read_direct_folders(root)?;
    let excluded_actor_folders =
        configured_top_level_folders(root, &[download_dir.as_path(), appreciation_dir.as_path()]);
    let mut actor_by_path = report
        .actors
        .iter()
        .map(|actor| (actor.path.clone(), actor.clone()))
        .collect::<HashMap<_, _>>();
    let snapshot_by_path = report
        .actor_snapshots
        .iter()
        .map(|snapshot| (snapshot.path.clone(), snapshot.modified_unix))
        .collect::<HashMap<_, _>>();
    let cached_scan_unix = parse_report_scanned_at(&report.stats.scanned_at);
    let candidate_actor_paths = direct_folders
        .iter()
        .filter(|folder| !is_system_folder(&folder.name))
        .filter(|folder| !excluded_actor_folders.contains(&folder.name))
        .map(|folder| folder.path.clone())
        .collect::<HashSet<_>>();
    let mut changed = false;

    for folder in direct_folders
        .iter()
        .filter(|folder| !is_system_folder(&folder.name))
        .filter(|folder| !excluded_actor_folders.contains(&folder.name))
    {
        let folder_modified = folder.modified_unix;
        let cached_actor = actor_by_path.get(&folder.path);
        let saved_snapshot = snapshot_by_path.get(&folder.path).copied();
        let snapshot_is_legacy = saved_snapshot
            .zip(cached_actor)
            .map(|(snapshot, actor)| snapshot == actor.modified_unix && snapshot != folder_modified)
            .unwrap_or(false);
        let unchanged_since_cache = cached_scan_unix
            .map(|timestamp| folder_modified <= timestamp)
            .unwrap_or(false);
        let can_trust_cached_actor = cached_actor.is_some()
            && unchanged_since_cache
            && (saved_snapshot.is_none() || snapshot_is_legacy);
        let needs_refresh = if can_trust_cached_actor {
            if saved_snapshot != Some(folder_modified) {
                changed = true;
            }
            false
        } else {
            saved_snapshot
                .map(|value| value != folder_modified)
                .unwrap_or(true)
        };
        if needs_refresh {
            if let Some(actor) =
                scan_actor_folder(root, folder, folder_modified, cover_dir, cover_percent)?
            {
                actor_by_path.insert(folder.path.clone(), actor);
            } else {
                actor_by_path.remove(&folder.path);
            }
            changed = true;
        }
    }

    if report
        .actor_snapshots
        .iter()
        .any(|snapshot| !candidate_actor_paths.contains(&snapshot.path))
    {
        changed = true;
    }

    let mut actors = actor_by_path
        .into_values()
        .filter(|actor| candidate_actor_paths.contains(&actor.path))
        .collect::<Vec<_>>();
    actors.sort_by(|a, b| {
        b.modified_unix
            .cmp(&a.modified_unix)
            .then_with(|| b.videos.len().cmp(&a.videos.len()))
            .then_with(|| a.name.cmp(&b.name))
    });

    report.actors = actors;
    report.actor_snapshots = snapshot_actor_folders(&direct_folders, &excluded_actor_folders);

    let current_appreciation_snapshot = directory_modified_unix(&appreciation_dir);
    let appreciation_path = appreciation_dir.to_string_lossy().to_string();
    let should_refresh_appreciation = report.appreciation_path.is_empty()
        || !paths_same_ignore_case(Path::new(&report.appreciation_path), &appreciation_dir)
        || report.appreciation_snapshot_unix != current_appreciation_snapshot;
    if should_refresh_appreciation {
        report.appreciation_videos =
            scan_video_files_in_dir(&appreciation_dir, cover_dir, cover_percent)?;
        report
            .appreciation_videos
            .sort_by(|a, b| b.modified_unix.cmp(&a.modified_unix));
        report.appreciation_snapshot_unix = current_appreciation_snapshot;
        report.appreciation_path = appreciation_path;
        changed = true;
    }
    report.download_move_plans =
        build_download_move_plans(&download_dir, &appreciation_dir, processing_options);

    let actor_video_count = report
        .actors
        .iter()
        .map(|actor| actor.videos.len())
        .sum::<usize>();
    report.stats.video_count = actor_video_count + report.appreciation_videos.len();
    report.stats.folder_count = direct_folders.len();
    report.stats.file_count = report
        .actors
        .iter()
        .map(|actor| actor.file_count)
        .sum::<usize>()
        + report.appreciation_videos.len();
    report.stats.total_size_gb = report
        .actors
        .iter()
        .map(|actor| actor.total_size_gb)
        .sum::<f64>()
        + report
            .appreciation_videos
            .iter()
            .map(|file| file.size_gb)
            .sum::<f64>();
    if changed {
        report.stats.scanned_at = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    }

    Ok(changed)
}

fn duplicate_groups<K, F, P>(files: &[LibraryFile], key_fn: F, predicate: P) -> Vec<DuplicateGroup>
where
    K: Into<String>,
    F: Fn(&LibraryFile) -> Option<K>,
    P: Fn(&LibraryFile) -> bool,
{
    let mut grouped: BTreeMap<String, Vec<LibraryFile>> = BTreeMap::new();

    for file in files.iter().filter(|file| predicate(file)) {
        if let Some(key) = key_fn(file) {
            grouped.entry(key.into()).or_default().push(file.clone());
        }
    }

    let mut groups = grouped
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(key, mut files)| {
            files.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
            let total_size = files.iter().map(|file| file.size_bytes).sum::<i64>();
            DuplicateGroup {
                key,
                count: files.len(),
                total_size_gb: bytes_to_gb(total_size),
                files,
            }
        })
        .collect::<Vec<_>>();

    groups.sort_by(|a, b| {
        b.count.cmp(&a.count).then_with(|| {
            b.total_size_gb
                .partial_cmp(&a.total_size_gb)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    groups.truncate(100);
    groups
}

fn numeric_duplicate_groups(
    files: &[LibraryFile],
    number_regex: &Regex,
) -> Vec<NumericDuplicateGroup> {
    let mut grouped: BTreeMap<(String, String), Vec<LibraryFile>> = BTreeMap::new();

    for file in files
        .iter()
        .filter(|file| file.depth == 1 && is_video_extension(&file.extension))
    {
        for hit in number_regex.find_iter(&file.name) {
            grouped
                .entry((file.parent.clone(), hit.as_str().to_string()))
                .or_default()
                .push(file.clone());
        }
    }

    let mut groups = grouped
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|((folder, number), mut files)| {
            files.sort_by(|a, b| a.name.cmp(&b.name));
            NumericDuplicateGroup {
                folder,
                number,
                count: files.len(),
                files,
            }
        })
        .collect::<Vec<_>>();

    groups.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.folder.cmp(&b.folder)));
    groups.truncate(150);
    groups
}

fn build_move_suggestions(
    root_files: &[LibraryFile],
    all_files: &[LibraryFile],
) -> Vec<MoveSuggestion> {
    let mut by_code: HashMap<String, HashMap<String, usize>> = HashMap::new();

    for file in all_files.iter().filter(|file| !file.is_root_file) {
        if let Some(code) = &file.media_code {
            *by_code
                .entry(code.clone())
                .or_default()
                .entry(file.parent.clone())
                .or_default() += 1;
        }
    }

    let mut suggestions = Vec::new();
    for file in root_files {
        let Some(code) = &file.media_code else {
            suggestions.push(MoveSuggestion {
                file: file.clone(),
                target_folder: "_review".to_string(),
                reason: "没有识别到稳定番号，建议人工确认".to_string(),
                confidence: "low".to_string(),
            });
            continue;
        };

        if let Some(folder_counts) = by_code.get(code) {
            if let Some((folder, _)) = folder_counts.iter().max_by_key(|(_, count)| *count) {
                suggestions.push(MoveSuggestion {
                    file: file.clone(),
                    target_folder: folder.clone(),
                    reason: format!("库内已存在同番号 {}", code),
                    confidence: "high".to_string(),
                });
                continue;
            }
        }

        suggestions.push(MoveSuggestion {
            file: file.clone(),
            target_folder: "_review".to_string(),
            reason: format!("识别到番号 {}，但没有匹配到已有目录", code),
            confidence: "medium".to_string(),
        });
    }

    suggestions.sort_by(|a, b| {
        confidence_rank(&b.confidence)
            .cmp(&confidence_rank(&a.confidence))
            .then_with(|| a.file.name.cmp(&b.file.name))
    });
    suggestions
}

fn largest_direct_files(files: &[LibraryFile], limit: usize) -> Vec<LargestFile> {
    let mut direct_files = files
        .iter()
        .filter(|file| file.depth == 1 && is_video_extension(&file.extension))
        .cloned()
        .collect::<Vec<_>>();
    direct_files.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

    direct_files
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(index, file)| LargestFile {
            rank: index + 1,
            file,
        })
        .collect()
}

fn build_rename_previews(root: &Path, files: &[LibraryFile]) -> Vec<RenamePreview> {
    let mut previews = files
        .iter()
        .filter(|file| file.depth <= 1 && file.name != "查重.txt")
        .filter_map(|file| rename_preview_for_file(root, file))
        .collect::<Vec<_>>();

    previews.sort_by(|a, b| {
        status_rank(&b.status)
            .cmp(&status_rank(&a.status))
            .then_with(|| a.current_name.cmp(&b.current_name))
    });
    previews.truncate(300);
    previews
}

fn rename_preview_for_file(_root: &Path, file: &LibraryFile) -> Option<RenamePreview> {
    let source = PathBuf::from(&file.path);
    let ext = source
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value))
        .unwrap_or_default();
    let original_base = source.file_stem()?.to_str()?.to_string();
    let mut base = original_base.clone();
    let mut rules = Vec::new();

    if let Some(index) = base.rfind('@') {
        base = base[index + 1..].to_string();
        rules.push("去 @ 前缀".to_string());
    }

    let with_dash = normalize_dash(&base);
    if with_dash != base {
        base = with_dash;
        rules.push("补横杠".to_string());
    }

    let upper = base.to_uppercase();
    if upper != base {
        base = upper;
        rules.push("名称大写".to_string());
    }

    if rules.is_empty() {
        return None;
    }

    let new_name = format!("{}{}", base, ext);
    if new_name == file.name {
        return None;
    }

    let target = source.with_file_name(&new_name);
    let target_exists = target.exists() && !paths_same_ignore_case(&source, &target);
    let status = if target_exists { "conflict" } else { "ready" }.to_string();
    let reason = if target_exists {
        "目标文件已存在，默认跳过".to_string()
    } else {
        "可安全重命名".to_string()
    };

    Some(RenamePreview {
        id: file.path.clone(),
        path: file.path.clone(),
        directory: source
            .parent()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default(),
        current_name: file.name.clone(),
        new_name,
        target_path: target.to_string_lossy().to_string(),
        rules,
        status,
        reason,
    })
}

fn normalize_dash(base: &str) -> String {
    let fc2 = Regex::new(r"(?i)^FC2[-_ ]?PPV[-_ ]?(\d{6,8})(.*)$").unwrap();
    if let Some(captures) = fc2.captures(base) {
        return format!("FC2PPV-{}{}", &captures[1], &captures[2]);
    }

    let leading_digits = Regex::new(r"(?i)^([0-9]{3}[A-Z]+)[-_ ]?(\d{3,5})(.*)$").unwrap();
    if let Some(captures) = leading_digits.captures(base) {
        return format!(
            "{}-{}{}",
            &captures[1],
            &captures[2],
            normalize_suffix(&captures[3])
        );
    }

    let letters = Regex::new(r"(?i)^([A-Z]{2,6})[-_ ]?(\d{3,5})(.*)$").unwrap();
    if let Some(captures) = letters.captures(base) {
        return format!(
            "{}-{}{}",
            &captures[1],
            &captures[2],
            normalize_suffix(&captures[3])
        );
    }

    let digit_letter = Regex::new(r"(\d{3})([A-Za-z])").unwrap();
    digit_letter.replace(base, "$1-$2").to_string()
}

fn normalize_suffix(suffix: &str) -> String {
    if suffix.is_empty() {
        return String::new();
    }
    let first = suffix.chars().next().unwrap_or_default();
    if first.is_ascii_alphanumeric() {
        format!("-{}", suffix)
    } else {
        suffix.to_string()
    }
}

fn normalize_uncensored_suffix(base: &str) -> String {
    let marker = Regex::new(r"(?i)(UNCENSORED|RESTORED)").unwrap();
    if !marker.is_match(base) {
        return base.to_string();
    }

    let normalized = normalize_dash(base);
    let code = Regex::new(r"(?i)^((?:FC2PPV|[0-9]{3}[A-Z]+|[A-Z]{2,8})-\d{2,8})").unwrap();
    let Some(captures) = code.captures(&normalized) else {
        return marker
            .replace_all(&normalized, "")
            .trim_matches(['-', '_', ' ', '.'])
            .to_string();
    };

    let code_text = captures[1].to_uppercase();
    let rest = normalized[captures.get(1).map(|item| item.end()).unwrap_or(0)..].to_uppercase();
    let censored_suffix = Regex::new(r"^[-_ ]C(?:[-_ .]|$)").unwrap();
    if censored_suffix.is_match(&rest) {
        format!("{}-UC", code_text)
    } else {
        format!("{}-U", code_text)
    }
}

fn build_download_move_plans(
    download_dir: &Path,
    appreciation_dir: &Path,
    options: &ProcessingOptions,
) -> Vec<DownloadMovePlan> {
    if !download_dir.is_dir() {
        return Vec::new();
    }

    let mut plans = Vec::new();
    let entries = match fs::read_dir(download_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_file() {
            if let Some(plan) = build_download_plan_for_file(
                download_dir,
                appreciation_dir,
                &path,
                "file",
                false,
                1,
                options,
            ) {
                plans.push(plan);
            }
            continue;
        }

        if !path.is_dir() {
            continue;
        }

        let (has_incomplete, video_files) = scan_download_folder(&path);
        let video_count = video_files.len();
        if video_count == 0 {
            plans.push(build_skipped_download_folder_plan(
                &path,
                has_incomplete,
                "folder",
            ));
            continue;
        }
        for file_path in video_files {
            if let Some(plan) = build_download_plan_for_file(
                download_dir,
                appreciation_dir,
                &file_path,
                "folder",
                has_incomplete,
                video_count,
                options,
            ) {
                plans.push(plan);
            }
        }
    }

    plans.sort_by(|a, b| {
        status_rank(&b.status)
            .cmp(&status_rank(&a.status))
            .then_with(|| b.source_name.cmp(&a.source_name))
    });
    plans
}

fn build_skipped_download_folder_plan(
    folder: &Path,
    has_incomplete: bool,
    item_type: &str,
) -> DownloadMovePlan {
    let folder_name = folder
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let folder_path = folder.to_string_lossy().to_string();
    DownloadMovePlan {
        id: folder_path.clone(),
        source_path: folder_path.clone(),
        source_name: folder_name.clone(),
        item_type: item_type.to_string(),
        folder: folder_name,
        folder_path,
        target_file_name: String::new(),
        target_path: String::new(),
        target_relative: "跳过".to_string(),
        file_count: 0,
        has_xltd: has_incomplete,
        status: "skipped".to_string(),
        reason: if has_incomplete {
            "检测到未完成下载文件".to_string()
        } else {
            "未找到完成的视频文件".to_string()
        },
        files: vec![],
    }
}

fn scan_download_folder(folder: &Path) -> (bool, Vec<PathBuf>) {
    let mut has_incomplete = false;
    let mut video_files = Vec::new();
    for entry in WalkDir::new(folder).follow_links(false) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if is_incomplete_extension(&extension) {
            has_incomplete = true;
        }
        if is_video_extension(&extension) {
            video_files.push(path.to_path_buf());
        }
    }
    (has_incomplete, video_files)
}

#[allow(clippy::too_many_arguments)]
fn build_download_plan_for_file(
    download_dir: &Path,
    appreciation_dir: &Path,
    source_file: &Path,
    item_type: &str,
    has_incomplete: bool,
    file_count: usize,
    options: &ProcessingOptions,
) -> Option<DownloadMovePlan> {
    let extension = source_file
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !is_video_extension(&extension) {
        return None;
    }

    let source_name = source_file
        .strip_prefix(download_dir)
        .unwrap_or(source_file)
        .to_string_lossy()
        .to_string();
    let target_file_name = build_target_file_name(
        source_file
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default(),
        options,
    );
    let target_path = appreciation_dir.join(&target_file_name);
    let target_relative = appreciation_dir
        .file_name()
        .and_then(|value| value.to_str())
        .map(|folder| format!(r"{}\{}", folder, target_file_name))
        .unwrap_or_else(|| target_file_name.clone());
    let source_path = source_file.to_string_lossy().to_string();
    let folder_name = source_file
        .parent()
        .and_then(|value| value.file_name())
        .and_then(|value| value.to_str())
        .unwrap_or_else(|| {
            download_dir
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(DOWNLOAD_FOLDER)
        })
        .to_string();
    let folder_path = source_file
        .parent()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| download_dir.to_string_lossy().to_string());

    let (status, reason) = if !options.move_to_appreciation {
        ("skipped", "已关闭“移动到欣赏区”")
    } else if is_incomplete_extension(&extension) || has_incomplete {
        ("skipped", "检测到未完成下载文件")
    } else if target_path.exists() && options.skip_if_exists {
        ("skipped", "欣赏区目标已存在")
    } else if target_path.exists() {
        ("conflict", "欣赏区目标已存在，不会覆盖")
    } else {
        ("ready", "可执行")
    };

    Some(DownloadMovePlan {
        id: source_path.clone(),
        source_path,
        source_name,
        item_type: item_type.to_string(),
        folder: folder_name,
        folder_path,
        target_file_name,
        target_path: target_path.to_string_lossy().to_string(),
        target_relative,
        file_count,
        has_xltd: has_incomplete,
        status: status.to_string(),
        reason: reason.to_string(),
        files: vec![],
    })
}

fn build_target_file_name(source_name: &str, options: &ProcessingOptions) -> String {
    if !options.rename_enabled {
        return source_name.to_string();
    }

    let source = PathBuf::from(source_name);
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value))
        .unwrap_or_default();
    let mut base = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(source_name)
        .to_string();

    if options.remove_at_prefix {
        if let Some(index) = base.rfind('@') {
            base = base[index + 1..].to_string();
        }
    }

    if options.normalize_dash {
        base = normalize_dash(&base);
    }

    if options.uppercase {
        base = base.to_uppercase();
    }

    if options.normalize_uncensored_suffix {
        base = normalize_uncensored_suffix(&base);
    }

    format!("{}{}", base, extension)
}

fn scan_actor_folder(
    root: &Path,
    folder: &DirectFolder,
    modified_unix: i64,
    cover_dir: &Path,
    cover_percent: f64,
) -> Result<Option<ActorSummary>> {
    let folder_path = PathBuf::from(&folder.path);
    if !folder_path.is_dir() {
        return Ok(None);
    }

    let mut files = scan_files_under_root(root, &folder_path, cover_dir, cover_percent)?;
    if files.is_empty() {
        return Ok(None);
    }

    files.sort_by(|a, b| b.modified_unix.cmp(&a.modified_unix));
    let total_bytes = files.iter().map(|file| file.size_bytes).sum::<i64>();
    let modified = files
        .first()
        .map(|file| file.modified.clone())
        .unwrap_or_else(|| "未知".to_string());
    let effective_modified_unix = files
        .first()
        .map(|file| file.modified_unix)
        .unwrap_or(modified_unix);
    let videos = files
        .iter()
        .filter(|file| is_video_extension(&file.extension))
        .map(|file| ActorVideo {
            path: file.path.clone(),
            name: file.name.clone(),
            size_gb: file.size_gb,
            modified: file.modified.clone(),
            modified_unix: file.modified_unix,
            media_code: file.media_code.clone(),
            cover_path: file.cover_path.clone(),
        })
        .collect::<Vec<_>>();

    if videos.is_empty() {
        return Ok(None);
    }

    Ok(Some(ActorSummary {
        name: folder.name.clone(),
        path: folder.path.clone(),
        file_count: files.len(),
        total_size_gb: bytes_to_gb(total_bytes),
        modified,
        modified_unix: effective_modified_unix,
        roles: actor_roles(
            videos.len(),
            bytes_to_gb(total_bytes),
            effective_modified_unix,
        ),
        videos,
    }))
}

fn scan_video_files_in_dir(
    dir: &Path,
    cover_dir: &Path,
    cover_percent: f64,
) -> Result<Vec<LibraryFile>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let root = dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dir.to_path_buf());
    let files = scan_files_under_root(&root, dir, cover_dir, cover_percent)?;
    Ok(files
        .into_iter()
        .filter(|file| is_video_extension(&file.extension))
        .collect())
}

fn scan_files_under_root(
    root: &Path,
    folder: &Path,
    cover_dir: &Path,
    cover_percent: f64,
) -> Result<Vec<LibraryFile>> {
    let code_regexes = build_code_regexes()?;
    let mut files = Vec::new();
    let mut next_id = 1i64;

    for entry in WalkDir::new(folder).follow_links(false) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        files.push(library_file_from_path(
            root,
            path,
            &metadata,
            next_id,
            &code_regexes,
            cover_dir,
            cover_percent,
        ));
        next_id += 1;
    }

    Ok(files)
}

fn library_file_from_path(
    root: &Path,
    path: &Path,
    metadata: &fs::Metadata,
    id: i64,
    code_regexes: &CodeRegexes,
    cover_dir: &Path,
    cover_percent: f64,
) -> LibraryFile {
    let size_bytes = metadata.len().min(i64::MAX as u64) as i64;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let parent_path = path.parent().unwrap_or(root);
    let parent = parent_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("sakana")
        .to_string();
    let depth = path
        .strip_prefix(root)
        .ok()
        .map(|relative| relative.components().count().saturating_sub(1))
        .unwrap_or(0);
    let modified_time = metadata.modified().ok();
    let modified_unix = modified_time.map(system_time_to_unix).unwrap_or_default();
    let modified = modified_time
        .map(format_system_time)
        .unwrap_or_else(|| "未知".to_string());
    let is_video = is_video_extension(&extension);
    let media_code = if is_video {
        extract_media_code(&name, code_regexes)
    } else {
        None
    };
    let cover_path = if is_video {
        Some(
            cover_path_for_video(path, metadata, cover_dir, cover_percent)
                .to_string_lossy()
                .to_string(),
        )
    } else {
        None
    };

    LibraryFile {
        id,
        name,
        path: path.to_string_lossy().to_string(),
        parent,
        extension,
        size_bytes,
        size_gb: bytes_to_gb(size_bytes),
        modified,
        modified_unix,
        depth,
        media_code,
        cover_path,
        is_root_file: parent_path == root,
    }
}

fn quiet_command(program: PathBuf) -> Command {
    let mut command = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn ensure_cover_for_video(
    video_path: &Path,
    metadata: &fs::Metadata,
    cover_dir: &Path,
    cover_percent: f64,
) -> Option<String> {
    if fs::create_dir_all(cover_dir).is_err() {
        return None;
    }

    let cover_path = cover_path_for_video(video_path, metadata, cover_dir, cover_percent);
    if cover_path.is_file() {
        return Some(cover_path.to_string_lossy().to_string());
    }

    if !ffmpeg_available() {
        return None;
    }

    let temporary_path = cover_path.with_extension("tmp.jpg");
    let _ = fs::remove_file(&temporary_path);
    let seek_timestamp = cover_seek_timestamp(video_path, cover_percent);

    let output = quiet_command(ffmpeg_tool_path("ffmpeg"))
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &seek_timestamp,
            "-i",
        ])
        .arg(video_path)
        .args(["-frames:v", "1", "-q:v", "3"])
        .arg(&temporary_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(result) if result.status.success() && temporary_path.is_file() => {
            let moved = fs::rename(&temporary_path, &cover_path)
                .or_else(|_| fs::copy(&temporary_path, &cover_path).map(|_| ()))
                .is_ok();
            if moved {
                let _ = fs::remove_file(&temporary_path);
                cover_path
                    .is_file()
                    .then(|| cover_path.to_string_lossy().to_string())
            } else {
                let _ = fs::remove_file(&temporary_path);
                None
            }
        }
        _ => {
            let _ = fs::remove_file(&temporary_path);
            None
        }
    }
}

fn cover_path_for_video(
    video_path: &Path,
    metadata: &fs::Metadata,
    cover_dir: &Path,
    cover_percent: f64,
) -> PathBuf {
    let modified_unix = metadata
        .modified()
        .map(system_time_to_unix)
        .unwrap_or_default();
    let normalized_percent = normalize_cover_capture_percent(Some(cover_percent));
    let mut hasher = Sha256::new();
    hasher.update(video_path.to_string_lossy().to_ascii_lowercase().as_bytes());
    hasher.update(b"|");
    hasher.update(metadata.len().to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(modified_unix.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(format!("{normalized_percent:.2}").as_bytes());
    let hash = hasher.finalize();
    cover_dir.join(format!("{hash:x}.jpg"))
}

fn normalize_cover_capture_percent(value: Option<f64>) -> f64 {
    let value = value.unwrap_or(DEFAULT_COVER_CAPTURE_PERCENT);
    if !value.is_finite() {
        return DEFAULT_COVER_CAPTURE_PERCENT;
    }
    value.clamp(0.0, 95.0)
}

fn cover_seek_timestamp(video_path: &Path, cover_percent: f64) -> String {
    let percent = normalize_cover_capture_percent(Some(cover_percent));
    let seconds = video_duration_seconds(video_path)
        .map(|duration| {
            if duration <= 0.0 {
                0.0
            } else {
                let target = duration * percent / 100.0;
                target.clamp(0.0, (duration - 0.2).max(0.0))
            }
        })
        .unwrap_or_else(|| if percent <= 0.0 { 0.0 } else { 5.0 });
    format!("{seconds:.3}")
}

fn video_duration_seconds(video_path: &Path) -> Option<f64> {
    let output = quiet_command(ffmpeg_tool_path("ffprobe"))
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(video_path)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let value = text.trim().parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

fn ffmpeg_available() -> bool {
    static FFMPEG_AVAILABLE: OnceLock<bool> = OnceLock::new();
    *FFMPEG_AVAILABLE.get_or_init(|| {
        quiet_command(ffmpeg_tool_path("ffmpeg"))
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    })
}

fn ffmpeg_tool_path(tool: &str) -> PathBuf {
    let executable = if cfg!(target_os = "windows") {
        format!("{tool}.exe")
    } else {
        tool.to_string()
    };

    if let Some(resource_dir) = APP_RESOURCE_DIR.get() {
        for candidate in [
            resource_dir.join(&executable),
            resource_dir.join("bin").join(&executable),
            resource_dir.join("ffmpeg").join(&executable),
        ] {
            if candidate.is_file() {
                return candidate;
            }
        }
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            for candidate in [
                exe_dir.join(&executable),
                exe_dir.join("bin").join(&executable),
                exe_dir.join("ffmpeg").join(&executable),
            ] {
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }

    PathBuf::from(tool)
}

fn build_actor_summaries(
    root: &Path,
    files: &[LibraryFile],
    direct_folders: &[DirectFolder],
    excluded_folders: &HashSet<String>,
) -> Vec<ActorSummary> {
    let mut summaries = direct_folders
        .iter()
        .filter(|folder| !is_system_folder(&folder.name))
        .filter(|folder| !excluded_folders.contains(&folder.name))
        .filter_map(|folder| {
            let mut folder_files = files
                .iter()
                .filter(|file| {
                    top_level_folder_name(root, &PathBuf::from(&file.path))
                        .map(|name| name == folder.name)
                        .unwrap_or(false)
                })
                .cloned()
                .collect::<Vec<_>>();

            if folder_files.is_empty() {
                return None;
            }

            folder_files.sort_by(|a, b| b.modified_unix.cmp(&a.modified_unix));
            let total_bytes = folder_files.iter().map(|file| file.size_bytes).sum::<i64>();
            let modified = folder_files
                .first()
                .map(|file| file.modified.clone())
                .unwrap_or_else(|| "未知".to_string());
            let modified_unix = folder_files
                .first()
                .map(|file| file.modified_unix)
                .unwrap_or_default();
            let videos = folder_files
                .iter()
                .filter(|file| is_video_extension(&file.extension))
                .map(|file| ActorVideo {
                    path: file.path.clone(),
                    name: file.name.clone(),
                    size_gb: file.size_gb,
                    modified: file.modified.clone(),
                    modified_unix: file.modified_unix,
                    media_code: file.media_code.clone(),
                    cover_path: file.cover_path.clone(),
                })
                .take(120)
                .collect::<Vec<_>>();

            if videos.is_empty() {
                return None;
            }

            Some(ActorSummary {
                name: folder.name.clone(),
                path: folder.path.clone(),
                file_count: folder_files.len(),
                total_size_gb: bytes_to_gb(total_bytes),
                modified,
                modified_unix,
                roles: actor_roles(videos.len(), bytes_to_gb(total_bytes), modified_unix),
                videos,
            })
        })
        .collect::<Vec<_>>();

    summaries.sort_by(|a, b| {
        b.modified_unix
            .cmp(&a.modified_unix)
            .then_with(|| b.videos.len().cmp(&a.videos.len()))
            .then_with(|| a.name.cmp(&b.name))
    });
    summaries
}

fn snapshot_actor_folders(
    direct_folders: &[DirectFolder],
    excluded_folders: &HashSet<String>,
) -> Vec<ActorFolderSnapshot> {
    let mut snapshots = direct_folders
        .iter()
        .filter(|folder| !is_system_folder(&folder.name))
        .filter(|folder| !excluded_folders.contains(&folder.name))
        .map(|folder| ActorFolderSnapshot {
            path: folder.path.clone(),
            modified_unix: folder.modified_unix,
        })
        .collect::<Vec<_>>();
    snapshots.sort_by(|a, b| a.path.cmp(&b.path));
    snapshots
}

fn default_processing_options() -> ProcessingOptions {
    ProcessingOptions {
        move_to_appreciation: true,
        rename_enabled: true,
        uppercase: true,
        normalize_dash: true,
        normalize_uncensored_suffix: false,
        remove_at_prefix: true,
        skip_if_exists: true,
        delete_source_folder_after_move: false,
    }
}

fn actor_roles(video_count: usize, total_size_gb: f64, modified_unix: i64) -> Vec<String> {
    let now = Utc::now().timestamp();
    let mut roles = Vec::new();
    if video_count >= 50 {
        roles.push("高频收藏".to_string());
    }
    if ((now - modified_unix).max(0) / 86_400) <= 14 {
        roles.push("最近新增".to_string());
    }
    if total_size_gb >= 300.0 {
        roles.push("大目录".to_string());
    }
    if roles.is_empty() {
        roles.push("普通演员".to_string());
    }
    roles
}

fn top_level_folder_name(root: &Path, file_path: &Path) -> Option<String> {
    file_path
        .strip_prefix(root)
        .ok()?
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .map(|value| value.to_string())
}

fn is_system_folder(name: &str) -> bool {
    name.starts_with('_')
        || name.eq_ignore_ascii_case("System Volume Information")
        || name.eq_ignore_ascii_case("$RECYCLE.BIN")
}

fn is_incomplete_extension(extension: &str) -> bool {
    INCOMPLETE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
}

fn resolve_configured_dir(
    root: &Path,
    configured: Option<&str>,
    default_name: &str,
) -> Result<PathBuf> {
    let candidate = configured
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(default_name));
    let path = if candidate.is_absolute() {
        candidate
    } else {
        root.join(candidate)
    };
    ensure_path_under_root(root, &path)?;
    if path.exists() {
        let canonical = path.canonicalize().context("无法解析目录")?;
        if canonical == root {
            anyhow::bail!("指定目录不能是库根目录");
        }
        Ok(canonical)
    } else {
        Ok(path)
    }
}

fn configured_top_level_folders(root: &Path, paths: &[&Path]) -> HashSet<String> {
    paths
        .iter()
        .filter_map(|path| {
            path.strip_prefix(root)
                .ok()?
                .components()
                .next()
                .and_then(|component| component.as_os_str().to_str())
                .map(|value| value.to_string())
        })
        .collect()
}

fn ensure_path_under_root(root: &Path, path: &Path) -> Result<()> {
    if path.exists() {
        let canonical = path.canonicalize().context("无法解析目录")?;
        if !canonical.starts_with(root) {
            anyhow::bail!("目录不在库根目录内");
        }
        return Ok(());
    }

    let mut existing = path.to_path_buf();
    while !existing.exists() {
        if !existing.pop() {
            anyhow::bail!("目录不存在");
        }
    }
    let canonical_existing = existing.canonicalize().context("无法解析目录")?;
    if !canonical_existing.starts_with(root) {
        anyhow::bail!("目录不在库根目录内");
    }
    Ok(())
}

fn build_stale_folders(files: &[LibraryFile], direct_folders: &[DirectFolder]) -> Vec<StaleFolder> {
    let now = Utc::now().timestamp();
    let mut by_folder: HashMap<String, Vec<LibraryFile>> = HashMap::new();
    for file in files.iter().filter(|file| file.depth == 1) {
        by_folder
            .entry(file.parent.clone())
            .or_default()
            .push(file.clone());
    }

    let mut stale = direct_folders
        .iter()
        .map(|folder| {
            let mut folder_files = by_folder.remove(&folder.name).unwrap_or_default();
            folder_files.sort_by(|a, b| b.modified_unix.cmp(&a.modified_unix));
            let latest = folder_files.first();
            let days_since = latest
                .map(|file| ((now - file.modified_unix).max(0)) / 86_400)
                .unwrap_or(9_999);
            let is_stale = days_since >= 60;
            StaleFolder {
                folder: folder.name.clone(),
                path: folder.path.clone(),
                latest_file: latest
                    .map(|file| file.name.clone())
                    .unwrap_or_else(|| "(空文件夹)".to_string()),
                latest_modified: latest
                    .map(|file| file.modified.clone())
                    .unwrap_or_else(|| "——".to_string()),
                days_since,
                is_stale,
                file_count: folder_files.len(),
                suggestion: if is_stale { "归档/复查" } else { "观察" }.to_string(),
            }
        })
        .collect::<Vec<_>>();

    stale.sort_by(|a, b| {
        b.days_since
            .cmp(&a.days_since)
            .then_with(|| a.folder.cmp(&b.folder))
    });
    stale
}

fn read_direct_folders(root: &Path) -> Result<Vec<DirectFolder>> {
    let mut folders = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        folders.push(DirectFolder {
            name,
            path: path.to_string_lossy().to_string(),
            modified_unix: directory_modified_unix(&path),
        });
    }
    Ok(folders)
}

fn directory_modified_unix(path: &Path) -> i64 {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .map(system_time_to_unix)
        .unwrap_or_default()
}

fn parse_report_scanned_at(value: &str) -> Option<i64> {
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S UTC")
        .ok()
        .map(|datetime| datetime.and_utc().timestamp())
}

fn rename_one(root: &Path, source_text: &str, target_text: &str) -> Result<String> {
    let source = ensure_existing_file_under_root(root, source_text)?;
    let target = ensure_target_file_under_root(root, target_text)?;

    if source.parent() != target.parent() {
        anyhow::bail!("重命名只能在同一目录内执行");
    }
    if target.exists() && !paths_same_ignore_case(&source, &target) {
        anyhow::bail!("目标文件已存在");
    }

    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value))
        .unwrap_or_default();
    let tmp = source.with_file_name(format!(
        "__KAWA_TMP_{}_{}{}",
        Utc::now().timestamp_millis(),
        source
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("file"),
        extension
    ));

    fs::rename(&source, &tmp).with_context(|| "无法改为临时文件名")?;
    if let Err(error) = fs::rename(&tmp, &target).with_context(|| "无法改为目标文件名") {
        let _ = fs::rename(&tmp, &source);
        return Err(error);
    }
    Ok("重命名成功".to_string())
}

fn move_one_file(root: &Path, source_text: &str, target_text: &str) -> Result<String> {
    let source = ensure_existing_file_under_root(root, source_text)?;
    let target = ensure_target_file_under_root(root, target_text)?;

    if target.exists() {
        anyhow::bail!("目标文件已存在");
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::rename(&source, &target).with_context(|| "移动文件失败")?;
    Ok("移动成功".to_string())
}

fn build_archive_move_preview(
    root: &Path,
    appreciation_dir: &Path,
    video_path: &str,
    actor_path: &str,
) -> Result<ArchiveMovePreview> {
    let appreciation_dir = if appreciation_dir.exists() {
        appreciation_dir
            .canonicalize()
            .context("无法解析欣赏区目录")?
    } else {
        appreciation_dir.to_path_buf()
    };
    let source = ensure_existing_file_under_root(root, video_path)?;
    if !source.starts_with(&appreciation_dir) {
        anyhow::bail!("源文件不在欣赏区内");
    }

    let actor_dir = ensure_target_dir_under_root_allow_missing(root, actor_path)?;
    if is_reserved_actor_dir(root, &actor_dir) {
        anyhow::bail!("目标演员目录不能是系统目录");
    }
    let actor_name = actor_dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let target = actor_dir.join(source.file_name().context("源文件名无效")?);
    let exists = target.exists() && !paths_same_ignore_case(&source, &target);
    let status = if exists { "conflict" } else { "ready" };
    let message = if exists {
        let file_name = target
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("目标文件");
        format!("演员目录已有同名文件：{}，不会覆盖", file_name)
    } else {
        "确认后会从欣赏区移动到演员目录".to_string()
    };

    Ok(ArchiveMovePreview {
        source_path: source.to_string_lossy().to_string(),
        target_path: target.to_string_lossy().to_string(),
        actor_name,
        status: status.to_string(),
        message,
    })
}

fn archive_move_from_preview(root: &Path, preview: &ArchiveMovePreview) -> Result<String> {
    if preview.status != "ready" {
        anyhow::bail!(preview.message.clone());
    }
    if let Some(parent) = PathBuf::from(&preview.target_path).parent() {
        fs::create_dir_all(parent).context("无法创建演员目录")?;
    }
    move_one_file(root, &preview.source_path, &preview.target_path)
}

fn delete_archive_file(root: &Path, appreciation_dir: &Path, video_path: &str) -> Result<String> {
    let appreciation_dir = appreciation_dir
        .canonicalize()
        .context("无法解析欣赏区目录")?;
    let source = ensure_existing_file_under_root(root, video_path)?;
    if !source.starts_with(&appreciation_dir) {
        anyhow::bail!("只能删除欣赏区内的视频");
    }
    fs::remove_file(&source).context("删除视频失败")?;
    Ok("删除成功".to_string())
}
fn process_download_item(
    root: &Path,
    download_dir: &Path,
    appreciation_dir: &Path,
    request: &DownloadProcessingRequest,
    options: &ProcessingOptions,
) -> Result<String> {
    if !options.move_to_appreciation {
        anyhow::bail!("已关闭“移动到欣赏区”");
    }

    let download_dir = download_dir.canonicalize().context("下载区目录不存在")?;
    fs::create_dir_all(appreciation_dir).context("无法创建欣赏区目录")?;
    let appreciation_dir = appreciation_dir
        .canonicalize()
        .context("无法解析欣赏区目录")?;

    let source = ensure_existing_file_under_root(root, &request.source_path)?;
    if !source.starts_with(&download_dir) {
        anyhow::bail!("源文件不在下载区内");
    }

    let target = ensure_target_file_under_root_allow_missing(root, &request.target_path)?;
    if let Some(target_parent) = target.parent() {
        fs::create_dir_all(target_parent).context("无法创建目标目录")?;
    }
    if !target.starts_with(&appreciation_dir) {
        anyhow::bail!("目标文件不在欣赏区内");
    }

    if target.exists() {
        if options.skip_if_exists {
            anyhow::bail!("目标文件已存在，跳过");
        }
        anyhow::bail!("目标文件已存在");
    }

    let source_parent = source.parent().map(Path::to_path_buf);
    fs::rename(&source, &target).with_context(|| "移动到欣赏区失败")?;

    let cleanup_message = if options.delete_source_folder_after_move {
        source_parent
            .as_deref()
            .map(|folder| cleanup_download_folder_after_move(&download_dir, folder))
            .transpose()?
            .unwrap_or_default()
    } else {
        String::new()
    };

    if cleanup_message.is_empty() {
        Ok("已移动到欣赏区".to_string())
    } else {
        Ok(format!("已移动到欣赏区，{}", cleanup_message))
    }
}

fn cleanup_download_folder_after_move(download_dir: &Path, folder: &Path) -> Result<String> {
    let folder = folder.canonicalize().context("无法解析待删除目录")?;
    if folder == download_dir {
        return Ok(String::new());
    }
    if !folder.starts_with(download_dir) {
        anyhow::bail!("待删除目录不在下载区内");
    }

    let mut has_incomplete = false;
    let mut has_video = false;
    for entry in WalkDir::new(&folder).follow_links(false) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let extension = entry
            .path()
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension == "xltd" {
            has_incomplete = true;
            break;
        }
        if is_video_extension(&extension) {
            has_video = true;
        }
    }

    if has_incomplete {
        return Ok("仍有 .xltd 未完成下载，已保留下载文件夹".to_string());
    }
    if has_video {
        return Ok("仍有视频未处理，已保留下载文件夹".to_string());
    }

    fs::remove_dir_all(&folder).context("删除下载文件夹失败")?;
    Ok("未发现 .xltd，已删除下载文件夹".to_string())
}

fn push_outcome(
    result: &mut OperationResult,
    action: &str,
    source_path: &str,
    target_path: &str,
    outcome: Result<String>,
) {
    let (status, message) = match outcome {
        Ok(message) => {
            result.success += 1;
            ("success".to_string(), message)
        }
        Err(error) => {
            let message = error.to_string();
            if message.contains("已存在")
                || message.contains("状态不是 ready")
                || message.contains("同名文件")
                || message.contains("跳过")
                || message.contains("空目录")
                || message.contains("未完成")
            {
                result.skipped += 1;
                ("skipped".to_string(), message)
            } else {
                result.failed += 1;
                ("failed".to_string(), message)
            }
        }
    };

    result.logs.push(OperationLog {
        id: 0,
        timestamp: Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        action: action.to_string(),
        source_path: source_path.to_string(),
        target_path: target_path.to_string(),
        status,
        message,
    });
}

fn canonical_root(root_path: &str) -> Result<PathBuf> {
    let root = PathBuf::from(root_path);
    if !root.exists() || !root.is_dir() {
        anyhow::bail!("根目录不存在或不是文件夹");
    }
    root.canonicalize().context("无法解析根目录")
}

fn ensure_existing_file_under_root(root: &Path, path_text: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path_text);
    let canonical = path.canonicalize().context("源文件不存在")?;
    if !canonical.is_file() {
        anyhow::bail!("源路径不是文件");
    }
    if !canonical.starts_with(root) {
        anyhow::bail!("源文件不在库目录内");
    }
    Ok(canonical)
}

fn ensure_existing_dir_under_root(root: &Path, path_text: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path_text);
    let canonical = path.canonicalize().context("目录不存在")?;
    if !canonical.is_dir() {
        anyhow::bail!("路径不是目录");
    }
    if !canonical.starts_with(root) {
        anyhow::bail!("目录不在库目录内");
    }
    Ok(canonical)
}

fn is_reserved_actor_dir(root: &Path, actor_dir: &Path) -> bool {
    actor_dir == root
        || actor_dir
            .strip_prefix(root)
            .ok()
            .and_then(|relative| relative.components().next())
            .and_then(|component| component.as_os_str().to_str())
            .map(|name| {
                is_system_folder(name)
                    || name.eq_ignore_ascii_case(DOWNLOAD_FOLDER)
                    || name.eq_ignore_ascii_case(APPRECIATION_FOLDER)
                    || name.eq_ignore_ascii_case(REVIEW_FOLDER)
            })
            .unwrap_or(true)
}

fn actor_dir_for_name(root: &Path, actor_name: &str) -> Result<PathBuf> {
    let name = sanitize_actor_folder_name(actor_name)?;
    ensure_target_dir_under_root_allow_missing(root, &root.join(name).to_string_lossy())
}

fn sanitize_actor_folder_name(actor_name: &str) -> Result<String> {
    let cleaned = actor_name
        .trim()
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>()
        .trim_matches([' ', '.'])
        .to_string();
    if cleaned.is_empty() {
        anyhow::bail!("演员名称为空，无法新建文件夹");
    }
    if is_system_folder(&cleaned)
        || cleaned.eq_ignore_ascii_case(DOWNLOAD_FOLDER)
        || cleaned.eq_ignore_ascii_case(APPRECIATION_FOLDER)
        || cleaned.eq_ignore_ascii_case(REVIEW_FOLDER)
    {
        anyhow::bail!("演员名称不能是系统目录");
    }
    Ok(cleaned)
}

fn ensure_target_dir_under_root_allow_missing(root: &Path, path_text: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path_text);
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) {
        anyhow::bail!("目标目录不能包含相对目录");
    }

    let mut existing = path.clone();
    while !existing.exists() {
        if !existing.pop() {
            anyhow::bail!("目标父目录不存在");
        }
    }

    let canonical_existing = existing.canonicalize().context("无法解析目标父目录")?;
    if !canonical_existing.starts_with(root) {
        anyhow::bail!("目标目录不在库目录内");
    }
    if path.exists() && !path.is_dir() {
        anyhow::bail!("目标路径不是目录");
    }

    let missing_tail = path
        .strip_prefix(&existing)
        .unwrap_or_else(|_| Path::new(""));
    Ok(canonical_existing.join(missing_tail))
}
fn ensure_target_file_under_root(root: &Path, path_text: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path_text);
    let parent = path.parent().context("目标路径无父目录")?;
    let canonical_parent = parent.canonicalize().context("目标父目录不存在")?;
    if !canonical_parent.starts_with(root) {
        anyhow::bail!("目标路径不在库目录内");
    }
    let file_name = path.file_name().context("目标文件名无效")?;
    Ok(canonical_parent.join(file_name))
}

fn ensure_target_file_under_root_allow_missing(root: &Path, path_text: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path_text);
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) {
        anyhow::bail!("目标路径不能包含相对目录");
    }

    let file_name = path.file_name().context("目标文件名无效")?.to_os_string();
    let parent = path.parent().context("目标路径无父目录")?;
    let mut existing = parent.to_path_buf();
    while !existing.exists() {
        if !existing.pop() {
            anyhow::bail!("目标父目录不存在");
        }
    }

    let canonical_existing = existing.canonicalize().context("无法解析目标父目录")?;
    if !canonical_existing.starts_with(root) {
        anyhow::bail!("目标路径不在库目录内");
    }

    let missing_tail = parent
        .strip_prefix(&existing)
        .unwrap_or_else(|_| Path::new(""));
    Ok(canonical_existing.join(missing_tail).join(file_name))
}

fn open_with_system(file: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(file)
            .spawn()
            .context("无法打开文件")?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(file)
            .spawn()
            .context("无法打开文件")?;
        Ok(())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(file)
            .spawn()
            .context("无法打开文件")?;
        Ok(())
    }
}

fn reveal_in_file_manager(file: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(format!("/select,{}", shell_display_path(file)))
            .spawn()
            .context("无法在文件管理器中定位文件")?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-R", &shell_display_path(file)])
            .spawn()
            .context("无法在文件管理器中定位文件")?;
        Ok(())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let parent = file.parent().unwrap_or(file);
        Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .context("无法在文件管理器中定位文件")?;
        Ok(())
    }
}

fn open_external(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        quiet_command(PathBuf::from("cmd"))
            .args(["/c", "start", "", url])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("无法打开链接")?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .context("无法打开链接")?;
        Ok(())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .context("无法打开链接")?;
        Ok(())
    }
}

fn shell_display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = text.as_ref();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", rest)
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        text.to_string()
    }
}

fn restore_window_size(app: &tauri::AppHandle, data_dir: &Path) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let state_path = data_dir.join(WINDOW_STATE_FILE);
    if let Ok(raw) = fs::read_to_string(&state_path) {
        if let Ok(size) = serde_json::from_str::<AppWindowSize>(&raw) {
            let _ = window.set_size(Size::Physical(PhysicalSize {
                width: size.width.max(MIN_WINDOW_WIDTH),
                height: size.height.max(MIN_WINDOW_HEIGHT),
            }));
        }
    }

    let save_path = state_path.clone();
    let tracked_window = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(size) => save_window_size(&save_path, size.width, size.height),
        WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed => {
            if let Ok(size) = tracked_window.inner_size() {
                save_window_size(&save_path, size.width, size.height);
            }
        }
        _ => {}
    });
}

fn save_window_size(path: &Path, width: u32, height: u32) {
    let state = AppWindowSize {
        width: width.max(MIN_WINDOW_WIDTH),
        height: height.max(MIN_WINDOW_HEIGHT),
    };
    if let Ok(raw) = serde_json::to_string(&state) {
        let _ = fs::write(path, raw);
    }
}

fn confidence_rank(value: &str) -> u8 {
    match value {
        "high" => 3,
        "medium" => 2,
        _ => 1,
    }
}

fn status_rank(value: &str) -> u8 {
    match value {
        "ready" => 3,
        "conflict" => 2,
        "skipped" => 1,
        _ => 0,
    }
}

fn top_level_folder_key(root: &Path, file_path: &Path) -> (String, String) {
    let relative = file_path.strip_prefix(root).unwrap_or(file_path);
    let folder_name = relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .unwrap_or("sakana")
        .to_string();
    let folder_path = root.join(&folder_name).to_string_lossy().to_string();
    (folder_name, folder_path)
}

struct CodeRegexes {
    fc2: Regex,
    catalog: Regex,
}

fn build_code_regexes() -> Result<CodeRegexes> {
    Ok(CodeRegexes {
        fc2: Regex::new(r"(?i)FC2[-_ ]?PPV[-_ ]?(\d{6,8})").context("failed to build FC2 regex")?,
        catalog: Regex::new(r"(?i)([A-Z]{2,6})[-_ ]?(\d{3,5})")
            .context("failed to build catalog regex")?,
    })
}

fn extract_media_code(name: &str, regexes: &CodeRegexes) -> Option<String> {
    if let Some(captures) = regexes.fc2.captures(name) {
        return Some(format!("FC2PPV-{}", &captures[1]));
    }

    let captures = regexes.catalog.captures(name)?;
    Some(format!(
        "{}-{}",
        captures[1].to_ascii_uppercase(),
        &captures[2]
    ))
}

fn normalize_archive_code(value: &str) -> Result<String> {
    let regexes = build_code_regexes()?;
    let stem = Path::new(value)
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or(value);
    let cleaned = stem
        .replace('@', " ")
        .replace(['_', '.'], "-")
        .to_ascii_uppercase();
    extract_media_code(&cleaned, &regexes).ok_or_else(|| anyhow::anyhow!("没有识别到番号"))
}

fn lookup_javbus_metadata(
    site_url: &str,
    code: &str,
    actors: &[DirectFolder],
    data_dir: &Path,
    session: Option<&MetadataSession>,
) -> Result<ArchiveLookup> {
    let search_url = build_metadata_search_url(site_url, code);
    let search_html = http_get_text_with_session(&search_url, session).context("搜索页请求失败")?;
    if is_age_verification_page(&search_html) {
        anyhow::bail!(
            "JavBus 返回年龄验证页，请先在设置中导入浏览器会话，或打开验证页完成验证后再导入"
        );
    }
    let mut search_actor_names = extract_actor_names(&search_html);
    let mut resolved_source_url = search_url.clone();

    if search_actor_names.is_empty() {
        let fallback_url = build_metadata_type_one_search_url(site_url, code);
        if let Ok(fallback_html) = http_get_text_with_session(&fallback_url, session) {
            let fallback_names = extract_actor_names(&fallback_html);
            if !fallback_names.is_empty() {
                search_actor_names = fallback_names;
                resolved_source_url = fallback_url;
            }
        }
    }

    let detail_url = extract_detail_url(&search_html, &search_url, code);
    let detail_html = detail_url
        .as_deref()
        .map(|url| http_get_text_with_session(url, session))
        .transpose()
        .context("详情页请求失败")?
        .unwrap_or_default();
    let detail_actor_names = extract_actor_names(&detail_html);
    let actor_names = if !detail_actor_names.is_empty() {
        merge_actor_names(detail_actor_names, search_actor_names)
    } else {
        search_actor_names
    };
    let matched_actor = actor_names
        .iter()
        .find_map(|name| match_actor_folder(actors, name))
        .or_else(|| {
            actor_names
                .iter()
                .find_map(|name| match_actor_folder_by_contains(actors, name))
        });
    let resolved_source_url = detail_url.unwrap_or(resolved_source_url);
    let avatar_url = extract_actor_avatar_url(&detail_html, &resolved_source_url)
        .or_else(|| extract_searchstar_avatar_url(&search_html, &search_url))
        .or_else(|| extract_first_image_url(&detail_html, &resolved_source_url));
    let avatar_path = if let Some(url) = avatar_url.as_deref() {
        matched_actor.as_ref().and_then(|actor| {
            download_avatar(
                url,
                data_dir,
                &actor.path,
                session,
                Some(&resolved_source_url),
            )
            .ok()
        })
    } else {
        None
    };
    let searched_at = now_string();

    Ok(if let Some(actor) = matched_actor {
        ArchiveLookup {
            code: code.to_string(),
            status: "searched".to_string(),
            actor_name: Some(actor.name),
            actor_path: Some(actor.path),
            avatar_path,
            source_url: Some(resolved_source_url.clone()),
            message: if actor_names.is_empty() {
                "已联网搜索，但页面未解析出演员；按本地目录兜底匹配".to_string()
            } else {
                format!("已搜索：{}", actor_names.join(" / "))
            },
            searched_at,
        }
    } else {
        ArchiveLookup {
            code: code.to_string(),
            status: "searched".to_string(),
            actor_name: actor_names.first().cloned(),
            actor_path: None,
            avatar_path: None,
            source_url: Some(resolved_source_url),
            message: if actor_names.is_empty() {
                "已搜索，但没有解析到演员或本地没有对应目录".to_string()
            } else {
                format!(
                    "已搜索到演员：{}；本地没有对应目录",
                    actor_names.join(" / ")
                )
            },
            searched_at,
        }
    })
}

fn match_actor_folder(actors: &[DirectFolder], actor_name: &str) -> Option<DirectFolder> {
    actors
        .iter()
        .find(|actor| actor.name.eq_ignore_ascii_case(actor_name))
        .cloned()
}

fn match_actor_folder_by_contains(
    actors: &[DirectFolder],
    actor_name: &str,
) -> Option<DirectFolder> {
    let needle = actor_name.trim();
    if needle.is_empty() {
        return None;
    }
    actors
        .iter()
        .find(|actor| actor.name.contains(needle) || needle.contains(&actor.name))
        .cloned()
}

fn extract_detail_url(html: &str, base_url: &str, code: &str) -> Option<String> {
    let compact = code.replace('-', "");
    let href_regex =
        Regex::new(r#"(?is)<a[^>]+href\s*=\s*["']([^"']+)["'][^>]*>(.*?)</a>"#).ok()?;
    for captures in href_regex.captures_iter(html) {
        let href = decode_html_entities(captures.get(1)?.as_str());
        if href.starts_with("javascript:") {
            continue;
        }
        let resolved = resolve_url(base_url, &href);
        if !is_detail_page_url(&resolved, &compact) {
            continue;
        }
        return Some(resolved);
    }
    None
}

fn extract_actor_names(html: &str) -> Vec<String> {
    let html = strip_script_and_style_blocks(html);
    let mut names = Vec::new();
    let star_name_regex =
        Regex::new(r#"(?is)<[^>]*class\s*=\s*["'][^"']*star-name[^"']*["'][^>]*>(.*?)</[^>]+>"#)
            .unwrap();
    for captures in star_name_regex.captures_iter(&html) {
        let name = strip_html_tags(
            captures
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default(),
        );
        let name = clean_metadata_text(&name);
        if is_plausible_actor_name(&name) && !names.iter().any(|existing| existing == &name) {
            names.push(name);
        }
    }

    let star_regex =
        Regex::new(r#"(?is)<a[^>]+href\s*=\s*["'][^"']*/star/[^"']+["'][^>]*>(.*?)</a>"#).unwrap();
    for captures in star_regex.captures_iter(&html) {
        let name = strip_html_tags(
            captures
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default(),
        );
        let name = clean_metadata_text(&name);
        if is_plausible_actor_name(&name) && !names.iter().any(|existing| existing == &name) {
            names.push(name);
        }
    }

    if names.is_empty() {
        let title_regex =
            Regex::new(r#"(?is)<span[^>]*>\s*(.*?)\s*<br\s*/?>\s*([^<]{1,64})\s*</span>"#).unwrap();
        for captures in title_regex.captures_iter(&html) {
            let candidate = clean_metadata_text(
                captures
                    .get(2)
                    .map(|value| value.as_str())
                    .unwrap_or_default(),
            );
            if is_plausible_actor_name(&candidate)
                && !names.iter().any(|existing| existing == &candidate)
            {
                names.push(candidate);
            }
        }
    }

    if names.is_empty() {
        let actor_block = Regex::new(
            r#"(?is)(演員|演员|出演者|Actor|Actress|Star)\s*[:：]?\s*(.*?)\s*(</p>|</div>|<br)"#,
        )
        .unwrap();
        for captures in actor_block.captures_iter(&html) {
            let text = strip_html_tags(
                captures
                    .get(2)
                    .map(|value| value.as_str())
                    .unwrap_or_default(),
            );
            for part in text.split([':', '：', '/', ',', '、', '\n', '\r', '\t']) {
                let name = clean_metadata_text(part);
                if is_plausible_actor_name(&name) && !names.iter().any(|existing| existing == &name)
                {
                    names.push(name);
                }
            }
        }
    }
    names
}

fn merge_actor_names(primary: Vec<String>, secondary: Vec<String>) -> Vec<String> {
    let mut names = Vec::new();
    for name in primary.into_iter().chain(secondary.into_iter()) {
        if !name.is_empty() && !names.iter().any(|existing| existing == &name) {
            names.push(name);
        }
    }
    names
}

fn is_detail_page_url(url: &str, compact_code: &str) -> bool {
    let parsed = match ParsedUrl::parse(url) {
        Some(parsed) => parsed,
        None => return false,
    };
    let path = parsed.path.trim_matches('/');
    if path.is_empty() {
        return false;
    }
    if path.contains('/') {
        return false;
    }
    let upper_path = path.to_ascii_uppercase();
    if upper_path.starts_with("SEARCH")
        || upper_path.starts_with("SEARCHSTAR")
        || upper_path.starts_with("STAR")
        || upper_path.starts_with("FORUM")
    {
        return false;
    }
    compact_code_from_text(path) == compact_code
}

fn strip_script_and_style_blocks(html: &str) -> String {
    let script_regex = Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let style_regex = Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    let without_scripts = script_regex.replace_all(html, " ");
    style_regex.replace_all(&without_scripts, " ").to_string()
}

fn extract_actor_avatar_url(html: &str, base_url: &str) -> Option<String> {
    let star_img = Regex::new(
        r#"(?is)<a[^>]+href\s*=\s*["'][^"']*/star/[^"']+["'][^>]*>.*?<img[^>]+src\s*=\s*["']([^"']+)["']"#,
    )
    .ok()?;
    star_img
        .captures(html)
        .and_then(|captures| captures.get(1))
        .map(|value| resolve_url(base_url, &decode_html_entities(value.as_str())))
}

fn extract_searchstar_avatar_url(html: &str, base_url: &str) -> Option<String> {
    let img_regex =
        Regex::new(r#"(?is)<img[^>]+src\s*=\s*["']([^"']*?/pics/actress/[^"']+)["'][^>]*>"#)
            .ok()?;
    img_regex
        .captures(html)
        .and_then(|captures| captures.get(1))
        .map(|value| resolve_url(base_url, &decode_html_entities(value.as_str())))
}

fn extract_first_image_url(html: &str, base_url: &str) -> Option<String> {
    let img_regex = Regex::new(r#"(?is)<img[^>]+src\s*=\s*["']([^"']+)["']"#).ok()?;
    for captures in img_regex.captures_iter(html) {
        let src = decode_html_entities(captures.get(1)?.as_str());
        let lower = src.to_ascii_lowercase();
        if lower.contains("logo") || lower.contains("blank") || lower.ends_with(".svg") {
            continue;
        }
        return Some(resolve_url(base_url, &src));
    }
    None
}

fn clean_metadata_text(value: &str) -> String {
    decode_html_entities(value)
        .replace('\u{00a0}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|char: char| {
            char.is_ascii_punctuation()
                || matches!(char, '：' | '，' | '。' | '、' | '《' | '》' | '【' | '】')
        })
        .to_string()
}

fn strip_html_tags(value: &str) -> String {
    let tag_regex = Regex::new(r"(?is)<[^>]+>").unwrap();
    tag_regex.replace_all(value, " ").to_string()
}

fn decode_html_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

fn build_metadata_search_url(site_url: &str, code: &str) -> String {
    let base = normalize_site_url(site_url);
    format!("{}/search/{}", base.trim_end_matches('/'), code)
}

fn build_metadata_type_one_search_url(site_url: &str, code: &str) -> String {
    let base = normalize_site_url(site_url);
    format!("{}/search/{}&type=1", base.trim_end_matches('/'), code)
}

fn build_actor_search_url(site_url: &str, actor_name: &str) -> String {
    let base = normalize_site_url(site_url);
    format!(
        "{}/searchstar/{}",
        base.trim_end_matches('/'),
        percent_encode_path_segment(actor_name)
    )
}

fn normalize_site_url(site_url: &str) -> String {
    let value = site_url.trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        value.trim_end_matches('/').to_string()
    } else {
        format!("https://{}", value.trim_end_matches('/'))
    }
}

fn resolve_url(base_url: &str, href: &str) -> String {
    let href = href.trim();
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    let parsed = ParsedUrl::parse(base_url).unwrap_or_else(|| ParsedUrl {
        scheme: "https".to_string(),
        host: "www.javbus.com".to_string(),
        port: None,
        path: "/".to_string(),
    });
    if href.starts_with("//") {
        return format!("{}:{}", parsed.scheme, href);
    }
    if href.starts_with('/') {
        return format!("{}://{}{}", parsed.scheme, parsed.authority(), href);
    }
    let parent = parsed
        .path
        .rsplit_once('/')
        .map(|(left, _)| left)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    format!(
        "{}://{}/{}",
        parsed.scheme,
        parsed.authority(),
        [parent, href].join("/").trim_start_matches('/')
    )
}

fn download_avatar(
    url: &str,
    data_dir: &Path,
    actor_path: &str,
    session: Option<&MetadataSession>,
    referer: Option<&str>,
) -> Result<String> {
    let bytes = http_get_bytes_with_session(url, session, referer).context("头像下载失败")?;
    let extension = image_extension_from_url(url);
    let avatars_dir = data_dir.join("avatars");
    fs::create_dir_all(&avatars_dir)?;
    let file_name = format!("{}{}", stable_path_hash(actor_path), extension);
    let target = avatars_dir.join(file_name);
    fs::write(&target, bytes).context("头像写入失败")?;
    Ok(target.to_string_lossy().to_string())
}

fn image_extension_from_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains(".png") {
        ".png"
    } else if lower.contains(".webp") {
        ".webp"
    } else {
        ".jpg"
    }
}

fn stable_path_hash(value: &str) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:016x}")
}

fn now_string() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn is_plausible_actor_name(value: &str) -> bool {
    if value.len() < 2 {
        return false;
    }
    let upper = value.to_ascii_uppercase();
    if ["演員", "演员", "出演者", "ACTOR", "ACTRESS", "STAR"].contains(&upper.as_str()) {
        return false;
    }
    if value.contains("http")
        || value.contains('/')
        || value.contains("磁力")
        || value.contains("樣品")
    {
        return false;
    }
    if value == "0" || value.chars().all(|char| char.is_ascii_digit()) {
        return false;
    }
    if value.contains("function")
        || value.contains("searchs")
        || value.contains("ajax")
        || value.contains("searchinput")
        || value.contains("modal")
    {
        return false;
    }
    true
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            encoded.push(ch);
        } else {
            encoded.push_str(&format!("%{:02X}", byte));
        }
    }
    encoded
}

fn compact_code_from_text(value: &str) -> String {
    value
        .chars()
        .filter(|char| char.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

fn normalize_site_host(site_url: &str) -> String {
    let normalized = normalize_site_url(site_url);
    ParsedUrl::parse(&normalized)
        .map(|parsed| parsed.host.to_ascii_lowercase())
        .unwrap_or_else(|| "www.javbus.com".to_string())
}

fn normalize_cookie_header_value(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("Cookie:")
        .trim_start_matches("cookie:")
        .replace(['\r', '\n'], " ")
        .trim()
        .to_string()
}

fn is_age_verification_page(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    lower.contains("age verification javbus")
        || lower.contains("driver-verify")
        || lower.contains("你是否已經成年")
        || lower.contains("我已經成年")
}

#[derive(Clone)]
struct ParsedUrl {
    scheme: String,
    host: String,
    port: Option<u16>,
    path: String,
}

impl ParsedUrl {
    fn parse(url: &str) -> Option<Self> {
        let trimmed = url.trim();
        let (scheme, rest) = trimmed
            .split_once("://")
            .map(|(scheme, rest)| (scheme.to_ascii_lowercase(), rest))
            .unwrap_or_else(|| ("https".to_string(), trimmed));
        if scheme != "http" && scheme != "https" {
            return None;
        }
        let (authority, path) = rest
            .split_once('/')
            .map(|(authority, path)| (authority, format!("/{}", path)))
            .unwrap_or((rest, "/".to_string()));
        let (host, port) = authority
            .rsplit_once(':')
            .and_then(|(host, port)| Some((host.to_string(), Some(port.parse::<u16>().ok()?))))
            .unwrap_or_else(|| (authority.to_string(), None));
        if host.is_empty() {
            return None;
        }
        Some(Self {
            scheme,
            host,
            port,
            path,
        })
    }

    fn authority(&self) -> String {
        match self.port {
            Some(port) => format!("{}:{}", self.host, port),
            None => self.host.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum BrowserKind {
    Edge,
    Chrome,
}

impl BrowserKind {
    fn label(self) -> &'static str {
        match self {
            Self::Edge => "Edge",
            Self::Chrome => "Chrome",
        }
    }

    #[cfg(target_os = "windows")]
    fn user_data_dir(self) -> Result<PathBuf> {
        let local_app_data =
            std::env::var("LOCALAPPDATA").context("LOCALAPPDATA 环境变量不存在")?;
        Ok(match self {
            Self::Edge => PathBuf::from(local_app_data)
                .join("Microsoft")
                .join("Edge")
                .join("User Data"),
            Self::Chrome => PathBuf::from(local_app_data)
                .join("Google")
                .join("Chrome")
                .join("User Data"),
        })
    }
}

#[derive(Debug)]
struct ChromiumProfile {
    browser: BrowserKind,
    user_data_dir: PathBuf,
    profile_name: String,
    profile_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ChromiumLocalState {
    os_crypt: Option<ChromiumOsCrypt>,
}

#[derive(Debug, Deserialize)]
struct ChromiumOsCrypt {
    encrypted_key: Option<String>,
}

fn import_browser_session_for_site(
    site_host: &str,
    browser: Option<&str>,
    data_dir: &Path,
) -> Result<MetadataSession> {
    #[cfg(target_os = "windows")]
    {
        let mut attempts = Vec::new();
        for browser_kind in requested_browser_kinds(browser)? {
            let profiles = collect_chromium_profiles(browser_kind)?;
            if profiles.is_empty() {
                attempts.push(format!("{}: 未找到可读取的配置目录", browser_kind.label()));
                continue;
            }

            for profile in profiles {
                let source = format!("{} / {}", profile.browser.label(), profile.profile_name);
                let cookie_header = match read_chromium_cookie_header(&profile, site_host, data_dir)
                {
                    Ok(Some(cookie_header)) => cookie_header,
                    Ok(None) => {
                        attempts.push(format!("{source}: 未找到 JavBus cookie"));
                        continue;
                    }
                    Err(error) => {
                        attempts.push(format!("{source}: {}", error));
                        continue;
                    }
                };
                let session = MetadataSession {
                    cookie_header,
                    source: source.clone(),
                    updated_at: now_string(),
                };
                let probe_url = build_metadata_search_url(site_host, "IPZZ-832");
                match http_get_text_with_session(&probe_url, Some(&session)) {
                    Ok(html) if !is_age_verification_page(&html) => return Ok(session),
                    Ok(_) => attempts.push(format!("{source}: 已读取 cookie，但站点仍返回验证页")),
                    Err(error) => attempts.push(format!("{source}: {}", error)),
                }
            }
        }

        anyhow::bail!("未找到可用的 JavBus 浏览器会话。{}", attempts.join("；"));
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (site_host, browser, data_dir);
        anyhow::bail!("当前浏览器会话导入仅支持 Windows")
    }
}

fn requested_browser_kinds(browser: Option<&str>) -> Result<Vec<BrowserKind>> {
    let requested = browser.unwrap_or("auto").trim().to_ascii_lowercase();
    match requested.as_str() {
        "" | "auto" => Ok(vec![BrowserKind::Edge, BrowserKind::Chrome]),
        "edge" => Ok(vec![BrowserKind::Edge]),
        "chrome" => Ok(vec![BrowserKind::Chrome]),
        other => anyhow::bail!("不支持的浏览器类型: {}", other),
    }
}

#[cfg(target_os = "windows")]
fn collect_chromium_profiles(browser: BrowserKind) -> Result<Vec<ChromiumProfile>> {
    let user_data_dir = browser.user_data_dir()?;
    if !user_data_dir.exists() {
        return Ok(Vec::new());
    }

    let mut profiles = fs::read_dir(&user_data_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|profile_dir| {
            let profile_name = profile_dir.file_name()?.to_string_lossy().to_string();
            if profile_name != "Default" && !profile_name.starts_with("Profile ") {
                return None;
            }
            chromium_cookie_db_path(&profile_dir).map(|_| ChromiumProfile {
                browser,
                user_data_dir: user_data_dir.clone(),
                profile_name,
                profile_dir,
            })
        })
        .collect::<Vec<_>>();
    profiles.sort_by(|left, right| {
        let left_rank = if left.profile_name == "Default" { 0 } else { 1 };
        let right_rank = if right.profile_name == "Default" {
            0
        } else {
            1
        };
        left_rank
            .cmp(&right_rank)
            .then_with(|| left.profile_name.cmp(&right.profile_name))
    });
    Ok(profiles)
}

#[cfg(target_os = "windows")]
fn chromium_cookie_db_path(profile_dir: &Path) -> Option<PathBuf> {
    let network_path = profile_dir.join("Network").join("Cookies");
    if network_path.exists() {
        return Some(network_path);
    }
    let legacy_path = profile_dir.join("Cookies");
    if legacy_path.exists() {
        return Some(legacy_path);
    }
    None
}

#[cfg(target_os = "windows")]
fn read_chromium_cookie_header(
    profile: &ChromiumProfile,
    site_host: &str,
    data_dir: &Path,
) -> Result<Option<String>> {
    let master_key = load_chromium_master_key(&profile.user_data_dir)?;
    let cookie_db_path =
        chromium_cookie_db_path(&profile.profile_dir).context("未找到 Cookies 数据库")?;
    let temp_dir = data_dir.join("tmp");
    fs::create_dir_all(&temp_dir)?;
    let temp_path = temp_dir.join(format!(
        "cookies-{}-{}.sqlite",
        stable_path_hash(&profile.profile_dir.to_string_lossy()),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    fs::copy(&cookie_db_path, &temp_path).with_context(|| {
        format!(
            "无法复制 Cookies 数据库：{}",
            cookie_db_path.to_string_lossy()
        )
    })?;

    let cookies_result = (|| -> Result<Vec<(String, String)>> {
        let db = Connection::open(&temp_path)?;
        let domain = site_host.trim_start_matches("www.").to_ascii_lowercase();
        let mut stmt = db.prepare(
            r#"
            SELECT name, value, encrypted_value, host_key
            FROM cookies
            WHERE host_key LIKE ?1
            ORDER BY length(path) DESC, expires_utc DESC
            "#,
        )?;
        let rows = stmt.query_map([format!("%{}%", domain)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        let mut cookies = Vec::new();
        let mut seen = HashSet::new();
        for row in rows {
            let (name, plain_value, encrypted_value, host_key) = row?;
            if !cookie_host_matches(&host_key, site_host) || !seen.insert(name.clone()) {
                continue;
            }
            let value = if !plain_value.is_empty() {
                plain_value
            } else {
                decrypt_chromium_cookie_value(&encrypted_value, &master_key)
                    .with_context(|| format!("无法解密 cookie {}", name))?
            };
            if value.trim().is_empty() {
                continue;
            }
            cookies.push((name, value));
        }
        Ok(cookies)
    })();

    let _ = fs::remove_file(&temp_path);
    let cookies = cookies_result?;
    if cookies.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        cookies
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; "),
    ))
}

#[cfg(target_os = "windows")]
fn cookie_host_matches(host_key: &str, site_host: &str) -> bool {
    let normalized_host = host_key.trim_start_matches('.').to_ascii_lowercase();
    let normalized_site = site_host.trim_start_matches('.').to_ascii_lowercase();
    let base_site = normalized_site.trim_start_matches("www.");
    normalized_host == normalized_site
        || normalized_host == base_site
        || normalized_host.ends_with(&format!(".{}", base_site))
}

#[cfg(target_os = "windows")]
fn load_chromium_master_key(user_data_dir: &Path) -> Result<Vec<u8>> {
    let local_state_path = user_data_dir.join("Local State");
    let local_state_raw = fs::read_to_string(&local_state_path).with_context(|| {
        format!(
            "无法读取 Local State：{}",
            local_state_path.to_string_lossy()
        )
    })?;
    let local_state: ChromiumLocalState =
        serde_json::from_str(&local_state_raw).context("Local State 不是合法 JSON")?;
    let encrypted_key = local_state
        .os_crypt
        .and_then(|os_crypt| os_crypt.encrypted_key)
        .context("Local State 缺少 encrypted_key")?;
    let decoded = BASE64
        .decode(encrypted_key)
        .context("encrypted_key 不是合法 Base64")?;
    let payload = decoded.strip_prefix(b"DPAPI").unwrap_or(&decoded);
    dpapi_unprotect(payload).context("无法解密浏览器主密钥")
}

#[cfg(target_os = "windows")]
fn decrypt_chromium_cookie_value(encrypted_value: &[u8], master_key: &[u8]) -> Result<String> {
    if encrypted_value.is_empty() {
        return Ok(String::new());
    }
    if encrypted_value.starts_with(b"v20") {
        anyhow::bail!("浏览器 cookie 使用 v20 加密，当前自动导入不可用");
    }
    if encrypted_value.starts_with(b"v10") || encrypted_value.starts_with(b"v11") {
        if encrypted_value.len() < 3 + 12 + 16 {
            anyhow::bail!("cookie 密文长度不足");
        }
        let cipher = Aes256Gcm::new_from_slice(master_key)
            .map_err(|_| anyhow::anyhow!("浏览器主密钥无效"))?;
        let nonce = Nonce::from_slice(&encrypted_value[3..15]);
        let plaintext = cipher
            .decrypt(nonce, &encrypted_value[15..])
            .map_err(|_| anyhow::anyhow!("AES-GCM 解密失败"))?;
        return String::from_utf8(plaintext).context("cookie 不是有效 UTF-8");
    }
    let plaintext = dpapi_unprotect(encrypted_value).context("DPAPI 解密失败")?;
    String::from_utf8(plaintext).context("cookie 不是有效 UTF-8")
}

#[cfg(target_os = "windows")]
fn dpapi_unprotect(input: &[u8]) -> Result<Vec<u8>> {
    unsafe {
        let mut input_blob = CRYPT_INTEGER_BLOB {
            cbData: input.len() as u32,
            pbData: input.as_ptr() as *mut u8,
        };
        let mut output_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };
        if CryptUnprotectData(
            &mut input_blob,
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
            &mut output_blob,
        ) == 0
        {
            anyhow::bail!(
                "CryptUnprotectData 调用失败：{}",
                std::io::Error::last_os_error()
            );
        }
        let bytes =
            std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec();
        if !output_blob.pbData.is_null() {
            LocalFree(output_blob.pbData as *mut c_void);
        }
        Ok(bytes)
    }
}

struct HttpResponse {
    status: u32,
    body: Vec<u8>,
}

fn http_get_text_with_session(url: &str, session: Option<&MetadataSession>) -> Result<String> {
    let response = http_get(url, session, None)?;
    if !(200..400).contains(&response.status) {
        anyhow::bail!("HTTP 状态码 {}", response.status);
    }
    Ok(String::from_utf8_lossy(&response.body).to_string())
}

fn http_get_bytes_with_session(
    url: &str,
    session: Option<&MetadataSession>,
    referer: Option<&str>,
) -> Result<Vec<u8>> {
    let response = http_get(url, session, referer)?;
    if !(200..400).contains(&response.status) {
        anyhow::bail!("HTTP 状态码 {}", response.status);
    }
    Ok(response.body)
}

#[cfg(target_os = "windows")]
fn http_get(
    url: &str,
    metadata_session: Option<&MetadataSession>,
    referer: Option<&str>,
) -> Result<HttpResponse> {
    let parsed = ParsedUrl::parse(url).ok_or_else(|| anyhow::anyhow!("URL 无效"))?;
    let port = parsed.port.unwrap_or(if parsed.scheme == "https" {
        INTERNET_DEFAULT_HTTPS_PORT
    } else {
        INTERNET_DEFAULT_HTTP_PORT
    });
    let flags = if parsed.scheme == "https" {
        WINHTTP_FLAG_SECURE
    } else {
        0
    };

    unsafe {
        let agent = to_wide("Kawa Library/0.1");
        let session = InternetHandle(WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        ));
        if session.0.is_null() {
            anyhow::bail!("无法初始化 WinHTTP：{}", std::io::Error::last_os_error());
        }
        WinHttpSetTimeouts(session.0, 8000, 8000, 8000, 12000);

        let host = to_wide(&parsed.host);
        let connect = InternetHandle(WinHttpConnect(session.0, host.as_ptr(), port, 0));
        if connect.0.is_null() {
            anyhow::bail!("无法连接站点：{}", std::io::Error::last_os_error());
        }

        let verb = to_wide("GET");
        let path = to_wide(&parsed.path);
        let request = InternetHandle(WinHttpOpenRequest(
            connect.0,
            verb.as_ptr(),
            path.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            flags,
        ));
        if request.0.is_null() {
            anyhow::bail!("无法创建请求：{}", std::io::Error::last_os_error());
        }

        let redirect_policy = WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS;
        WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_REDIRECT_POLICY,
            &redirect_policy as *const _ as *const c_void,
            std::mem::size_of_val(&redirect_policy) as u32,
        );

        let mut headers_text =
            "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8\r\n\
             Accept-Language: zh-CN,zh;q=0.9,ja;q=0.8,en;q=0.6\r\n\
             User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) KawaLibrary/0.1\r\n\
             Cookie: existmag=all; over18=1; age=verified"
                .to_string();
        if let Some(session) = metadata_session {
            if !session.cookie_header.trim().is_empty() {
                headers_text.push_str("; ");
                headers_text.push_str(session.cookie_header.trim());
            }
        }
        headers_text.push_str("\r\n");
        if let Some(referer) = referer {
            headers_text.push_str("Referer: ");
            headers_text.push_str(referer);
            headers_text.push_str("\r\n");
        }
        let headers = to_wide(&headers_text);
        WinHttpAddRequestHeaders(
            request.0,
            headers.as_ptr(),
            u32::MAX,
            WINHTTP_ADDREQ_FLAG_ADD,
        );

        if WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) == 0 {
            anyhow::bail!("请求发送失败：{}", std::io::Error::last_os_error());
        }
        if WinHttpReceiveResponse(request.0, ptr::null_mut()) == 0 {
            anyhow::bail!("响应读取失败：{}", std::io::Error::last_os_error());
        }

        let mut status = 0u32;
        let mut status_size = std::mem::size_of::<u32>() as u32;
        let mut header_index = 0u32;
        let _ = WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            &mut status as *mut _ as *mut c_void,
            &mut status_size,
            &mut header_index,
        );

        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            if WinHttpQueryDataAvailable(request.0, &mut available) == 0 {
                anyhow::bail!("读取响应长度失败：{}", std::io::Error::last_os_error());
            }
            if available == 0 {
                break;
            }
            let allowed = available.min(12_000_000u32.saturating_sub(body.len() as u32));
            if allowed == 0 {
                break;
            }
            let mut buffer = vec![0u8; allowed as usize];
            let mut read = 0u32;
            if WinHttpReadData(
                request.0,
                buffer.as_mut_ptr() as *mut c_void,
                allowed,
                &mut read,
            ) == 0
            {
                anyhow::bail!("读取响应内容失败：{}", std::io::Error::last_os_error());
            }
            if read == 0 {
                break;
            }
            buffer.truncate(read as usize);
            body.extend_from_slice(&buffer);
        }

        Ok(HttpResponse { status, body })
    }
}

#[cfg(not(target_os = "windows"))]
fn http_get(
    _url: &str,
    _metadata_session: Option<&MetadataSession>,
    _referer: Option<&str>,
) -> Result<HttpResponse> {
    anyhow::bail!("当前联网元数据实现仅支持 Windows")
}

#[cfg(target_os = "windows")]
struct InternetHandle(*mut c_void);

#[cfg(target_os = "windows")]
impl Drop for InternetHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                WinHttpCloseHandle(self.0);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn is_video_extension(extension: &str) -> bool {
    VIDEO_EXTENSIONS.contains(&extension)
}

fn format_system_time(time: SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Local> = time.into();
    datetime.format("%Y-%m-%d %H:%M").to_string()
}

fn system_time_to_unix(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn bytes_to_gb(bytes: i64) -> f64 {
    let gb = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    (gb * 100.0).round() / 100.0
}

fn paths_same_ignore_case(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[derive(Clone)]
struct DirectFolder {
    name: String,
    path: String,
    modified_unix: i64,
}

struct FolderSummaryBuilder {
    name: String,
    path: String,
    file_count: usize,
    size_bytes: i64,
    modified: String,
    modified_unix: i64,
}

impl FolderSummaryBuilder {
    fn finish(self) -> FolderSummary {
        FolderSummary {
            name: self.name,
            path: self.path,
            file_count: self.file_count,
            size_gb: bytes_to_gb(self.size_bytes),
            modified: self.modified,
            modified_unix: self.modified_unix,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_common_media_codes() {
        let regexes = build_code_regexes().unwrap();

        assert_eq!(
            extract_media_code("FC2-PPV-3308060-1.mp4", &regexes),
            Some("FC2PPV-3308060".to_string())
        );
        assert_eq!(
            extract_media_code("fc2ppv_4883198-C.mp4", &regexes),
            Some("FC2PPV-4883198".to_string())
        );
        assert_eq!(
            extract_media_code("SSNI025.resolution.mp4", &regexes),
            Some("SSNI-025".to_string())
        );
        assert_eq!(
            extract_media_code("MIKR-089-U.mp4", &regexes),
            Some("MIKR-089".to_string())
        );
    }

    #[test]
    fn ignores_files_without_codes() {
        let regexes = build_code_regexes().unwrap();
        assert_eq!(extract_media_code("readme.txt", &regexes), None);
    }

    #[test]
    fn normalizes_script_rename_cases() {
        assert_eq!(normalize_dash("SSNI025"), "SSNI-025");
        assert_eq!(normalize_dash("300MIUM1357"), "300MIUM-1357");
        assert_eq!(normalize_dash("fc2ppv4883198"), "FC2PPV-4883198");
    }

    #[test]
    fn builds_target_name_from_processing_options() {
        let options = ProcessingOptions {
            move_to_appreciation: true,
            rename_enabled: true,
            uppercase: true,
            normalize_dash: true,
            normalize_uncensored_suffix: false,
            remove_at_prefix: true,
            skip_if_exists: true,
            delete_source_folder_after_move: false,
        };
        assert_eq!(
            build_target_file_name("hhd800.com@snos224u.mp4", &options),
            "SNOS-224-U.mp4"
        );
    }

    #[test]
    fn normalizes_uncensored_processing_suffixes() {
        let options = ProcessingOptions {
            move_to_appreciation: true,
            rename_enabled: true,
            uppercase: true,
            normalize_dash: true,
            normalize_uncensored_suffix: true,
            remove_at_prefix: true,
            skip_if_exists: true,
            delete_source_folder_after_move: false,
        };
        assert_eq!(
            build_target_file_name("MIDV-119-UNCENSORED-NYAP2P.COM.mp4", &options),
            "MIDV-119-U.mp4"
        );
        assert_eq!(
            build_target_file_name("snis255-c-restored.mp4", &options),
            "SNIS-255-UC.mp4"
        );
    }

    #[test]
    fn recognizes_incomplete_extensions() {
        assert!(is_incomplete_extension("xltd"));
        assert!(is_incomplete_extension("PART"));
        assert!(!is_incomplete_extension("mp4"));
    }

    #[test]
    fn normalizes_archive_search_codes_without_suffix_letter() {
        assert_eq!(
            normalize_archive_code("CJOD-138-U.mp4").unwrap(),
            "CJOD-138"
        );
        assert_eq!(
            normalize_archive_code("MIDV-119-UNCENSORED-NYAP2P.COM.mp4").unwrap(),
            "MIDV-119"
        );
        assert_eq!(normalize_archive_code("IPZZ-832.mp4").unwrap(), "IPZZ-832");
    }

    #[test]
    fn extracts_actor_name_from_search_result_title_span() {
        let html = r#"
        <div class="movie-box">
          <span>背徳の寝取らせシアタールーム 低俗男たちの醜い肉棒で汚された貞淑妻ー。 新妻ゆうか<br />新妻ゆうか</span>
        </div>
        "#;
        assert_eq!(extract_actor_names(html), vec!["新妻ゆうか".to_string()]);
    }

    #[test]
    fn extracts_actor_name_from_star_name_class() {
        let html = r#"
        <div class="star-name">佐々木さき</div>
        "#;
        assert_eq!(extract_actor_names(html), vec!["佐々木さき".to_string()]);
    }

    #[test]
    fn extracts_searchstar_avatar_url_from_page() {
        let html = r#"<img src="/pics/actress/2jv_a.jpg" title="波多野結衣">"#;
        assert_eq!(
            extract_searchstar_avatar_url(
                html,
                "https://www.javbus.com/searchstar/%E6%B3%A2%E5%A4%9A%E9%87%8E%E7%B5%90%E8%A1%A3"
            ),
            Some("https://www.javbus.com/pics/actress/2jv_a.jpg".to_string())
        );
    }

    #[test]
    fn finds_only_real_detail_page_url() {
        let html = r#"
        <a href="javascript:searchs(obj)">bad</a>
        <a href="/search/SIRO-5664">bad-search</a>
        <a href="/SIRO-5664">good</a>
        "#;
        assert_eq!(
            extract_detail_url(html, "https://www.javbus.com/search/SIRO-5664", "SIRO-5664"),
            Some("https://www.javbus.com/SIRO-5664".to_string())
        );
    }

    #[test]
    fn ignores_script_noise_when_extracting_actor_names() {
        let html = r#"
        <script>
          function searchs(obj){ return 0; }
        </script>
        <div class="star-name">佐々木さき</div>
        "#;
        assert_eq!(extract_actor_names(html), vec!["佐々木さき".to_string()]);
    }
    #[test]
    fn prefers_detail_page_actor_names_when_merging() {
        assert_eq!(
            merge_actor_names(
                vec!["Sasaki".to_string()],
                vec!["0".to_string(), "Sasaki".to_string(), "Alias".to_string()]
            ),
            vec!["Sasaki".to_string(), "0".to_string(), "Alias".to_string()]
        );
    }
}
