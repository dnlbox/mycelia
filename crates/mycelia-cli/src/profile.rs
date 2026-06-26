use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Deserialize, Serialize)]
struct StoredProfile {
    root: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct CorpusProfile {
    pub(crate) name: String,
    pub(crate) root: PathBuf,
    pub(crate) database: PathBuf,
}

#[derive(Debug)]
struct ProfilePaths {
    config: PathBuf,
    data: PathBuf,
}

impl ProfilePaths {
    fn from_environment() -> Result<Self> {
        Ok(Self {
            config: mycelia_home("MYCELIA_CONFIG_HOME", "XDG_CONFIG_HOME", ".config")?,
            data: mycelia_home("MYCELIA_DATA_HOME", "XDG_DATA_HOME", ".local/share")?,
        })
    }

    fn profile_path(&self, name: &str) -> PathBuf {
        self.config.join("corpora").join(format!("{name}.json"))
    }

    fn database_path(&self, name: &str) -> PathBuf {
        self.data.join("corpora").join(format!("{name}.sqlite3"))
    }

    fn log_path(&self, name: &str) -> PathBuf {
        self.data.join("logs").join(format!("{name}.log"))
    }
}

/// Returns the path of the corpus activity log.
pub(crate) fn log_path_for(name: &str) -> Result<PathBuf> {
    Ok(ProfilePaths::from_environment()?.log_path(name))
}

/// Returns the path of the corpus profile file.
pub(crate) fn profile_path_for(name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    Ok(ProfilePaths::from_environment()?.profile_path(name))
}

/// Walks up from `from` looking for a `.git` directory, returning the first
/// ancestor that contains one. Works for both bare repos and worktrees.
pub(crate) fn git_root(from: &Path) -> Option<PathBuf> {
    let mut dir = from.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Infers the corpus for the current working directory by walking up to the
/// nearest git root and matching it against registered corpus roots. The
/// deepest registered root that is an ancestor of (or equal to) `cwd` wins.
pub(crate) fn infer_from_cwd(cwd: &Path) -> Result<CorpusProfile> {
    let all = list()?;
    if all.is_empty() {
        return Err("no corpora registered; run `mycelia setup` first".to_string());
    }

    // Canonicalise cwd so comparisons work regardless of symlinks.
    let canonical_cwd = cwd
        .canonicalize()
        .map_err(|error| format!("cannot resolve current directory: {error}"))?;

    let mut best: Option<CorpusProfile> = None;
    for profile in all {
        let canonical_root = match profile.root.canonicalize() {
            Ok(r) => r,
            Err(_) => continue,
        };
        if canonical_cwd.starts_with(&canonical_root) {
            match &best {
                None => best = Some(profile),
                Some(prev) => {
                    if canonical_root.components().count()
                        > prev
                            .root
                            .canonicalize()
                            .map(|r| r.components().count())
                            .unwrap_or(0)
                    {
                        best = Some(profile);
                    }
                }
            }
        }
    }

    best.ok_or_else(|| {
        "current directory is not under any registered corpus; run `mycelia setup` first"
            .to_string()
    })
}

pub(crate) fn set(name: &str, root: &Path) -> Result<CorpusProfile> {
    validate_name(name)?;
    let root = root
        .canonicalize()
        .map_err(|error| format!("invalid corpus root {}: {error}", root.display()))?;
    if !root.is_dir() {
        return Err(format!(
            "corpus root is not a directory: {}",
            root.display()
        ));
    }

    let paths = ProfilePaths::from_environment()?;
    let profile_path = paths.profile_path(name);
    let parent = profile_path
        .parent()
        .ok_or_else(|| "profile path has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;

    let contents = serde_json::to_vec_pretty(&StoredProfile { root: root.clone() })
        .map_err(|error| format!("failed to serialize corpus profile: {error}"))?;
    let temporary_path = profile_path.with_extension("json.tmp");
    fs::write(&temporary_path, contents)
        .map_err(|error| format!("failed to write {}: {error}", temporary_path.display()))?;
    fs::rename(&temporary_path, &profile_path)
        .map_err(|error| format!("failed to install {}: {error}", profile_path.display()))?;

    Ok(CorpusProfile {
        name: name.to_owned(),
        root,
        database: paths.database_path(name),
    })
}

pub(crate) fn get(name: &str) -> Result<CorpusProfile> {
    validate_name(name)?;
    let paths = ProfilePaths::from_environment()?;
    load(&paths, name)
}

pub(crate) fn list() -> Result<Vec<CorpusProfile>> {
    let paths = ProfilePaths::from_environment()?;
    let directory = paths.config.join("corpora");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!("failed to read {}: {error}", directory.display()));
        }
    };

    let mut names = entries
        .map(|entry| {
            let entry = entry.map_err(|error| {
                format!("failed to read entry in {}: {error}", directory.display())
            })?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                return Ok(None);
            }
            let name = path
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    format!("profile filename is not valid UTF-8: {}", path.display())
                })?;
            validate_name(name)?;
            Ok(Some(name.to_owned()))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    names.sort();

    names
        .iter()
        .map(|name| load(&paths, name))
        .collect::<Result<Vec<_>>>()
}

pub(crate) fn remove(name: &str) -> Result<()> {
    validate_name(name)?;
    let paths = ProfilePaths::from_environment()?;
    let profile_path = paths.profile_path(name);
    std::fs::remove_file(&profile_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("corpus profile not found: {name}")
        } else {
            format!("failed to remove {}: {error}", profile_path.display())
        }
    })
}

fn load(paths: &ProfilePaths, name: &str) -> Result<CorpusProfile> {
    let profile_path = paths.profile_path(name);
    let contents = fs::read_to_string(&profile_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("corpus profile not found: {name}")
        } else {
            format!("failed to read {}: {error}", profile_path.display())
        }
    })?;
    let stored: StoredProfile = serde_json::from_str(&contents)
        .map_err(|error| format!("invalid corpus profile {}: {error}", profile_path.display()))?;

    Ok(CorpusProfile {
        name: name.to_owned(),
        root: stored.root,
        database: paths.database_path(name),
    })
}

fn validate_name(name: &str) -> Result<()> {
    let mut characters = name.chars();
    let starts_valid = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric());
    let remainder_valid = characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'));

    if starts_valid && remainder_valid {
        Ok(())
    } else {
        Err(format!(
            "invalid corpus name {name:?}; use ASCII letters, digits, '_' or '-', beginning with a letter or digit"
        ))
    }
}
fn mycelia_home(override_name: &str, xdg_name: &str, home_suffix: &str) -> Result<PathBuf> {
    if let Some(path) = env::var_os(override_name) {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os(xdg_name) {
        return Ok(PathBuf::from(path).join("mycelia"));
    }
    let home = env::var_os("HOME")
        .ok_or_else(|| format!("{override_name}, {xdg_name}, and HOME are unset"))?;
    Ok(PathBuf::from(home).join(home_suffix).join("mycelia"))
}

#[cfg(test)]
mod tests {
    use super::validate_name;

    #[test]
    fn validates_safe_profile_names() {
        for name in ["forge", "forge-2", "forge_local", "2forge"] {
            validate_name(name).expect("valid name");
        }
    }

    #[test]
    fn rejects_names_that_are_unsafe_as_filenames() {
        for name in ["", "../forge", ".forge", "forge/path", "for ge", "forgé"] {
            assert!(validate_name(name).is_err(), "{name:?} should fail");
        }
    }
}
