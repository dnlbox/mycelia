use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    name: Option<String>,
}

const BLOCK_START: &str = "<!-- BEGIN mycelia -->";
const BLOCK_END: &str = "<!-- END mycelia -->";

const GITIGNORE_CONTENT: &str = "db/\nlogs/\ncache/\nartifacts/\n";

fn agents_md_content(name: &str) -> String {
    format!(
        "# Mycelia project context\n\
         \n\
         This project ({name}) has a Mycelia index. Use Mycelia before broad\n\
         shell search or file reads when you are orienting, locating an\n\
         implementation, tracing related code, or looking for docs.\n\
         \n\
         Mandatory protocol files still come first when they define the active\n\
         contract, for example `AGENTS.md`, `BUILD_STATE.md`, or `prompt.md`.\n\
         Only say you used Mycelia when the transcript shows an actual Mycelia\n\
         MCP tool call such as `find`, `find_changed`, `locate_implementation`,\n\
         `search_codebase`, `retrieve`, `find_related`, or `list_corpora`.\n\
         If the MCP tools are not available, say that plainly and use shell\n\
         search as a fallback. Do not describe `rg`, `grep`, `sed`, `nl`, or\n\
         direct file reads as Mycelia use.\n\
         \n\
         ## MCP tools\n\
         \n\
         - `find(query, limit?, corpus?)`: cheap first-pass orientation. Returns\n\
           ranked headers with paths, line ranges, signatures or synopses,\n\
           scores, and namespaced chunk ids. Use this before grep/read for broad\n\
           discovery.\n\
         - `search_codebase(query, limit?, corpus?)`: alias for `find`; use when\n\
           the task says search the codebase or find relevant files.\n\
         - `locate_implementation(query, limit?, corpus?)`: alias for `find`; use\n\
           when the task asks where behavior is implemented or which symbols\n\
           support a feature.\n\
         - `retrieve(chunk_id)`: fetch the selected chunk body after a Mycelia\n\
           search. Retrieval validates against disk and never serves a stale\n\
           indexed slice.\n\
         - `find_related(symbol, direction, corpus?)`: inspect sourced `calls`\n\
           relationships. Use `direction=\"callers\"` for who calls a symbol and\n\
           `direction=\"callees\"` for what a symbol calls.\n\
         - `find_changed(paths, limit?, corpus?)`: PR-review orientation. Pass\n\
           changed paths relative to the corpus root to get changed chunks plus\n\
           their callers and callees.\n\
         - `list_corpora()`: disambiguate available corpora only when the user\n\
           names another project or Mycelia asks for a corpus.\n\
         \n\
         ## Tool choice\n\
         \n\
         1. For broad orientation, call `find`, `search_codebase`, or\n\
            `locate_implementation` before shell search.\n\
         2. Retrieve only the most relevant chunks, usually one to three, then\n\
            read exact files or lines for edits and verification.\n\
         3. Use `find_related` for call graph questions instead of trying to infer\n\
            relationships from text search.\n\
         4. Use `find_changed` first for PR-review or change-impact tasks when\n\
            changed paths are known.\n\
         5. Use grep/read directly for exact literals, known files, generated\n\
            output, lockfiles, or after Mycelia misses.\n\
         6. If the first result set is broad, refine the Mycelia query once before\n\
            falling back to grep.\n\
         \n\
         ## Few-shot patterns\n\
         \n\
         - User asks \"where is X implemented?\" -> call\n\
           `locate_implementation(\"X implementation\")`, then retrieve the best\n\
           chunk.\n\
         - User asks \"what calls X?\" -> call\n\
           `find_related(\"X\", direction=\"callers\")`, then retrieve sourced\n\
           hits as needed.\n\
         - User asks for current project direction -> call `find` with the slice,\n\
           roadmap, state, and concept keywords, then retrieve the state and\n\
           roadmap chunks.\n\
         - User gives an exact file path or line -> read that path directly;\n\
           Mycelia is not needed for exact lookup.\n\
         \n\
         Run `mycelia status` to check index health and `mycelia stats --recent 20`\n\
         to verify whether agents are using Mycelia.\n"
    )
}

fn config_toml_content(name: &str) -> String {
    format!("name = \"{name}\"\n")
}

fn guidance_block() -> String {
    format!(
        "{BLOCK_START}\nProject index: see `.mycelia/AGENTS.md` for orientation tools (find/retrieve before broad reads).\n{BLOCK_END}\n"
    )
}

