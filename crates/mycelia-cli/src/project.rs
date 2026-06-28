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
        "# Mycelia index\n\
         \n\
         This project ({name}) is indexed with Mycelia. Use these tools before\n\
         reading files broadly:\n\
         \n\
         - `find(\"your question\")` — ranked chunk headers (paths, signatures, scores)\n\
         - `retrieve(\"chunk_id\")` — full body, validated against disk\n\
         \n\
         Run `mycelia status` to check index health, `mycelia refresh` to rebuild.\n"
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

/// Returns paths of `AGENTS.md` and `CLAUDE.md` that exist directly at `root`.
pub(crate) fn detect_guidance_files(root: &Path) -> Vec<PathBuf> {
    ["AGENTS.md", "CLAUDE.md"]
        .iter()
        .map(|name| root.join(name))
        .filter(|p| p.is_file())
        .collect()
}

/// Returns the owned-block text that `insert_guidance_include` would write.
pub(crate) fn guidance_include_preview() -> &'static str {
    // Return a static string so callers can print it without allocation.
    "<!-- BEGIN mycelia -->\nProject index: see `.mycelia/AGENTS.md` for orientation tools (find/retrieve before broad reads).\n<!-- END mycelia -->\n"
}

/// Inserts or updates the Mycelia owned block in `path`. Idempotent: when the
/// block is already present it is replaced in place; otherwise it is appended.
pub(crate) fn insert_guidance_include(path: &Path) -> Result<(), String> {
    let existing = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;

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
}
