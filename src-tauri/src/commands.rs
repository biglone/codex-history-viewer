use crate::db::{
    extract_id_from_filename, find_matching_snippet, get_codex_dir, open_conn, parse_rollout_file,
    query_sessions, Message, SearchResult, Session,
};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[tauri::command]
pub fn get_sessions(page: usize, page_size: usize) -> Result<Vec<Session>, String> {
    let conn = open_conn()?;
    let offset = page * page_size;
    query_sessions(
        &conn,
        "SELECT id, title, cwd, first_user_message, created_at_ms, updated_at_ms,
                model, model_provider, archived, rollout_path, preview
         FROM threads
         ORDER BY created_at_ms DESC
         LIMIT ?1 OFFSET ?2",
        &[&(page_size as i64), &(offset as i64)],
    )
}

#[tauri::command]
pub fn get_session_count() -> Result<i64, String> {
    let conn = open_conn()?;
    conn.query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_session_messages(session_id: String) -> Result<Vec<Message>, String> {
    let conn = open_conn()?;
    let rollout_path: String = conn
        .query_row(
            "SELECT rollout_path FROM threads WHERE id = ?1",
            [&session_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    parse_rollout_file(&rollout_path)
}

#[tauri::command]
pub fn search_sessions(query: String) -> Result<Vec<SearchResult>, String> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }

    let conn = open_conn()?;
    let query_lower = query.to_lowercase();
    let like_query = format!("%{}%", &query);

    let db_sessions = query_sessions(
        &conn,
        "SELECT id, title, cwd, first_user_message, created_at_ms, updated_at_ms,
                model, model_provider, archived, rollout_path, preview
         FROM threads
         WHERE LOWER(title) LIKE LOWER(?1)
            OR LOWER(first_user_message) LIKE LOWER(?1)
            OR LOWER(cwd) LIKE LOWER(?1)
            OR LOWER(preview) LIKE LOWER(?1)
         ORDER BY created_at_ms DESC
         LIMIT 100",
        &[&like_query as &dyn rusqlite::ToSql],
    )?;

    let mut results: Vec<SearchResult> = db_sessions
        .into_iter()
        .map(|s| {
            let matched = if s.title.to_lowercase().contains(&query_lower) {
                s.title.clone()
            } else {
                s.first_user_message.chars().take(200).collect()
            };
            SearchResult {
                session: s,
                matched_message: matched,
                context: String::new(),
            }
        })
        .collect();

    let codex_dir = get_codex_dir();
    let sessions_dir = codex_dir.join("sessions");
    let archived_dir = codex_dir.join("archived_sessions");

    let mut seen_ids: std::collections::HashSet<String> =
        results.iter().map(|r| r.session.id.clone()).collect();

    for dir in &[sessions_dir, archived_dir] {
        if !dir.exists() {
            continue;
        }
        for entry in WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "jsonl"))
        {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                if content.to_lowercase().contains(&query_lower) {
                    let filename = entry.file_name().to_string_lossy().to_string();
                    if let Some(id) = extract_id_from_filename(&filename) {
                        if !seen_ids.contains(&id) {
                            seen_ids.insert(id.clone());
                            if let Ok(sessions) = query_sessions(
                                &conn,
                                "SELECT id, title, cwd, first_user_message, created_at_ms, updated_at_ms,
                                        model, model_provider, archived, rollout_path, preview
                                 FROM threads WHERE id = ?1",
                                &[&id as &dyn rusqlite::ToSql],
                            ) {
                                if let Some(session) = sessions.into_iter().next() {
                                    let matched = find_matching_snippet(&content, &query_lower);
                                    results.push(SearchResult {
                                        session,
                                        matched_message: matched,
                                        context: String::new(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    results.sort_by(|a, b| b.session.updated_at_ms.cmp(&a.session.updated_at_ms));
    Ok(results)
}

#[tauri::command]
pub fn get_projects() -> Result<Vec<String>, String> {
    let conn = open_conn()?;
    let mut stmt = conn
        .prepare("SELECT DISTINCT cwd FROM threads ORDER BY cwd")
        .map_err(|e| e.to_string())?;
    let cwds: rusqlite::Result<Vec<String>> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect();
    cwds.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_sessions_by_project(
    cwd: String,
    page: usize,
    page_size: usize,
) -> Result<Vec<Session>, String> {
    let conn = open_conn()?;
    let offset = page * page_size;
    query_sessions(
        &conn,
        "SELECT id, title, cwd, first_user_message, created_at_ms, updated_at_ms,
                model, model_provider, archived, rollout_path, preview
         FROM threads WHERE cwd = ?1
         ORDER BY created_at_ms DESC
         LIMIT ?2 OFFSET ?3",
        &[&cwd as &dyn rusqlite::ToSql, &(page_size as i64), &(offset as i64)],
    )
}

// ===================================
// Sync Commands
// ===================================

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct SyncConfig {
    pub server_url: String,
    pub api_token: String,
    pub device_id: String,
    pub device_name: String,
    pub last_sync_ms: i64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            api_token: String::new(),
            device_id: generate_device_id(),
            device_name: get_hostname(),
            last_sync_ms: 0,
        }
    }
}

fn sync_config_path() -> std::path::PathBuf {
    crate::db::get_codex_dir().join("codex_sync_config.json")
}

fn generate_device_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let hex = format!("{:032x}", t % u128::MAX);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32]
    )
}

fn get_hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::env::var("NAME"))
        .unwrap_or_else(|_| "Unknown Device".to_string())
}

/// 读取本地同步配置
#[tauri::command]
pub fn get_sync_config() -> Result<SyncConfig, String> {
    let path = sync_config_path();
    if !path.exists() {
        return Ok(SyncConfig::default());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

/// 保存同步配置到本地
#[tauri::command]
pub fn save_sync_config(config: SyncConfig) -> Result<(), String> {
    let path = sync_config_path();
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

/// 获取所有本地 session 的 id + updated_at_ms（用于增量同步对比）
#[tauri::command]
pub fn get_local_session_ids() -> Result<Vec<serde_json::Value>, String> {
    let conn = open_conn()?;
    let mut stmt = conn
        .prepare("SELECT id, updated_at_ms FROM threads ORDER BY updated_at_ms DESC")
        .map_err(|e| e.to_string())?;
    let rows: rusqlite::Result<Vec<serde_json::Value>> = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let updated: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
            Ok(serde_json::json!({ "id": id, "updated_at_ms": updated }))
        })
        .map_err(|e| e.to_string())?
        .collect();
    rows.map_err(|e| e.to_string())
}

/// 本地项目路径映射条目（供跨设备 cwd 自动匹配使用）
#[derive(Debug, serde::Serialize)]
pub struct LocalProjectPath {
    pub project_name: String, // cwd 最后一段目录名，如 "MyProject"
    pub local_cwd: String,    // 完整本地路径，如 "C:\\Users\\win\\work\\MyProject"
}

/// 查询本地所有不重复的 cwd，提取项目名，供前端做跨设备路径匹配
/// 同名取最近使用的那条（ORDER BY updated_at_ms DESC）
#[tauri::command]
pub fn get_local_project_paths() -> Result<Vec<LocalProjectPath>, String> {
    let conn = open_conn()?;
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT cwd FROM threads \
             WHERE cwd IS NOT NULL AND cwd != '' \
             ORDER BY updated_at_ms DESC",
        )
        .map_err(|e| e.to_string())?;

    let cwds: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    for cwd in cwds {
        let project_name = std::path::Path::new(&cwd)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if project_name.is_empty() {
            continue;
        }
        // 同名只保留第一条（即最近使用的）
        if seen.insert(project_name.clone()) {
            result.push(LocalProjectPath { project_name, local_cwd: cwd });
        }
    }

    Ok(result)
}

/// 获取单条 session 完整数据（元数据 + 消息），用于上传到服务端
#[derive(Debug, serde::Serialize)]
pub struct SessionUploadPayload {
    pub session: crate::db::Session,
    pub messages: Vec<crate::db::Message>,
}

#[tauri::command]
pub fn get_session_for_upload(session_id: String) -> Result<SessionUploadPayload, String> {
    let conn = open_conn()?;

    let sessions = crate::db::query_sessions(
        &conn,
        "SELECT id, title, cwd, first_user_message, created_at_ms, updated_at_ms,
                model, model_provider, archived, rollout_path, preview
         FROM threads WHERE id = ?1",
        &[&session_id],
    )?;

    let session = sessions
        .into_iter()
        .next()
        .ok_or_else(|| format!("Session not found: {}", session_id))?;

    let messages = if session.rollout_path.is_empty() {
        vec![]
    } else {
        crate::db::parse_rollout_file(&session.rollout_path).unwrap_or_default()
    };

    Ok(SessionUploadPayload { session, messages })
}

/// 批量获取多条 session 数据供上传（最多 50 条）
#[tauri::command]
pub fn get_sessions_for_upload(
    session_ids: Vec<String>,
) -> Result<Vec<SessionUploadPayload>, String> {
    if session_ids.len() > 50 {
        return Err("Max 50 sessions per batch".to_string());
    }
    let mut result = Vec::new();
    for id in session_ids {
        match get_session_for_upload(id) {
            Ok(payload) => result.push(payload),
            Err(e) => eprintln!("[sync] skip session error: {}", e),
        }
    }
    Ok(result)
}

// ===================================
// Import (Download) Commands
// ===================================

/// 从服务端下载的单条 session 数据结构（前端透传）
#[derive(Debug, serde::Deserialize)]
pub struct ImportSession {
    /// 服务端 session 元数据
    pub id: String,
    pub device_id: String,
    pub device_name: Option<String>,
    pub title: Option<String>,
    /// 原始上传时的 cwd（保留源设备路径，不做替换）
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub first_user_message: Option<String>,
    pub preview: Option<String>,
    pub created_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub archived: Option<bool>,
    pub message_count: Option<i64>,
    /// 对应的消息列表（来自 /api/sessions/:id/messages）
    pub messages: Vec<crate::db::Message>,
}

/// 单条导入结果
#[derive(Debug, serde::Serialize)]
pub struct ImportResult {
    pub id: String,
    pub ok: bool,
    pub skipped: bool, // true = 本地已有更新版本，跳过
    pub error: Option<String>,
}

/// 将服务端下载的会话批量写入本地 SQLite + 磁盘
///
/// # 路径策略
/// - `cwd` 保留服务端原始值（即来源设备的项目路径），不做替换。
///   这样跨设备来的历史会话在"按项目浏览"中以来源路径显示，内容完整可查。
/// - 消息 JSONL 文件保存到 `~/.codex/synced/<session_id>.jsonl`，
///   与本地任何项目路径解耦，避免路径不存在导致读取失败。
#[tauri::command]
pub fn import_sessions(sessions: Vec<ImportSession>) -> Result<Vec<ImportResult>, String> {
    let conn = open_conn()?;

    // 确保 synced 目录存在
    let synced_dir = get_codex_dir().join("synced");
    std::fs::create_dir_all(&synced_dir).map_err(|e| e.to_string())?;

    let mut results = Vec::new();

    for s in sessions {
        let result = import_one(&conn, &synced_dir, s);
        results.push(result);
    }

    Ok(results)
}

fn import_one(
    conn: &rusqlite::Connection,
    synced_dir: &std::path::Path,
    s: ImportSession,
) -> ImportResult {
    // 1. 检查本地是否已有更新版本（updated_at_ms 更大则跳过）
    let local_updated: Option<i64> = conn
        .query_row(
            "SELECT updated_at_ms FROM threads WHERE id = ?1",
            [&s.id],
            |row| row.get(0),
        )
        .ok();

    let incoming_updated = s.updated_at_ms.unwrap_or(0);
    if let Some(local_ms) = local_updated {
        if local_ms >= incoming_updated {
            return ImportResult {
                id: s.id,
                ok: true,
                skipped: true,
                error: None,
            };
        }
    }

    // 2. 将消息写入 ~/.codex/synced/<session_id>.jsonl
    //    路径与本地 cwd 完全解耦，避免不同设备路径不同的问题
    let jsonl_path = synced_dir.join(format!("{}.jsonl", s.id));
    let rollout_path_str = jsonl_path.to_string_lossy().to_string();

    if !s.messages.is_empty() {
        // parse_rollout_file 期望 Codex 原始格式：
        //   {"type":"response_item","timestamp":"...","payload":{"role":"user","content":[{"type":"output_text","text":"..."}]}}
        // 直接序列化 Message 结构体得到的是简化格式 {role, content, timestamp}，
        // parse_rollout_file 找不到 type=="response_item" 会返回空列表。
        // 必须在写文件时转换为标准格式。
        let content: String = s
            .messages
            .iter()
            .map(|m| {
                let line = serde_json::json!({
                    "type": "response_item",
                    "timestamp": m.timestamp,
                    "payload": {
                        "role": m.role,
                        "content": [{
                            "type": "output_text",
                            "text": m.content
                        }]
                    }
                });
                serde_json::to_string(&line).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join("\n");
        if let Err(e) = std::fs::write(&jsonl_path, content) {
            return ImportResult {
                id: s.id,
                ok: false,
                skipped: false,
                error: Some(format!("写入消息文件失败: {}", e)),
            };
        }
    }

    // 3. Upsert 到本地 threads 表（适配真实 schema）
    //    cwd 保留来源设备原始路径，不做替换
    //    thread_source 存 "synced:<device_name>@<device_id>" 标识跨设备来源
    let thread_source = format!(
        "synced:{}@{}",
        s.device_name.as_deref().unwrap_or("unknown"),
        s.device_id
    );
    let created_at_ms = s.created_at_ms.unwrap_or(0);
    // threads 表的 created_at / updated_at 是秒级 INTEGER（codex 原始字段）
    let created_at_sec = created_at_ms / 1000;
    let updated_at_sec = incoming_updated / 1000;

    let upsert_result = conn.execute(
        "INSERT INTO threads (
            id, rollout_path, created_at, updated_at, source,
            model_provider, cwd, title, sandbox_policy, approval_mode,
            first_user_message, preview,
            created_at_ms, updated_at_ms,
            model, archived, thread_source
         ) VALUES (?1,?2,?3,?4,'remote',?5,?6,?7,'none','suggest',?8,?9,?10,?11,?12,?13,?14)
         ON CONFLICT(id) DO UPDATE SET
            title              = excluded.title,
            cwd                = excluded.cwd,
            first_user_message = excluded.first_user_message,
            preview            = excluded.preview,
            updated_at         = excluded.updated_at,
            updated_at_ms      = excluded.updated_at_ms,
            model              = excluded.model,
            model_provider     = excluded.model_provider,
            archived           = excluded.archived,
            rollout_path       = excluded.rollout_path,
            thread_source      = excluded.thread_source
         WHERE excluded.updated_at_ms > threads.updated_at_ms",
        rusqlite::params![
            s.id,
            // rollout_path: 无消息时置空，有消息时指向 synced 目录
            if s.messages.is_empty() { String::new() } else { rollout_path_str },
            created_at_sec,
            updated_at_sec,
            s.model_provider.as_deref().unwrap_or(""),   // model_provider
            s.cwd.as_deref().unwrap_or(""),               // cwd 保留原始路径
            s.title.as_deref().unwrap_or("(无标题)"),
            s.first_user_message.as_deref().unwrap_or(""),
            s.preview.as_deref().unwrap_or(""),
            created_at_ms,
            incoming_updated,
            s.model.as_deref().unwrap_or(""),
            s.archived.unwrap_or(false) as i32,
            thread_source,                                // 来源标识
        ],
    );

    match upsert_result {
        Ok(_) => ImportResult {
            id: s.id,
            ok: true,
            skipped: false,
            error: None,
        },
        Err(e) => ImportResult {
            id: s.id,
            ok: false,
            skipped: false,
            error: Some(e.to_string()),
        },
    }
}

// ===================================
// Agy Import Commands
// ===================================

#[derive(Debug, serde::Serialize, Clone)]
pub struct AgySessionPreview {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub message_count: usize,
    pub source_file: String,
}

#[derive(Debug, serde::Serialize)]
pub struct AgyImportPreview {
    pub source_root: String,
    pub scanned_files: usize,
    pub candidate_count: usize,
    pub default_paths: Vec<String>,
    pub sessions: Vec<AgySessionPreview>,
    pub warnings: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct AgyImportSummary {
    pub scanned_files: usize,
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
    pub results: Vec<ImportResult>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct AgyParsedSession {
    id: String,
    title: String,
    cwd: String,
    model: Option<String>,
    model_provider: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
    source_file: PathBuf,
    messages: Vec<Message>,
}

#[tauri::command]
pub fn preview_agy_import(source_path: Option<String>) -> Result<AgyImportPreview, String> {
    let (root, default_paths) = resolve_agy_import_root(source_path)?;
    let scan = scan_agy_sessions(&root)?;

    let sessions = scan
        .sessions
        .into_iter()
        .take(100)
        .map(|s| AgySessionPreview {
            id: s.id,
            title: s.title,
            cwd: s.cwd,
            created_at_ms: s.created_at_ms,
            updated_at_ms: s.updated_at_ms,
            message_count: s.messages.len(),
            source_file: s.source_file.to_string_lossy().to_string(),
        })
        .collect::<Vec<_>>();

    Ok(AgyImportPreview {
        source_root: root.to_string_lossy().to_string(),
        scanned_files: scan.scanned_files,
        candidate_count: scan.candidate_count,
        default_paths,
        sessions,
        warnings: scan.warnings,
    })
}

#[tauri::command]
pub fn import_agy_sessions(source_path: Option<String>) -> Result<AgyImportSummary, String> {
    let (root, _) = resolve_agy_import_root(source_path)?;
    let scan = scan_agy_sessions(&root)?;
    let conn = open_conn()?;

    let imported_dir = get_codex_dir().join("agy_imported");
    std::fs::create_dir_all(&imported_dir).map_err(|e| e.to_string())?;

    let mut imported = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let mut results = Vec::new();

    for parsed in scan.sessions {
        let import_session = ImportSession {
            id: parsed.id,
            device_id: "agy-local".to_string(),
            device_name: Some("Agy Import".to_string()),
            title: Some(parsed.title),
            cwd: Some(parsed.cwd),
            model: parsed.model,
            model_provider: parsed.model_provider.or_else(|| Some("Agy".to_string())),
            first_user_message: first_message_by_role(&parsed.messages, "user"),
            preview: first_message_by_role(&parsed.messages, "assistant"),
            created_at_ms: Some(parsed.created_at_ms),
            updated_at_ms: Some(parsed.updated_at_ms),
            archived: Some(false),
            message_count: Some(parsed.messages.len() as i64),
            messages: parsed.messages,
        };

        let result = import_one(&conn, &imported_dir, import_session);
        if result.ok && result.skipped {
            skipped += 1;
        } else if result.ok {
            imported += 1;
        } else {
            failed += 1;
        }
        results.push(result);
    }

    Ok(AgyImportSummary {
        scanned_files: scan.scanned_files,
        imported,
        skipped,
        failed,
        results,
        warnings: scan.warnings,
    })
}

struct AgyScanResult {
    scanned_files: usize,
    candidate_count: usize,
    sessions: Vec<AgyParsedSession>,
    warnings: Vec<String>,
}

fn resolve_agy_import_root(
    source_path: Option<String>,
) -> Result<(PathBuf, Vec<String>), String> {
    let defaults = default_agy_paths();
    if let Some(raw) = source_path {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let expanded = expand_home_path(trimmed);
            if expanded.exists() {
                return Ok((expanded, defaults));
            }
            return Err(format!("路径不存在: {}", expanded.to_string_lossy()));
        }
    }

    for path in &defaults {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok((p, defaults));
        }
    }

    Err("未找到默认 Agy 历史目录，请手动填写 Agy 导出的 JSON/JSONL/TXT 文件或目录路径".to_string())
}

fn default_agy_paths() -> Vec<String> {
    let home = dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mut paths = Vec::new();
    if let Ok(env_home) = std::env::var("AGY_HOME") {
        if !env_home.trim().is_empty() {
            paths.push(expand_home_path(&env_home).to_string_lossy().to_string());
        }
    }
    for rel in [
        ".agy",
        ".agy/sessions",
        ".agy/conversations",
        ".config/agy",
        ".config/agy/sessions",
        ".local/share/agy",
        ".local/share/agy/sessions",
        ".cache/agy",
        ".gemini/conversations",
        ".gemini/sessions",
        ".gemini/history",
    ] {
        paths.push(home.join(rel).to_string_lossy().to_string());
    }
    paths.sort();
    paths.dedup();
    paths
}

fn expand_home_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return dirs_next::home_dir().unwrap_or_else(|| PathBuf::from(raw));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return dirs_next::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(rest);
    }
    PathBuf::from(raw)
}

fn scan_agy_sessions(root: &Path) -> Result<AgyScanResult, String> {
    let mut scanned_files = 0;
    let mut sessions = Vec::new();
    let mut warnings = Vec::new();

    let files = collect_agy_candidate_files(root)?;
    for path in files {
        scanned_files += 1;
        match parse_agy_file(&path) {
            Ok(mut parsed) => sessions.append(&mut parsed),
            Err(e) => {
                if warnings.len() < 20 {
                    warnings.push(format!("跳过 {}: {}", path.to_string_lossy(), e));
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    let candidate_count = sessions.len();
    Ok(AgyScanResult {
        scanned_files,
        candidate_count,
        sessions,
        warnings,
    })
}

fn collect_agy_candidate_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    if root.is_file() {
        return if is_agy_candidate_file(root) {
            Ok(vec![root.to_path_buf()])
        } else {
            Err("文件类型不支持，请选择 .json/.jsonl/.ndjson/.txt/.md".to_string())
        };
    }

    if !root.is_dir() {
        return Err(format!("不是文件或目录: {}", root.to_string_lossy()));
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .max_depth(8)
        .into_iter()
        .filter_entry(|entry| !should_skip_agy_entry(entry.path()))
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && is_agy_candidate_file(path) {
            if file_size_ok(path) {
                files.push(path.to_path_buf());
            }
        }
        if files.len() >= 1000 {
            break;
        }
    }
    Ok(files)
}

fn should_skip_agy_entry(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_lowercase()) else {
        return false;
    };
    matches!(
        name.as_str(),
        ".git" | "node_modules" | "target" | "dist" | "build" | "venv" | ".venv"
            | "runtime" | "skills"
    )
}

fn is_agy_candidate_file(path: &Path) -> bool {
    let Some(ext) = path.extension().map(|e| e.to_string_lossy().to_lowercase()) else {
        return false;
    };
    matches!(ext.as_str(), "json" | "jsonl" | "ndjson" | "txt" | "md")
}

fn file_size_ok(path: &Path) -> bool {
    path.metadata()
        .map(|m| m.len() <= 20 * 1024 * 1024)
        .unwrap_or(false)
}

fn parse_agy_file(path: &Path) -> Result<Vec<AgyParsedSession>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Err("空文件".to_string());
    }

    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let mut sessions = if ext == "jsonl" || ext == "ndjson" {
        parse_agy_jsonl(path, &raw)?
    } else if ext == "json" {
        parse_agy_json(path, &raw)?
    } else {
        parse_agy_text(path, &raw)?
    };

    sessions.retain(|s| {
        s.messages
            .iter()
            .any(|m| m.role == "user" || m.role == "assistant")
    });
    if sessions.is_empty() {
        return Err("未识别到 user/assistant 消息".to_string());
    }
    Ok(sessions)
}

fn parse_agy_json(path: &Path, raw: &str) -> Result<Vec<AgyParsedSession>, String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let mut sessions = Vec::new();
    extract_sessions_from_json(path, &value, &mut sessions);
    Ok(sessions)
}

fn parse_agy_jsonl(path: &Path, raw: &str) -> Result<Vec<AgyParsedSession>, String> {
    let mut meta = serde_json::Map::new();
    let mut messages = Vec::new();
    let mut embedded_sessions = Vec::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        if value.get("type").and_then(|v| v.as_str()) == Some("session_meta") {
            if let Some(obj) = value.get("payload").and_then(|v| v.as_object()) {
                for (k, v) in obj {
                    meta.insert(k.clone(), v.clone());
                }
            }
            continue;
        }

        let before = embedded_sessions.len();
        extract_sessions_from_json(path, &value, &mut embedded_sessions);
        if embedded_sessions.len() > before {
            continue;
        }

        if let Some(msg) = message_from_json(&value) {
            messages.push(msg);
        }
    }

    if !messages.is_empty() {
        let meta_value = serde_json::Value::Object(meta);
        embedded_sessions.push(build_agy_session(path, &meta_value, messages, 0));
    }

    Ok(embedded_sessions)
}