/// Creates or updates the `.mycelia/` project directory tree. Idempotent:
/// directories are always created safely; config.toml is only written when
/// absent (the user may customise it); AGENTS.md and .gitignore are always
/// rewritten (Mycelia owns them).
pub(crate) fn init_project(root: &Path, name: &str) -> Result<(), String> {
    let mycelia_dir = root.join(".mycelia");

    for subdir in &["db", "logs", "cache"] {
        std::fs::create_dir_all(mycelia_dir.join(subdir))
            .map_err(|e| format!("failed to create .mycelia/{subdir}: {e}"))?;
    }

    let gitignore = mycelia_dir.join(".gitignore");
    std::fs::write(&gitignore, GITIGNORE_CONTENT)
        .map_err(|e| format!("failed to write .mycelia/.gitignore: {e}"))?;

    let agents_md = mycelia_dir.join("AGENTS.md");
    std::fs::write(&agents_md, agents_md_content(name))
        .map_err(|e| format!("failed to write .mycelia/AGENTS.md: {e}"))?;

    let config_toml = mycelia_dir.join("config.toml");
    if !config_toml.exists() {
        std::fs::write(&config_toml, config_toml_content(name))
            .map_err(|e| format!("failed to write .mycelia/config.toml: {e}"))?;
    }

    Ok(())
}

