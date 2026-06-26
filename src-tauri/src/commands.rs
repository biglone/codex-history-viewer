use crate::db::{
    extract_id_from_filename, find_matching_snippet, get_codex_dir, open_conn, parse_rollout_file,
    query_sessions, Message, SearchResult, Session,
};
use walkdir::WalkDir;
use std::fs;

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