fn parse_agy_text(path: &Path, raw: &str) -> Result<Vec<AgyParsedSession>, String> {
    let mut messages = Vec::new();
    let mut current_role = String::new();
    let mut current = String::new();

    for line in raw.lines() {
        if let Some((role, content)) = split_text_role_line(line) {
            if !current_role.is_empty() && !current.trim().is_empty() {
                messages.push(Message {
                    role: current_role.clone(),
                    content: current.trim().to_string(),
                    timestamp: String::new(),
                });
            }
            current_role = role.to_string();
            current = content.trim().to_string();
        } else if !current_role.is_empty() {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }

    if !current_role.is_empty() && !current.trim().is_empty() {
        messages.push(Message {
            role: current_role,
            content: current.trim().to_string(),
            timestamp: String::new(),
        });
    }

    if messages.is_empty() {
        return Err("文本文件未找到 User/Assistant 前缀".to_string());
    }

    Ok(vec![build_agy_session(
        path,
        &serde_json::Value::Object(serde_json::Map::new()),
        messages,
        0,
    )])
}

fn split_text_role_line(line: &str) -> Option<(&'static str, &str)> {
    let trimmed = line.trim_start();
    for prefix in ["User:", "Human:", "用户:", "你:", "Me:"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some(("user", rest));
        }
    }
    for prefix in ["Assistant:", "AI:", "Agy:", "Agent:", "助手:", "回答:"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some(("assistant", rest));
        }
    }
    None
}

