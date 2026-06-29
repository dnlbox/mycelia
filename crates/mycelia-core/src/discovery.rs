use std::path::{Component, Path, PathBuf};

use ignore::WalkBuilder;

use crate::{Error, Result};

pub(crate) struct Discovery {
    pub files: Vec<PathBuf>,
    pub rejected: usize,
}

pub(crate) fn discover(root: &Path) -> Result<Discovery> {
    if !root.is_dir() {
        return Err(Error::InvalidRoot(root.to_path_buf()));
    }

    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .follow_links(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(false)
        .ignore(true)
        .parents(true)
        .filter_entry(|entry| !contains_internal_metadata(entry.path()));

    let mut files = Vec::new();
    let mut rejected = 0;

    for entry in builder.build() {
        match entry {
            Ok(entry)
                if entry.file_type().is_some_and(|kind| kind.is_file())
                    && !is_evaluation_manifest(entry.path()) =>
            {
                files.push(entry.into_path());
            }
            Ok(_) => {}
            Err(_) => rejected += 1,
        }
    }

    files.sort();
    Ok(Discovery { files, rejected })
}

fn is_evaluation_manifest(path: &Path) -> bool {
    if path.extension().and_then(|value| value.to_str()) != Some("json") {
        return false;
    }

    let mut previous_was_fixtures = false;
    for component in path.components() {
        let Component::Normal(name) = component else {
            previous_was_fixtures = false;
            continue;
        };
        if previous_was_fixtures && name == "eval" {
            return true;
        }
        previous_was_fixtures = name == "fixtures";
    }
    false
}

fn contains_internal_metadata(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            Component::Normal(name)
                if name == ".git" || name == ".hg" || name == ".svn" || name == ".mycelia"
        )
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn discovers_sorted_files_and_respects_gitignore() {
        let directory = tempdir().expect("temp directory");
        fs::write(directory.path().join(".gitignore"), "ignored.txt\n").expect("write gitignore");
        fs::write(directory.path().join("z.txt"), "z").expect("write z");
        fs::write(directory.path().join("a.txt"), "a").expect("write a");
        fs::write(directory.path().join("ignored.txt"), "ignored").expect("write ignored");
        fs::create_dir(directory.path().join(".hidden")).expect("create hidden");
        fs::write(directory.path().join(".hidden/kept.txt"), "kept").expect("write hidden");
        fs::create_dir_all(directory.path().join("fixtures/eval")).expect("create eval fixtures");
        fs::write(
            directory.path().join("fixtures/eval/manifest.json"),
            r#"{"cases":[]}"#,
        )
        .expect("write eval manifest");
        fs::create_dir_all(directory.path().join("fixtures/smoke")).expect("create smoke fixtures");
        fs::write(directory.path().join("fixtures/smoke/v1.json"), "{}").expect("write smoke json");
        fs::create_dir(directory.path().join(".git")).expect("create git metadata");
        fs::write(directory.path().join(".git/config"), "secret").expect("write git config");
        fs::create_dir_all(directory.path().join(".mycelia/db")).expect("create mycelia state");
        fs::write(directory.path().join(".mycelia/AGENTS.md"), "internal")
            .expect("write mycelia guidance");
        fs::write(
            directory.path().join(".mycelia/db/index.sqlite3"),
            "internal",
        )
        .expect("write mycelia database");

        let discovery = discover(directory.path()).expect("discover");
        let relative: Vec<_> = discovery
            .files
            .iter()
            .map(|path| path.strip_prefix(directory.path()).expect("relative"))
            .collect();

        assert_eq!(
            relative,
            vec![
                Path::new(".gitignore"),
                Path::new(".hidden/kept.txt"),
                Path::new("a.txt"),
                Path::new("fixtures/smoke/v1.json"),
                Path::new("z.txt")
            ]
        );
        assert_eq!(discovery.rejected, 0);
    }

    #[test]
    fn rejects_invalid_root() {
        let directory = tempdir().expect("temp directory");
        let missing = directory.path().join("missing");

        assert!(matches!(
            discover(&missing),
            Err(Error::InvalidRoot(path)) if path == missing
        ));
    }
}
