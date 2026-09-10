//! Shared helpers for the integration suites. Each file in `tests/` is its own
//! crate, so this module is pulled in with `mod common;` where it is needed.

use std::path::Path;

/// A `file://` URL for a repository on disk, well-formed on every OS.
///
/// `repo_name` splits addresses on `/` only -- a backslash is a legal filename
/// character on Unix, so it must not be a separator -- and a raw backslash inside
/// a JSON string is an invalid escape. So `format!("file://{}", path)` is wrong
/// on Windows twice over: it installs under one mangled directory name instead
/// of the repository name, and any mock or index carrying it fails to parse.
/// Forward slashes plus the three-slash form (`file:///C:/...`) keep the last
/// segment the repository name on Windows and Unix alike; on Unix the string
/// produced is `file://` + an absolute path, exactly as before.
pub fn file_url(repo: &Path) -> String {
    let repo_str = repo
        .to_str()
        .expect("test path is UTF-8")
        .replace('\\', "/");
    if repo_str.starts_with('/') {
        format!("file://{repo_str}")
    } else {
        format!("file:///{repo_str}")
    }
}
