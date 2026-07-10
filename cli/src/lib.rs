use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Message {
    role: String,
    content: String,
    timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct AgySessionPreview {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub message_count: usize,
    pub source_file: String,
}

#[derive(Debug, Serialize)]
pub struct AgyImportPreview {
    pub source_root: String,
    pub scanned_files: usize,
    pub candidate_count: usize,
    pub default_paths: Vec<String>,
    pub sessions: Vec<AgySessionPreview>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub id: String,
    pub ok: bool,
    pub skipped: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
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

pub fn preview_agy_import(source_path: Option<String>) -> Result<AgyImportPreview, String> {
    let (root, default_paths) = resolve_agy_import_root(source_path)?;
    let scan = scan_agy_sessions(&root)?;
    let candidate_count = scan.sessions.len();
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
        candidate_count,
        default_paths,
        sessions,
        warnings: scan.warnings,
    })
}

pub fn import_agy_sessions(source_path: Option<String>) -> Result<AgyImportSummary, String> {
    let (root, _) = resolve_agy_import_root(source_path)?;
    let scan = scan_agy_sessions(&root)?;
    let codex_dir = get_codex_dir();
    let db_path = get_db_path();
    if !db_path.exists() {
        return Err(format!(
            "Codex state database not found: {}. Run Codex once before importing.",
            db_path.to_string_lossy()
        ));
    }
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let imported_dir = codex_dir.join("agy_imported");
    std::fs::create_dir_all(&imported_dir).map_err(|e| e.to_string())?;

    let mut imported = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let mut results = Vec::new();

    for session in scan.sessions {
        let result = import_one(&conn, &imported_dir, session);
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

fn import_one(conn: &Connection, imported_dir: &Path, s: AgyParsedSession) -> ImportResult {
    let local_updated: Option<i64> = conn
        .query_row(
            "SELECT updated_at_ms FROM threads WHERE id = ?1",
            [&s.id],
            |row| row.get(0),
        )
        .ok();

    if let Some(local_ms) = local_updated {
        if local_ms >= s.updated_at_ms {
            return ImportResult {
                id: s.id,
                ok: true,
                skipped: true,
                error: None,
            };
        }
    }

    let jsonl_path = imported_dir.join(format!("{}.jsonl", s.id));
    let rollout_path = jsonl_path.to_string_lossy().to_string();
    let content = s
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
            error: Some(format!("write jsonl failed: {e}")),
        };
    }

    let created_at_sec = s.created_at_ms / 1000;
    let updated_at_sec = s.updated_at_ms / 1000;
    let first_user = first_message_by_role(&s.messages, "user").unwrap_or_default();
    let preview = first_message_by_role(&s.messages, "assistant").unwrap_or_default();

    let upsert = conn.execute(
        "INSERT INTO threads (
            id, rollout_path, created_at, updated_at, source,
            model_provider, cwd, title, sandbox_policy, approval_mode,
            first_user_message, preview,
            created_at_ms, updated_at_ms,
            model, archived, thread_source
         ) VALUES (?1,?2,?3,?4,'remote',?5,?6,?7,'none','suggest',?8,?9,?10,?11,?12,0,?13)
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
            rollout_path,
            created_at_sec,
            updated_at_sec,
            s.model_provider.as_deref().unwrap_or("Agy"),
            s.cwd,
            s.title,
            first_user,
            preview,
            s.created_at_ms,
            s.updated_at_ms,
            s.model.as_deref().unwrap_or(""),
            format!("agy-import:{}", s.source_file.to_string_lossy()),
        ],
    );

    match upsert {
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

struct AgyScanResult {
    scanned_files: usize,
    sessions: Vec<AgyParsedSession>,
    warnings: Vec<String>,
}

fn resolve_agy_import_root(source_path: Option<String>) -> Result<(PathBuf, Vec<String>), String> {
    let defaults = default_agy_paths();
    if let Some(raw) = source_path {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let expanded = expand_home_path(trimmed);
            if expanded.exists() {
                return Ok((expanded, defaults));
            }
            return Err(format!("path not found: {}", expanded.to_string_lossy()));
        }
    }

    for path in &defaults {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok((p, defaults));
        }
    }

    Err(
        "default Agy history directory not found; pass a JSON/JSONL/TXT file or directory path"
            .to_string(),
    )
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

    for path in collect_agy_candidate_files(root)? {
        scanned_files += 1;
        match parse_agy_file(&path) {
            Ok(mut parsed) => sessions.append(&mut parsed),
            Err(e) => {
                if warnings.len() < 20 {
                    warnings.push(format!("skip {}: {}", path.to_string_lossy(), e));
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    Ok(AgyScanResult {
        scanned_files,
        sessions,
        warnings,
    })
}

fn collect_agy_candidate_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    if root.is_file() {
        return if is_agy_candidate_file(root) {
            Ok(vec![root.to_path_buf()])
        } else {
            Err("unsupported file type; use .json/.jsonl/.ndjson/.txt/.md".to_string())
        };
    }
    if !root.is_dir() {
        return Err(format!(
            "not a file or directory: {}",
            root.to_string_lossy()
        ));
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .max_depth(8)
        .into_iter()
        .filter_entry(|entry| !should_skip_entry(entry.path()))
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && is_agy_candidate_file(path) && file_size_ok(path) {
            files.push(path.to_path_buf());
        }
        if files.len() >= 1000 {
            break;
        }
    }
    Ok(files)
}

fn should_skip_entry(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_lowercase()) else {
        return false;
    };
    matches!(
        name.as_str(),
        ".git"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "venv"
            | ".venv"
            | "runtime"
            | "skills"
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
        return Err("empty file".to_string());
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mut sessions = if ext == "jsonl" || ext == "ndjson" {
        parse_jsonl(path, &raw)?
    } else if ext == "json" {
        parse_json(path, &raw)?
    } else {
        parse_text(path, &raw)?
    };
    sessions.retain(|s| !s.messages.is_empty());
    if sessions.is_empty() {
        return Err("no user/assistant messages recognized".to_string());
    }
    Ok(sessions)
}

fn parse_json(path: &Path, raw: &str) -> Result<Vec<AgyParsedSession>, String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let mut sessions = Vec::new();
    extract_sessions_from_json(path, &value, &mut sessions);
    Ok(sessions)
}

fn parse_jsonl(path: &Path, raw: &str) -> Result<Vec<AgyParsedSession>, String> {
    let mut meta = serde_json::Map::new();
    let mut messages = Vec::new();
    let mut embedded_sessions = Vec::new();

    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
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
        embedded_sessions.push(build_session(
            path,
            &serde_json::Value::Object(meta),
            messages,
            0,
        ));
    }
    Ok(embedded_sessions)
}

