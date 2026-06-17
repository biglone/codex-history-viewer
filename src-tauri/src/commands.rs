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
