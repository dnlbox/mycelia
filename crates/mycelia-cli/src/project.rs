use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    name: Option<String>,
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
