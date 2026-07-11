use crate::db::{
    extract_id_from_filename, find_matching_snippet, get_codex_dir, open_conn, parse_rollout_file,
    query_sessions, Message, SearchResult, Session,
};
use std::fs;
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

#[tauri::command]
pub fn preview_agy_import(
    source_path: Option<String>,
) -> Result<codex_history_cli::AgyImportPreview, String> {
    codex_history_cli::preview_agy_import(source_path)
}

#[tauri::command]
pub fn import_agy_sessions(
    source_path: Option<String>,
) -> Result<codex_history_cli::AgyImportSummary, String> {
    codex_history_cli::import_agy_sessions(source_path)
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
    let project_paths = local_project_path_map();

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
                match normalize_automation_content(fname, fcontent, &project_paths) {
                    Ok(normalized) => {
                        if std::fs::write(&fpath, normalized).is_ok() {
                            any_written = true;
                        }
                    }
                    Err(e) => {
                        results.push(AutomationImportResult {
                            name: format!("{}{}", automation.name, fname),
                            ok: false,
                            skipped: false,
                            error: Some(e),
                        });
                    }
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

            let content = match normalize_automation_content(
                &automation.name,
                &automation.content,
                &project_paths,
            ) {
                Ok(content) => content,
                Err(e) => {
                    results.push(AutomationImportResult {
                        name: automation.name,
                        ok: false,
                        skipped: false,
                        error: Some(e),
                    });
                    continue;
                }
            };

            match std::fs::write(&file_path, &content) {
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

fn local_project_path_map() -> std::collections::BTreeMap<String, String> {
    get_local_project_paths()
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.project_name, p.local_cwd))
        .collect()
}

fn normalize_automation_content(
    file_name: &str,
    content: &str,
    project_paths: &std::collections::BTreeMap<String, String>,
) -> Result<String, String> {
    if !looks_like_json_file(file_name) {
        return Ok(content.to_string());
    }

    let mut value: serde_json::Value = match serde_json::from_str(content) {
        Ok(value) => value,
        Err(_) => return Ok(content.to_string()),
    };

    normalize_automation_json_value(&mut value, None, project_paths)
        .map_err(|e| format!("路径无法自动映射，已跳过以避免 Codex 启动报错: {e}"))?;

    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}

fn looks_like_json_file(name: &str) -> bool {
    name.ends_with(".json")
        || name.ends_with(".jsonc")
        || name.ends_with(".code-workspace")
        || !name.contains('.')
}

fn normalize_automation_json_value(
    value: &mut serde_json::Value,
    key: Option<&str>,
    project_paths: &std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                normalize_automation_json_value(v, Some(k.as_str()), project_paths)?;
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                normalize_automation_json_value(item, key, project_paths)?;
            }
        }
        serde_json::Value::String(raw) => {
            if key.is_some_and(is_automation_path_key) && looks_like_path_value(raw) {
                let mapped = map_path_for_this_device(raw, project_paths)
                    .ok_or_else(|| format!("{key:?}={raw}"))?;
                *raw = mapped;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_automation_path_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    matches!(
        normalized.as_str(),
        "cwd"
            | "path"
            | "root"
            | "root_dir"
            | "workdir"
            | "work_dir"
            | "working_dir"
            | "workspace"
            | "workspace_path"
            | "project_path"
            | "repo_path"
            | "repository_path"
            | "base_path"
    )
}

fn looks_like_path_value(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with('/')
        || trimmed.starts_with("~/")
        || trimmed.starts_with("\\\\")
        || trimmed.starts_with("//")
        || (trimmed.len() > 2
            && trimmed.as_bytes()[1] == b':'
            && matches!(trimmed.as_bytes()[2], b'\\' | b'/'))
}

fn map_path_for_this_device(
    raw: &str,
    project_paths: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let expanded = expand_home_path(raw);
    if expanded.is_absolute() {
        if expanded.exists() {
            return Some(expanded.to_string_lossy().to_string());
        }
    }

    let leaf = path_leaf(raw)?;
    if let Some(local) = project_paths.get(&leaf) {
        return Some(local.clone());
    }

    for root in common_project_roots() {
        let candidate = root.join(&leaf);
        if candidate.exists() && candidate.is_absolute() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }

    None
}

fn expand_home_path(raw: &str) -> std::path::PathBuf {
    if raw == "~" {
        return dirs_next::home_dir().unwrap_or_else(|| std::path::PathBuf::from(raw));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return dirs_next::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(rest);
    }
    std::path::PathBuf::from(raw)
}

fn path_leaf(raw: &str) -> Option<String> {
    raw.replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != "~")
        .last()
        .map(str::to_string)
}

fn common_project_roots() -> Vec<std::path::PathBuf> {
    let home = dirs_next::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    vec![
        home.join("workspace"),
        home.join("work"),
        home.join("projects"),
        home.join("Documents"),
    ]
}