fn extract_sessions_from_json(
    path: &Path,
    value: &serde_json::Value,
    out: &mut Vec<AgyParsedSession>,
) {
    if let Some(messages) = messages_from_container(value) {
        let idx = out.len();
        out.push(build_agy_session(path, value, messages, idx));
        return;
    }

    if let Some(arr) = value.as_array() {
        let messages = arr.iter().filter_map(message_from_json).collect::<Vec<_>>();
        if !messages.is_empty() {
            let idx = out.len();
            out.push(build_agy_session(path, value, messages, idx));
            return;
        }
        for item in arr {
            extract_sessions_from_json(path, item, out);
        }
        return;
    }

    if let Some(obj) = value.as_object() {
        for key in ["sessions", "conversations", "chats", "threads", "items"] {
            if let Some(arr) = obj.get(key).and_then(|v| v.as_array()) {
                for item in arr {
                    extract_sessions_from_json(path, item, out);
                }
            }
        }
    }
}

fn messages_from_container(value: &serde_json::Value) -> Option<Vec<Message>> {
    let obj = value.as_object()?;
    for key in ["messages", "history", "conversation", "transcript", "turns"] {
        if let Some(arr) = obj.get(key).and_then(|v| v.as_array()) {
            let messages = arr.iter().filter_map(message_from_json).collect::<Vec<_>>();
            if !messages.is_empty() {
                return Some(messages);
            }
        }
    }
    None
}

