use rusqlite::{Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use walkdir::WalkDir;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub first_user_message: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub archived: bool,
    pub rollout_path: String,
    pub preview: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub session: Session,
    pub matched_message: String,
    pub context: String,
}

pub fn get_codex_dir() -> PathBuf {
    dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
}

pub fn get_db_path() -> PathBuf {
    get_codex_dir().join("state_5.sqlite")
}

pub fn open_conn() -> Result<Connection, String> {
    Connection::open(get_db_path()).map_err(|e| e.to_string())
}

pub fn query_sessions(conn: &Connection, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<Session>, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows: SqlResult<Vec<Session>> = stmt
        .query_map(params, |row| {
            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                cwd: row.get(2)?,
                first_user_message: row.get(3)?,
                created_at_ms: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                updated_at_ms: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                model: row.get(6)?,
                model_provider: row.get(7)?,
                archived: row.get::<_, i32>(8)? != 0,
                rollout_path: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                preview: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
            })
        })
        .map_err(|e| e.to_string())?
        .collect();
    rows.map_err(|e| e.to_string())
}

pub fn parse_rollout_file(rollout_path: &str) -> Result<Vec<Message>, String> {
    let path = PathBuf::from(rollout_path);
    let actual_path = if path.exists() {
        path
    } else {
        let codex_dir = get_codex_dir();
        let relative = rollout_path.trim_start_matches('/');
        let alt = codex_dir.join(relative);
        if alt.exists() {
            alt
        } else {
            let sessions_dir = codex_dir.join("sessions");
            find_session_file(&sessions_dir, rollout_path)?
        }
    };

    let file = fs::File::open(&actual_path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();

    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&line) {
            let msg_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let timestamp = obj
                .get("timestamp")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if msg_type == "response_item" {
                if let Some(payload) = obj.get("payload") {
                    let role = payload
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    if role == "user" || role == "assistant" {
                        let content = extract_content(payload);
                        if !content.trim().is_empty() {
                            messages.push(Message {
                                role,
                                content,
                                timestamp,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(messages)
}

pub fn extract_content(payload: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    if let Some(content_arr) = payload.get("content").and_then(|v| v.as_array()) {
        for item in content_arr {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                parts.push(text.to_string());
            } else if let Some(text) = item.get("output").and_then(|v| v.as_str()) {
                parts.push(text.to_string());
            }
        }
    }
    parts.join("\n")
}

pub fn find_session_file(sessions_dir: &PathBuf, rollout_path: &str) -> Result<PathBuf, String> {
    let filename = PathBuf::from(rollout_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    if filename.is_empty() {
        return Err(format!("Cannot resolve session file: {}", rollout_path));
    }

    for entry in WalkDir::new(sessions_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_name().to_string_lossy() == filename {
            return Ok(entry.path().to_path_buf());
        }
    }

    Err(format!("Session file not found: {}", filename))
}

pub fn extract_id_from_filename(filename: &str) -> Option<String> {
    let without_ext = filename.trim_end_matches(".jsonl");
    let parts: Vec<&str> = without_ext.split('-').collect();
    let n = parts.len();
    if n >= 5 {
        let uuid = format!(
            "{}-{}-{}-{}-{}",
            parts[n - 5],
            parts[n - 4],
            parts[n - 3],
            parts[n - 2],
            parts[n - 1]
        );
        return Some(uuid);
    }
    None
}

pub fn find_matching_snippet(content: &str, query: &str) -> String {
    let content_lower = content.to_lowercase();
    if let Some(pos) = content_lower.find(query) {
        let start = pos.saturating_sub(100);
        let end = (pos + query.len() + 150).min(content.len());
        let snippet = &content[start..end];
        if let Ok(val) =
            serde_json::from_str::<serde_json::Value>(snippet.lines().next().unwrap_or(""))
        {
            if let Some(text) = val
                .get("payload")
                .and_then(|p| p.get("content"))
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| item.get("text"))
                .and_then(|t| t.as_str())
            {
                return text.chars().take(200).collect();
            }
        }
        return snippet.chars().take(200).collect::<String>();
    }
    String::new()
}