fn parse_text(path: &Path, raw: &str) -> Result<Vec<AgyParsedSession>, String> {
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
        return Err("text file needs User:/Assistant: style prefixes".to_string());
    }
    Ok(vec![build_session(
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
        out.push(build_session(path, value, messages, idx));
        return;
    }
    if let Some(arr) = value.as_array() {
        let messages = arr.iter().filter_map(message_from_json).collect::<Vec<_>>();
        if !messages.is_empty() {
            let idx = out.len();
            out.push(build_session(path, value, messages, idx));
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
    value.as_object()?;
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
                return Some(Message {
                    role: "user".to_string(),
                    content: json_string(payload, &["message", "text", "content"])?,
                    timestamp: json_string(value, &["timestamp"]).unwrap_or_default(),
                });
            }
        }
    }

    let role = normalize_role(&json_string(
        value,
        &["role", "author", "sender", "speaker", "type"],
    )?)?;
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

fn build_session(
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
                .unwrap_or_else(|| "Agy imported session".to_string())
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
    let a = fnv1a64(seed.as_bytes(), 0xcbf29ce484222325);
    let reversed = seed.chars().rev().collect::<String>();
    let b = fnv1a64(reversed.as_bytes(), 0x84222325cbf29ce4);
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

fn fnv1a64(bytes: &[u8], seed: u64) -> u64 {
    let mut hash = seed;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn get_codex_dir() -> PathBuf {
    if let Some(path) = env_path("CODEX_HOME") {
        return path;
    }
    dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
}

fn get_db_path() -> PathBuf {
    env_path("CODEX_SQLITE_HOME")
        .unwrap_or_else(get_codex_dir)
        .join("state_5.sqlite")
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