fn message_from_json(value: &serde_json::Value) -> Option<Message> {
    let obj = value.as_object()?;

    if value.get("type").and_then(|v| v.as_str()) == Some("response_item") {
        if let Some(payload) = value.get("payload") {
            return message_from_json(payload).map(|mut m| {
                if m.timestamp.is_empty() {
                    m.timestamp = json_string(value, &["timestamp"]).unwrap_or_default();
                }
                m
            });
        }
    }

    if value.get("type").and_then(|v| v.as_str()) == Some("event_msg") {
        if let Some(payload) = value.get("payload") {
            if payload.get("type").and_then(|v| v.as_str()) == Some("user_message") {
                let content = json_string(payload, &["message", "text", "content"])?;
                return Some(Message {
                    role: "user".to_string(),
                    content,
                    timestamp: json_string(value, &["timestamp"]).unwrap_or_default(),
                });
            }
        }
    }

    let raw_role = json_string(value, &["role", "author", "sender", "speaker", "type"])?;
    let role = normalize_role(&raw_role)?;
    let content = json_content(value)?;
    if content.trim().is_empty() {
        return None;
    }
    Some(Message {
        role,
        content,
        timestamp: json_string(value, &["timestamp", "created_at", "time", "date"])
            .unwrap_or_default(),
    })
}