/// Strips `//` and `/* */` comments from JSON/JSONC text so serde_json can parse it.
pub(crate) fn strip_json_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_str = false;
    let mut esc = false;
    while let Some(c) = chars.next() {
        if esc {
            out.push(c);
            esc = false;
            continue;
        }
        if c == '\\' && in_str {
            esc = true;
            out.push(c);
            continue;
        }
        if c == '"' {
            in_str = !in_str;
            out.push(c);
            continue;
        }
        if !in_str && c == '/' {
            if chars.peek() == Some(&'/') {
                loop {
                    match chars.next() {
                        Some('\n') => {
                            out.push('\n');
                            break;
                        }
                        Some(_) => continue,
                        None => break,
                    }
                }
                continue;
            } else if chars.peek() == Some(&'*') {
                chars.next();
                loop {
                    match chars.next() {
                        Some('*') if chars.peek() == Some(&'/') => {
                            chars.next();
                            break;
                        }
                        Some(_) => continue,
                        None => break,
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn is_claude_settings_file(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some("settings.json")
        && path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some(".claude")
}

/// Returns paths of instruction conventions across target harnesses in `root`.
pub(crate) fn detect_guidance_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for name in &["AGENTS.md", "CLAUDE.md"] {
        let p = root.join(name);
        if p.is_file() {
            files.push(p);
        }
    }

    if root.join("CLAUDE.md").is_file() || root.join(".claude").exists() {
        files.push(root.join(".claude").join("settings.json"));
    }

    let agents_md = root.join(".agents").join("AGENTS.md");
    if agents_md.is_file() || root.join(".agents").exists() {
        files.push(agents_md);
    }

    let codex_md = root.join(".codex").join("instructions.md");
    if codex_md.is_file() || root.join(".codex").exists() {
        files.push(codex_md);
    }

    let opencode_md = root.join(".opencode").join("AGENTS.md");
    if opencode_md.is_file() || root.join(".opencode").exists() {
        files.push(opencode_md);
    }

    let kilo_md = root.join(".kilo").join("AGENTS.md");
    if kilo_md.is_file() || root.join(".kilo").exists() {
        files.push(kilo_md);
    }

    let cursor_rules = root.join(".cursor").join("rules");
    if cursor_rules.is_dir() {
        let mut found_mdc = false;
        if let Ok(entries) = std::fs::read_dir(&cursor_rules) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && p.extension().and_then(|ext| ext.to_str()) == Some("mdc") {
                    files.push(p);
                    found_mdc = true;
                }
            }
        }
        if !found_mdc {
            files.push(cursor_rules.join("mycelia.mdc"));
        }
    } else if root.join(".cursor").exists() {
        files.push(cursor_rules.join("mycelia.mdc"));
    }

    files.sort();
    files.dedup();
    files
}

/// Returns the guidance block text or JSON snippet that `insert_guidance_include` would write.
pub(crate) fn guidance_include_preview(path: &Path) -> String {
    if is_claude_settings_file(path) {
        "{\n  \"enableAllProjectMcpServers\": true,\n  \"enabledMcpjsonServers\": [\n    \"mycelia\"\n  ]\n}\n".to_string()
    } else {
        guidance_block()
    }
}

fn update_claude_project_settings(path: &Path) -> Result<(), String> {
    let mut root: serde_json::Value = if path.is_file() {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let stripped = strip_json_comments(&text);
        serde_json::from_str(&stripped)
            .map_err(|e| format!("invalid JSON in {}: {e}", path.display()))?
    } else {
        serde_json::json!({})
    };

    let obj = root
        .as_object_mut()
        .ok_or_else(|| format!("unexpected root type in {}", path.display()))?;

    obj.insert(
        "enableAllProjectMcpServers".to_string(),
        serde_json::Value::Bool(true),
    );

    let servers = obj
        .entry("enabledMcpjsonServers")
        .or_insert_with(|| serde_json::json!([]));
    if let Some(arr) = servers.as_array_mut() {
        let name_val = serde_json::Value::String("mycelia".to_string());
        if !arr.contains(&name_val) {
            arr.push(name_val);
        }
    } else {
        return Err(format!(
            "enabledMcpjsonServers is not an array in {}",
            path.display()
        ));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

/// Inserts or updates the Mycelia owned block or eager tool loading in `path`. Idempotent: when the
/// block/setting is already present it is replaced/kept in place; otherwise it is appended/added.
pub(crate) fn insert_guidance_include(path: &Path) -> Result<(), String> {
    if is_claude_settings_file(path) {
        return update_claude_project_settings(path);
    }

    let existing = if path.is_file() {
        std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        String::new()
    };

    let block = guidance_block();

    let updated = if let (Some(start_idx), Some(end_tag_start)) =
        (existing.find(BLOCK_START), existing.find(BLOCK_END))
    {
        if end_tag_start > start_idx {
            let after_end = end_tag_start + BLOCK_END.len();
            // Consume the newline that terminates the end-tag line, if present.
            let after_end = if existing.as_bytes().get(after_end) == Some(&b'\n') {
                after_end + 1
            } else {
                after_end
            };
            format!(
                "{}{}{}",
                &existing[..start_idx],
                block,
                &existing[after_end..]
            )
        } else {
            append_block(&existing, &block)
        }
    } else {
        append_block(&existing, &block)
    };

    std::fs::write(path, updated).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

fn append_block(existing: &str, block: &str) -> String {
    let mut result = existing.to_string();
    if !result.is_empty() {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
    }
    result.push_str(block);
    result
}

pub(crate) struct ProjectResolution {
    pub(crate) name: String,
    pub(crate) root: PathBuf,
    pub(crate) database: PathBuf,
    pub(crate) log_path: PathBuf,
}

/// Walks up from `cwd` looking for `.mycelia/config.toml`. Returns
/// `Some(Ok(...))` when found and valid, `Some(Err(...))` when found but
/// malformed, `None` when no config exists in any ancestor directory.
pub(crate) fn resolve_from_cwd(cwd: &Path) -> Option<Result<ProjectResolution, String>> {
    let mut dir = cwd.to_path_buf();
    loop {
        let config_path = dir.join(".mycelia").join("config.toml");
        if config_path.is_file() {
            return Some(load(&config_path, &dir));
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn load(config_path: &Path, project_root: &Path) -> Result<ProjectResolution, String> {
    let text = std::fs::read_to_string(config_path)
        .map_err(|e| format!("failed to read {}: {e}", config_path.display()))?;
    let config: ProjectConfig =
        toml_edit::de::from_str(&text).map_err(|e| format!("invalid .mycelia/config.toml: {e}"))?;

    let name = config.name.unwrap_or_else(|| {
        project_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_owned()
    });

    let mycelia_dir = project_root.join(".mycelia");
    Ok(ProjectResolution {
        name: name.clone(),
        root: project_root.to_path_buf(),
        database: mycelia_dir.join("db").join("index.sqlite3"),
        log_path: mycelia_dir.join("logs").join(format!("{name}.log")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // ---- init_project -------------------------------------------------------

    #[test]
    fn init_project_creates_expected_tree() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();

        init_project(root, "myrepo").expect("init_project");

        let m = root.join(".mycelia");
        assert!(m.join("config.toml").is_file(), "config.toml missing");
        assert!(m.join("AGENTS.md").is_file(), "AGENTS.md missing");
        assert!(m.join(".gitignore").is_file(), ".gitignore missing");
        assert!(m.join("db").is_dir(), "db/ missing");
        assert!(m.join("logs").is_dir(), "logs/ missing");
        assert!(m.join("cache").is_dir(), "cache/ missing");

        let config = fs::read_to_string(m.join("config.toml")).expect("read config");
        assert!(
            config.contains("myrepo"),
            "config.toml should contain corpus name"
        );

        let agents = fs::read_to_string(m.join("AGENTS.md")).expect("read AGENTS.md");
        assert!(
            agents.contains("myrepo"),
            "AGENTS.md should mention corpus name"
        );
        assert!(agents.contains("find("), "AGENTS.md should mention find");
        assert!(
            agents.contains("locate_implementation"),
            "AGENTS.md should mention the implementation-hunt alias"
        );
        assert!(
            agents.contains("find_related"),
            "AGENTS.md should mention graph relationships"
        );
        assert!(
            agents.contains("Few-shot patterns"),
            "AGENTS.md should include usage examples"
        );
        assert!(
            agents.contains("Only say you used Mycelia"),
            "AGENTS.md should distinguish real MCP calls from claims"
        );

        let gi = fs::read_to_string(m.join(".gitignore")).expect("read .gitignore");
        assert!(gi.contains("db/"));
        assert!(gi.contains("logs/"));
        assert!(gi.contains("cache/"));
        assert!(gi.contains("artifacts/"));
    }

    #[test]
    fn init_project_is_idempotent() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();

        init_project(root, "repo").expect("first init");
        // Manually customise config.toml to verify it is not overwritten.
        let config_path = root.join(".mycelia").join("config.toml");
        fs::write(&config_path, "name = \"repo\"\ncustom = true\n").expect("write custom config");

        init_project(root, "repo").expect("second init");

        let config = fs::read_to_string(&config_path).expect("read config");
        assert!(
            config.contains("custom = true"),
            "second init must not overwrite existing config.toml"
        );
    }

    // ---- detect_guidance_files ---------------------------------------------

    #[test]
    fn detect_guidance_files_finds_present_files() {
        let temp = tempdir().expect("tempdir");
        fs::write(temp.path().join("AGENTS.md"), "# agents").expect("write AGENTS.md");
        fs::write(temp.path().join("CLAUDE.md"), "# claude").expect("write CLAUDE.md");

        let found = detect_guidance_files(temp.path());
        let names: Vec<_> = found.iter().filter_map(|p| p.file_name()).collect();
        assert!(names.contains(&std::ffi::OsStr::new("AGENTS.md")));
        assert!(names.contains(&std::ffi::OsStr::new("CLAUDE.md")));
    }

    #[test]
    fn detect_guidance_files_returns_empty_when_none_present() {
        let temp = tempdir().expect("tempdir");
        assert!(detect_guidance_files(temp.path()).is_empty());
    }

    // ---- insert_guidance_include -------------------------------------------

    #[test]
    fn insert_guidance_include_appends_block_to_existing_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("AGENTS.md");
        fs::write(&path, "# Project\n\nSome existing content.\n").expect("write file");

        insert_guidance_include(&path).expect("insert");

        let result = fs::read_to_string(&path).expect("read result");
        assert!(result.contains(BLOCK_START));
        assert!(result.contains(BLOCK_END));
        assert!(result.contains("Some existing content."));
        assert!(result.contains(".mycelia/AGENTS.md"));
    }

    #[test]
    fn insert_guidance_include_is_idempotent() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("AGENTS.md");
        fs::write(&path, "# Existing\n").expect("write file");

        insert_guidance_include(&path).expect("first insert");
        let after_first = fs::read_to_string(&path).expect("read after first");

        insert_guidance_include(&path).expect("second insert");
        let after_second = fs::read_to_string(&path).expect("read after second");

        assert_eq!(
            after_first.matches(BLOCK_START).count(),
            1,
            "block should appear exactly once after first insert"
        );
        assert_eq!(
            after_second.matches(BLOCK_START).count(),
            1,
            "block should appear exactly once after second insert"
        );
    }

    #[test]
    fn insert_guidance_include_updates_block_content_in_place() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("AGENTS.md");
        let initial =
            "# Header\n\n<!-- BEGIN mycelia -->\nold content\n<!-- END mycelia -->\n\nTrailing.\n";
        fs::write(&path, initial).expect("write file");

        insert_guidance_include(&path).expect("update");

        let result = fs::read_to_string(&path).expect("read result");
        assert!(result.contains("# Header\n"));
        assert!(result.contains("Trailing.\n"));
        assert!(
            !result.contains("old content"),
            "old block content should be replaced"
        );
        assert_eq!(result.matches(BLOCK_START).count(), 1);
    }

    #[test]
    fn insert_guidance_include_works_on_empty_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("CLAUDE.md");
        fs::write(&path, "").expect("write empty file");

        insert_guidance_include(&path).expect("insert into empty");

        let result = fs::read_to_string(&path).expect("read result");
        assert!(result.starts_with(BLOCK_START));
    }

    #[test]
    fn resolves_from_direct_project_dir() {
        let temp = tempdir().expect("tempdir");
        let project = temp.path().join("myproject");
        let mycelia_dir = project.join(".mycelia");
        fs::create_dir_all(&mycelia_dir).expect("create .mycelia");
        fs::write(mycelia_dir.join("config.toml"), r#"name = "myproject""#).expect("write config");

        let result = resolve_from_cwd(&project)
            .expect("should find config")
            .expect("should parse");
        assert_eq!(result.name, "myproject");
        assert_eq!(result.root, project);
        assert_eq!(
            result.database,
            mycelia_dir.join("db").join("index.sqlite3")
        );
        assert_eq!(
            result.log_path,
            mycelia_dir.join("logs").join("myproject.log")
        );
    }

    #[test]
    fn resolves_from_subdirectory() {
        let temp = tempdir().expect("tempdir");
        let project = temp.path().join("myproject");
        let mycelia_dir = project.join(".mycelia");
        let subdir = project.join("src").join("deep");
        fs::create_dir_all(&mycelia_dir).expect("create .mycelia");
        fs::create_dir_all(&subdir).expect("create subdir");
        fs::write(mycelia_dir.join("config.toml"), "# no name field").expect("write config");

        let result = resolve_from_cwd(&subdir)
            .expect("should find config")
            .expect("should parse");
        assert_eq!(result.name, "myproject");
        assert_eq!(result.root, project);
    }

    #[test]
    fn returns_none_when_no_config() {
        let temp = tempdir().expect("tempdir");
        assert!(resolve_from_cwd(temp.path()).is_none());
    }

    #[test]
    fn returns_err_on_malformed_config() {
        let temp = tempdir().expect("tempdir");
        let mycelia_dir = temp.path().join(".mycelia");
        fs::create_dir_all(&mycelia_dir).expect("create .mycelia");
        fs::write(mycelia_dir.join("config.toml"), "name = [1, 2, 3]").expect("write invalid");

        let result = resolve_from_cwd(temp.path()).expect("found config");
        assert!(result.is_err(), "malformed config should return Err");
    }

    #[test]
    fn detect_guidance_files_finds_harness_conventions() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        fs::create_dir_all(root.join(".claude")).expect("create .claude");
        fs::write(root.join(".claude/settings.json"), "{}").expect("write settings");
        fs::create_dir_all(root.join(".agents")).expect("create .agents");
        fs::write(root.join(".agents/AGENTS.md"), "# agents").expect("write agents");
        fs::create_dir_all(root.join(".opencode")).expect("create .opencode");
        fs::write(root.join(".opencode/AGENTS.md"), "# opencode").expect("write opencode");

        let found = detect_guidance_files(root);
        let paths: Vec<_> = found
            .iter()
            .map(|p| p.strip_prefix(root).unwrap())
            .collect();
        assert!(paths.contains(&Path::new(".claude/settings.json")));
        assert!(paths.contains(&Path::new(".agents/AGENTS.md")));
        assert!(paths.contains(&Path::new(".opencode/AGENTS.md")));
    }

    #[test]
    fn insert_guidance_include_updates_claude_settings_idempotently() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join(".claude/settings.json");
        fs::create_dir_all(path.parent().unwrap()).expect("create dir");
        // Include comments to verify strip_json_comments works when reading existing settings.json
        fs::write(&path, "{\n  // A comment\n  \"custom\": true\n}\n").expect("write initial");

        insert_guidance_include(&path).expect("first insert");
        let content = fs::read_to_string(&path).expect("read after first");
        let val: serde_json::Value = serde_json::from_str(&content).expect("valid json");
        assert_eq!(val["enableAllProjectMcpServers"], true);
        assert_eq!(val["custom"], true);
        assert_eq!(val["enabledMcpjsonServers"].as_array().unwrap().len(), 1);

        insert_guidance_include(&path).expect("second insert");
        let content2 = fs::read_to_string(&path).expect("read after second");
        let val2: serde_json::Value = serde_json::from_str(&content2).expect("valid json");
        assert_eq!(val2["enabledMcpjsonServers"].as_array().unwrap().len(), 1);
    }
}
