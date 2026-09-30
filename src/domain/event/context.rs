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
    use super::sanitize_remote;

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