fn normalize_role(raw: &str) -> Option<String> {
    let r = raw.trim().to_lowercase();
    if matches!(
        r.as_str(),
        "user" | "human" | "me" | "client" | "prompt" | "request"
    ) {
        return Some("user".to_string());
    }
    if matches!(
        r.as_str(),
        "assistant" | "ai" | "agent" | "agy" | "model" | "bot" | "response"
    ) {
        return Some("assistant".to_string());
    }
    None
}

fn json_content(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = json_string(
        value,
        &[
            "content", "text", "message", "markdown", "output", "response",
        ],
    ) {
        return Some(s);
    }

    for key in ["parts", "content", "chunks"] {
        if let Some(arr) = value.get(key).and_then(|v| v.as_array()) {
            let parts = arr.iter().filter_map(json_content_part).collect::<Vec<_>>();
            if !parts.is_empty() {
                return Some(parts.join("\n"));
            }
        }
    }
    None
}

fn json_content_part(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    if value.is_object() {
        return json_string(value, &["text", "content", "message", "output"]);
    }
    None
}

fn json_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let obj = value.as_object()?;
    for key in keys {
        if let Some(v) = obj.get(*key) {
            if let Some(s) = v.as_str() {
                return Some(s.to_string());
            }
            if v.is_number() || v.is_boolean() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn build_agy_session(
    path: &Path,
    meta: &serde_json::Value,
    messages: Vec<Message>,
    index: usize,
) -> AgyParsedSession {
    let file_ms = file_modified_ms(path);
    let created_at_ms = json_i64(meta, &["created_at_ms", "createdAtMs", "start_time_ms"])
        .or_else(|| first_numeric_timestamp_ms(&messages))
        .unwrap_or(file_ms);
    let updated_at_ms = json_i64(meta, &["updated_at_ms", "updatedAtMs", "last_modified_ms"])
        .or_else(|| last_numeric_timestamp_ms(&messages))
        .unwrap_or(created_at_ms.max(file_ms));
    let cwd = json_string(
        meta,
        &[
            "cwd",
            "workspace",
            "workspace_path",
            "project_path",
            "root_dir",
        ],
    )
    .unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from(""))
            .to_string_lossy()
            .to_string()
    });
    let title = json_string(meta, &["title", "name", "summary"])
        .or_else(|| first_message_by_role(&messages, "user"))
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Agy 导入会话".to_string())
        });
    let model = json_string(meta, &["model", "model_name"]);
    let model_provider =
        json_string(meta, &["model_provider", "provider"]).or_else(|| Some("Agy".to_string()));
    let id_seed = format!(
        "{}:{}:{}:{}:{}",
        path.to_string_lossy(),
        index,
        created_at_ms,
        updated_at_ms,
        messages
            .first()
            .map(|m| m.content.as_str())
            .unwrap_or_default()
    );

    AgyParsedSession {
        id: stable_uuid(&id_seed),
        title: title.chars().take(180).collect(),
        cwd,
        model,
        model_provider,
        created_at_ms,
        updated_at_ms,
        source_file: path.to_path_buf(),
        messages,
    }
}

