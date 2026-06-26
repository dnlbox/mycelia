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
        .filter_entry(|entry| !contains_vcs_metadata(entry.path()));

    let mut files = Vec::new();
    let mut rejected = 0;

    for entry in builder.build() {
        match entry {
            Ok(entry) if entry.file_type().is_some_and(|kind| kind.is_file()) => {
                files.push(entry.into_path());
            }
            Ok(_) => {}
            Err(_) => rejected += 1,
        }
    }

    files.sort();
    Ok(Discovery { files, rejected })
}

fn contains_vcs_metadata(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            Component::Normal(name)
                if name == ".git" || name == ".hg" || name == ".svn"
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
        fs::create_dir(directory.path().join(".git")).expect("create git metadata");
        fs::write(directory.path().join(".git/config"), "secret").expect("write git config");

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
