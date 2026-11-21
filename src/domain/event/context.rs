//! Working context captured with each event.

use serde::{Deserialize, Serialize};

/// Where an event was recorded: directory, Git state, and optional agent session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_remote: Option<String>,
    /// Agent session label from `SILLOK_SESSION`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

impl WorkContext {
    /// Grouping key: the repository root, else the working directory.
    pub fn key(&self) -> Option<&str> {
        match &self.git_root {
            Some(root) => Some(root.as_str()),
            None => self.cwd.as_deref(),
        }
    }
}

/// The repository `status` is asked about, as seen from the working directory.
///
/// Work recorded in another worktree or clone of the same repository has a
/// different root path, so a root alone would hide it; the main checkout's
/// root and the remote URL tie those contexts together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    /// Repository root, else the working directory.
    pub root: String,
    /// Root of the main checkout when `root` is a linked worktree.
    pub main_root: Option<String>,
    /// `origin` URL, sanitized like the one stored with each event.
    pub remote: Option<String>,
    /// Every stored spelling of that remote (SSH, HTTPS, with or without
    /// `.git`), filled in from the store before querying.
    pub remotes: Vec<String>,
}

/// Canonical identity of a Git remote: `host/path`, with the scheme, login,
/// port, trailing slash, and `.git` suffix removed and the host lowercased.
///
/// Two machines often reach one repository through different URLs
/// (`git@github.com:o/r.git` and `https://github.com/o/r`); both map to
/// `github.com/o/r`. A local path maps to itself.
pub fn remote_key(raw: &str) -> String {
    let trimmed = raw.trim();
    let (host, path) = match trimmed.find("://") {
        Some(index) => {
            let rest = &trimmed[index + 3..];
            if trimmed[..index].eq_ignore_ascii_case("file") {
                return clean_path(rest).to_string();
            }
            let (authority, path) = match rest.find('/') {
                Some(slash) => (&rest[..slash], &rest[slash + 1..]),
                None => (rest, ""),
            };
            let host = match authority.rsplit('@').next() {
                Some(value) => value,
                None => authority,
            };
            // Drop a port; a bracketed IPv6 literal keeps its colons.
            let host = match host.starts_with('[') {
                true => host,
                false => match host.split(':').next() {
                    Some(value) => value,
                    None => host,
                },
            };
            (host, path)
        }
        // SCP style, `[user@]host:path`. A single letter before the colon is
        // a Windows drive, and a slash before it means a plain path.
        None => match trimmed.split_once(':') {
            Some((before, after)) if before.len() > 1 && !before.contains('/') => {
                let host = match before.rsplit('@').next() {
                    Some(value) => value,
                    None => before,
                };
                (host, after)
            }
            Some(_) | None => return clean_path(trimmed).to_string(),
        },
    };
    format!(
        "{}/{}",
        host.to_ascii_lowercase(),
        clean_path(path).trim_start_matches('/')
    )
}

fn clean_path(path: &str) -> &str {
    let path = path.trim_end_matches('/');
    match path.strip_suffix(".git") {
        Some(stripped) => stripped.trim_end_matches('/'),
        None => path,
    }
}

/// Removes `user[:password]@` from URL-style Git remotes.
///
/// Remote URLs are copied into every event and pushed to the sync remote, so
/// an embedded token would leak. SCP-style remotes (`git@host:path`) carry a
/// login name, not a secret, and are kept as they are.
pub fn sanitize_remote(raw: &str) -> String {
    let scheme_end = match raw.find("://") {
        Some(index) => index,
        None => return raw.to_string(),
    };
    let authority_start = scheme_end + 3;
    let rest = &raw[authority_start..];
    let authority_len = match rest.find('/') {
        Some(index) => index,
        None => rest.len(),
    };
    match rest[..authority_len].rfind('@') {
        Some(at) => format!("{}{}", &raw[..authority_start], &rest[at + 1..]),
        None => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{remote_key, sanitize_remote};

    #[test]
    fn spellings_of_one_remote_share_a_key() {
        for url in [
            "git@github.com:Owner/Repo.git",
            "ssh://git@github.com/Owner/Repo.git",
            "ssh://git@GitHub.com:22/Owner/Repo",
            "https://github.com/Owner/Repo.git/",
            "https://github.com/Owner/Repo",
            "git://github.com/Owner/Repo.git",
        ] {
            assert_eq!(remote_key(url), "github.com/Owner/Repo", "{url}");
        }
        assert_ne!(
            remote_key("https://github.com/Owner/Other"),
            "github.com/Owner/Repo"
        );
        assert_ne!(
            remote_key("https://gitlab.com/Owner/Repo"),
            "github.com/Owner/Repo"
        );
    }

    #[test]
    fn local_remotes_keep_their_path() {
        assert_eq!(remote_key("/srv/git/repo.git"), "/srv/git/repo");
        assert_eq!(remote_key("file:///srv/git/repo.git"), "/srv/git/repo");
        assert_eq!(remote_key("C:/git/repo.git"), "C:/git/repo");
        assert_eq!(remote_key("../repo"), "../repo");
    }

    #[test]
    fn strips_https_credentials() {
        assert_eq!(
            sanitize_remote("https://user:ghp_secret@github.com/a/b.git"),
            "https://github.com/a/b.git"
        );
        assert_eq!(
            sanitize_remote("https://token@example.com"),
            "https://example.com"
        );
    }

    #[test]
    fn keeps_clean_and_scp_remotes() {
        assert_eq!(
            sanitize_remote("git@github.com:a/b.git"),
            "git@github.com:a/b.git"
        );
        assert_eq!(
            sanitize_remote("https://github.com/a/b.git"),
            "https://github.com/a/b.git"
        );
        assert_eq!(
            sanitize_remote("ssh://git@host/path@x"),
            "ssh://host/path@x"
        );
    }
}