fn json_i64(value: &serde_json::Value, keys: &[&str]) -> Option<i64> {
    let obj = value.as_object()?;
    for key in keys {
        if let Some(v) = obj.get(*key) {
            if let Some(n) = v.as_i64() {
                return Some(normalize_epoch_ms(n));
            }
            if let Some(s) = v.as_str() {
                if let Ok(n) = s.parse::<i64>() {
                    return Some(normalize_epoch_ms(n));
                }
            }
        }
    }
    None
}

fn normalize_epoch_ms(value: i64) -> i64 {
    if value > 0 && value < 10_000_000_000 {
        value * 1000
    } else {
        value
    }
}

fn first_numeric_timestamp_ms(messages: &[Message]) -> Option<i64> {
    messages
        .iter()
        .filter_map(|m| m.timestamp.parse::<i64>().ok())
        .map(normalize_epoch_ms)
        .next()
}

fn last_numeric_timestamp_ms(messages: &[Message]) -> Option<i64> {
    messages
        .iter()
        .rev()
        .filter_map(|m| m.timestamp.parse::<i64>().ok())
        .map(normalize_epoch_ms)
        .next()
}

fn file_modified_ms(path: &Path) -> i64 {
    path.metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or_else(now_ms)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn first_message_by_role(messages: &[Message], role: &str) -> Option<String> {
    messages
        .iter()
        .find(|m| m.role == role && !m.content.trim().is_empty())
        .map(|m| m.content.chars().take(300).collect())
}

fn stable_uuid(seed: &str) -> String {
    let mut h1 = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut h1);
    let a = h1.finish();

    let mut h2 = std::collections::hash_map::DefaultHasher::new();
    "agy-import".hash(&mut h2);
    seed.chars().rev().collect::<String>().hash(&mut h2);
    let b = h2.finish();

    let hex = format!("{:016x}{:016x}", a, b);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

// ===================================
// Automations Sync Commands
// ===================================

/// 本地单个 automation 文件的信息
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct AutomationFile {
    /// 文件名（不含路径），例如 "daily-report.json"
    pub name: String,
    /// 文件内容（UTF-8 文本）
    pub content: String,
    /// 文件最后修改时间（Unix ms，用于增量对比）
    pub updated_at_ms: i64,
    /// 实际扫描目录路径（仅第一条携带，供前端诊断用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir_path: Option<String>,
}

/// 获取本地 ~/.codex/automations/ 目录下的所有条目
/// 支持两种格式：
///   1. 直接文件（如 task.json）
///   2. 子目录（如 my-task/），将子目录内所有文件打包为 JSON object
#[tauri::command]
pub fn get_local_automations() -> Result<Vec<AutomationFile>, String> {
    let automations_dir = crate::db::get_codex_dir().join("automations");
    let dir_path_str = automations_dir.to_string_lossy().to_string();

    // 目录不存在时返回携带路径的哨兵条目，供前端日志诊断
    if !automations_dir.exists() {
        return Ok(vec![AutomationFile {
            name: String::new(),
            content: String::new(),
            updated_at_ms: 0,
            dir_path: Some(dir_path_str),
        }]);
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let mut files = Vec::new();
    let mut first = true;

    for entry in std::fs::read_dir(&automations_dir).map_err(|e| e.to_string())? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if name.is_empty() || name.starts_with('.') {
            continue;
        }

        let (content, updated_at_ms) = if path.is_file() {
            // ── 模式 1：直接文件 ──────────────────────────────────────────────
            let c = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let raw_ms = path
                .metadata().ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            (c, if raw_ms > 0 { raw_ms } else { now_ms })
        } else if path.is_dir() {
            // ── 模式 2：子目录，将内部文件打包为 JSON object ──────────────────
            let mut sub_map = std::collections::BTreeMap::new();
            let mut dir_max_ms: i64 = 0;

            if let Ok(sub_entries) = std::fs::read_dir(&path) {
                for sub_entry in sub_entries.flatten() {
                    let sp = sub_entry.path();
                    if !sp.is_file() { continue; }
                    let sname = sp.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if sname.is_empty() || sname.starts_with('.') { continue; }
                    if let Ok(sc) = std::fs::read_to_string(&sp) {
                        let ms = sp.metadata().ok()
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0);
                        if ms > dir_max_ms { dir_max_ms = ms; }
                        sub_map.insert(sname, sc);
                    }
                }
            }

            if sub_map.is_empty() { continue; }

            let packed = serde_json::to_string(&sub_map)
                .unwrap_or_else(|_| "{}".to_string());
            let ms = if dir_max_ms > 0 { dir_max_ms } else { now_ms };
            // 目录名加 "/" 后缀用于区分文件和目录格式
            (packed, ms)
        } else {
            continue;
        };

        // 目录条目 name 加 "/" 后缀，方便 import 时判断恢复为目录
        let entry_name = if path.is_dir() {
            format!("{}/", name)
        } else {
            name
        };

        files.push(AutomationFile {
            name: entry_name,
            content,
            updated_at_ms,
            dir_path: if first {
                first = false;
                Some(dir_path_str.clone())
            } else {
                None
            },
        });
    }

    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

/// 诊断用：返回 ~/.codex/automations/ 目录的原始条目列表（不过滤）
#[derive(Debug, serde::Serialize)]
pub struct DirEntry {
    pub name: String,
    pub is_file: bool,
    pub is_dir: bool,
    pub size_bytes: u64,
}

#[tauri::command]
pub fn debug_automations_dir() -> Result<(String, Vec<DirEntry>), String> {
    let automations_dir = crate::db::get_codex_dir().join("automations");
    let dir_path_str = automations_dir.to_string_lossy().to_string();

    if !automations_dir.exists() {
        return Ok((dir_path_str, vec![]));
    }

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&automations_dir).map_err(|e| e.to_string())? {
        let entry = match entry { Ok(e) => e, Err(_) => continue };
        let path = entry.path();
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let meta = path.metadata().ok();
        entries.push(DirEntry {
            name,
            is_file: path.is_file(),
            is_dir: path.is_dir(),
            size_bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((dir_path_str, entries))
}

/// 单条 automation 导入结果
#[derive(Debug, serde::Serialize)]
pub struct AutomationImportResult {
    pub name: String,
    pub ok: bool,
    pub skipped: bool,
    pub error: Option<String>,
}

/// 将从服务端下载的 automation 文件写入本地 ~/.codex/automations/
///
/// 策略：以 updated_at_ms 对比决定是否覆盖；本地更新的保留不覆盖。
#[tauri::command]
pub fn import_automations(
    automations: Vec<AutomationFile>,
) -> Result<Vec<AutomationImportResult>, String> {
    let automations_dir = crate::db::get_codex_dir().join("automations");
    std::fs::create_dir_all(&automations_dir).map_err(|e| e.to_string())?;

    let mut results = Vec::new();

    for automation in automations {
        // 跳过哨兵条目（name 为空，仅用于诊断路径）
        if automation.name.is_empty() {
            continue;
        }

        let is_dir_pack = automation.name.ends_with('/');

        if is_dir_pack {
            // ── 目录格式：name="my-task/" content=JSON{filename->content} ──
            let dir_name = automation.name.trim_end_matches('/');
            let target_dir = automations_dir.join(dir_name);

            // 解包 content JSON
            let sub_files: std::collections::BTreeMap<String, String> =
                match serde_json::from_str(&automation.content) {
                    Ok(m) => m,
                    Err(e) => {
                        results.push(AutomationImportResult {
                            name: automation.name,
                            ok: false,
                            skipped: false,
                            error: Some(format!("解析目录包失败: {}", e)),
                        });
                        continue;
                    }
                };

            if let Err(e) = std::fs::create_dir_all(&target_dir) {
                results.push(AutomationImportResult {
                    name: automation.name,
                    ok: false,
                    skipped: false,
                    error: Some(e.to_string()),
                });
                continue;
            }

            let mut any_written = false;
            for (fname, fcontent) in &sub_files {
                let fpath = target_dir.join(fname);
                if std::fs::write(&fpath, fcontent).is_ok() {
                    any_written = true;
                }
            }

            results.push(AutomationImportResult {
                name: automation.name,
                ok: any_written || sub_files.is_empty(),
                skipped: false,
                error: None,
            });
        } else {
            // ── 普通文件格式 ──────────────────────────────────────────────────
            let file_path = automations_dir.join(&automation.name);

            // 若本地已存在该文件，对比修改时间
            if file_path.exists() {
                let local_ms = file_path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);

                if local_ms >= automation.updated_at_ms {
                    results.push(AutomationImportResult {
                        name: automation.name,
                        ok: true,
                        skipped: true,
                        error: None,
                    });
                    continue;
                }
            }

            match std::fs::write(&file_path, &automation.content) {
                Ok(_) => {
                    results.push(AutomationImportResult {
                        name: automation.name,
                        ok: true,
                        skipped: false,
                        error: None,
                    });
                }
                Err(e) => {
                    results.push(AutomationImportResult {
                        name: automation.name,
                        ok: false,
                        skipped: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }
    }

    Ok(results)
}
