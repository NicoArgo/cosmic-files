// SPDX-License-Identifier: GPL-3.0-only

//! POP Flow: which folder "Open in terminal" opens for a given item.
//!
//! - a folder: the folder itself;
//! - an executable (a binary, a script with +x, an AppImage): the root of the
//!   git repository it lives in, or its folder when it isn't in one;
//! - a `.desktop` launcher: its `Path=` when it has one, else the repository
//!   of the program it runs (symlinks followed, so `/usr/local/bin/x` linked
//!   into a checkout finds the checkout), else the launcher's folder;
//! - any other file: its folder.
//!
//! Plain std and no `git` process: the right-click menu asks this for every
//! selected item while it is being built.

use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

/// The folder a terminal opened "on" `path` should start in.
pub fn terminal_dir_for(path: &Path) -> Option<PathBuf> {
    let meta = fs::metadata(path).ok()?;
    if meta.is_dir() {
        return Some(path.to_path_buf());
    }
    let parent = path.parent()?.to_path_buf();
    if path.extension().is_some_and(|ext| ext == "desktop") {
        return Some(desktop_entry_dir(path).unwrap_or(parent));
    }
    if meta.is_file() && meta.permissions().mode() & 0o111 != 0 {
        return Some(repo_root(&parent).unwrap_or(parent));
    }
    Some(parent)
}

/// The folders to open for a selection: one per distinct folder, in order.
pub fn terminal_dirs_for<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    paths
        .into_iter()
        .filter_map(terminal_dir_for)
        .filter(|dir| seen.insert(dir.clone()))
        .collect()
}

/// The closest folder at or above `dir` holding `.git` (a folder, or a file
/// for worktrees and submodules).
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
}

/// For a launcher: its working folder, or the repository of what it runs.
fn desktop_entry_dir(path: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(path).ok()?;
    let (exec, work_dir) = desktop_entry_fields(&text);
    if let Some(dir) = work_dir.map(PathBuf::from).filter(|d| d.is_dir()) {
        return Some(dir);
    }
    let program = resolve_program(&exec?)?;
    let program = fs::canonicalize(&program).unwrap_or(program);
    repo_root(program.parent()?)
}

/// `Exec=` and `Path=` from the `[Desktop Entry]` group (not the actions).
fn desktop_entry_fields(text: &str) -> (Option<String>, Option<String>) {
    let mut in_entry = false;
    let (mut exec, mut work_dir) = (None, None);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(v) = line.strip_prefix("Exec=") {
                exec.get_or_insert_with(|| v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("Path=") {
                work_dir.get_or_insert_with(|| v.trim().to_string());
            }
        }
    }
    (exec, work_dir)
}

/// The program an `Exec=` line runs: first word, quotes removed, past an
/// `env VAR=value` prefix, looked up in `PATH` when it has no slash.
fn resolve_program(exec: &str) -> Option<PathBuf> {
    let program = exec_words(exec)
        .into_iter()
        .skip_while(|w| w == "env" || (w.contains('=') && !w.starts_with('/')))
        .next()?;
    if program.contains('/') {
        return Some(PathBuf::from(program));
    }
    std::env::var_os("PATH")?
        .to_str()?
        .split(':')
        .map(|dir| Path::new(dir).join(&program))
        .find(|p| p.is_file())
}

/// Splits an `Exec=` value into words, honoring double quotes.
fn exec_words(exec: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let (mut quoted, mut escaped) = (false, false);
    for c in exec.chars() {
        match c {
            _ if escaped => {
                word.push(c);
                escaped = false;
            }
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(repo.join("target/release")).unwrap();
        fs::create_dir_all(root.path().join("loose")).unwrap();
        for (file, mode) in [
            ("repo/target/release/app", 0o755),
            ("repo/README.md", 0o644),
            ("loose/run.sh", 0o755),
            ("loose/notes.txt", 0o644),
        ] {
            let p = root.path().join(file);
            fs::write(&p, "x").unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
        }
        root
    }

    #[test]
    fn folder_opens_itself() {
        let t = tree();
        let dir = t.path().join("repo/target");
        assert_eq!(terminal_dir_for(&dir), Some(dir));
    }

    #[test]
    fn executable_opens_its_repo_root() {
        let t = tree();
        let app = t.path().join("repo/target/release/app");
        assert_eq!(terminal_dir_for(&app), Some(t.path().join("repo")));
    }

    #[test]
    fn executable_outside_a_repo_opens_its_folder() {
        let t = tree();
        let script = t.path().join("loose/run.sh");
        assert_eq!(terminal_dir_for(&script), Some(t.path().join("loose")));
    }

    #[test]
    fn plain_file_opens_its_folder_even_in_a_repo() {
        let t = tree();
        let readme = t.path().join("repo/README.md");
        assert_eq!(terminal_dir_for(&readme), Some(t.path().join("repo")));
        let notes = t.path().join("loose/notes.txt");
        assert_eq!(terminal_dir_for(&notes), Some(t.path().join("loose")));
    }

    #[test]
    fn launcher_follows_its_program_into_the_repo() {
        let t = tree();
        let link = t.path().join("loose/app-link");
        std::os::unix::fs::symlink(t.path().join("repo/target/release/app"), &link).unwrap();
        let entry = t.path().join("loose/app.desktop");
        fs::write(
            &entry,
            format!(
                "[Desktop Entry]\nName=App\nExec=env A=1 \"{}\" %U\n\n[Desktop Action x]\nExec=/bin/false\n",
                link.display()
            ),
        )
        .unwrap();
        assert_eq!(terminal_dir_for(&entry), Some(t.path().join("repo")));
    }

    #[test]
    fn launcher_path_key_wins() {
        let t = tree();
        let entry = t.path().join("loose/app.desktop");
        let work = t.path().join("repo/target");
        fs::write(
            &entry,
            format!("[Desktop Entry]\nExec=/bin/true\nPath={}\n", work.display()),
        )
        .unwrap();
        assert_eq!(terminal_dir_for(&entry), Some(work));
    }

    #[test]
    fn launcher_without_repo_opens_its_folder() {
        let t = tree();
        let entry = t.path().join("loose/app.desktop");
        fs::write(&entry, "[Desktop Entry]\nExec=/bin/true\n").unwrap();
        assert_eq!(terminal_dir_for(&entry), Some(t.path().join("loose")));
    }

    #[test]
    fn selection_collapses_to_distinct_folders() {
        let t = tree();
        let a = t.path().join("loose/run.sh");
        let b = t.path().join("loose/notes.txt");
        let c = t.path().join("repo/target/release/app");
        let dirs = terminal_dirs_for([a.as_path(), b.as_path(), c.as_path()]);
        assert_eq!(dirs, vec![t.path().join("loose"), t.path().join("repo")]);
    }
}
