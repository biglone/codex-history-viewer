use codex_history_cli::{
    import_agy_sessions, preview_agy_import, AgyImportPreview, AgyImportSummary,
};
use serde::Serialize;

struct CliOptions {
    action: Action,
    source_path: Option<String>,
    json: bool,
}

#[derive(Clone, Copy)]
enum Action {
    Preview,
    Run,
}

fn main() {
    let code = match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    };
    std::process::exit(code);
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{}", usage());
        return Ok(());
    }

    let opts = parse_args(args)?;
    match opts.action {
        Action::Preview => {
            let preview = preview_agy_import(opts.source_path)?;
            if opts.json {
                print_json(&preview)?;
            } else {
                print_preview(&preview);
            }
        }
        Action::Run => {
            let summary = import_agy_sessions(opts.source_path)?;
            if opts.json {
                print_json(&summary)?;
            } else {
                print_summary(&summary);
            }
            if summary.failed > 0 {
                return Err(format!("{} sessions failed to import", summary.failed));
            }
        }
    }
    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<CliOptions, String> {
    if args.first().map(|s| s.as_str()) != Some("agy-import") {
        return Err(usage());
    }

    let mut action = Action::Run;
    let mut source_path = None;
    let mut json = false;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "preview" | "--preview" | "--dry-run" => action = Action::Preview,
            "run" | "import" => action = Action::Run,
            "--json" => json = true,
            "--source" | "-s" => {
                i += 1;
                if i >= args.len() {
                    return Err("--source requires a path".to_string());
                }
                source_path = Some(args[i].clone());
            }
            other if other.starts_with('-') => return Err(format!("unknown option: {other}")),
            path => {
                if source_path.is_some() {
                    return Err(format!("unexpected extra path: {path}"));
                }
                source_path = Some(path.to_string());
            }
        }
        i += 1;
    }

    Ok(CliOptions {
        action,
        source_path,
        json,
    })
}

fn usage() -> String {
    [
        "Usage:",
        "  codex-history-cli agy-import preview [--json] [--source PATH|PATH]",
        "  codex-history-cli agy-import run     [--json] [--source PATH|PATH]",
        "",
        "Examples:",
        "  codex-history-cli agy-import preview ~/.agy --json",
        "  codex-history-cli agy-import run ~/exports/agy-history.jsonl",
    ]
    .join("\n")
}

fn print_json<T: Serialize>(value: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    println!("{text}");
    Ok(())
}

fn print_preview(preview: &AgyImportPreview) {
    println!("source: {}", preview.source_root);
    println!("scanned_files: {}", preview.scanned_files);
    println!("candidate_count: {}", preview.candidate_count);
    for s in preview.sessions.iter().take(20) {
        println!(
            "- {} | {} messages | {} | {}",
            s.id, s.message_count, s.title, s.source_file
        );
    }
    if !preview.warnings.is_empty() {
        println!("warnings:");
        for w in &preview.warnings {
            println!("  - {w}");
        }
    }
}

fn print_summary(summary: &AgyImportSummary) {
    println!("scanned_files: {}", summary.scanned_files);
    println!("imported: {}", summary.imported);
    println!("skipped: {}", summary.skipped);
    println!("failed: {}", summary.failed);
    for r in &summary.results {
        if !r.ok {
            println!(
                "- failed {}: {}",
                r.id,
                r.error.as_deref().unwrap_or("unknown error")
            );
        }
    }
    if !summary.warnings.is_empty() {
        println!("warnings:");
        for w in &summary.warnings {
            println!("  - {w}");
        }
    }
}
