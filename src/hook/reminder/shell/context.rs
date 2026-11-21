//! Working directory and variables a script sets for its own later commands.
//!
//! Scripts commonly name a scratch directory once and write through it, as in
//! `W=/tmp/x; cmd > $W/out.json`, or `cd` into it first. Following those two forms
//! keeps scratch writes from counting as work. Values are tracked textually; nothing
//! is evaluated, and command substitutions in values stay unexpanded.

use std::path::{Path, PathBuf};

/// Variables tracked per script; later assignments past this are ignored.
const MAX_VARS: usize = 32;

#[derive(Clone, Debug)]
pub struct Context {
    pub cwd: Option<PathBuf>,
    vars: Vec<(String, String)>,
}

impl Context {
    pub fn new(cwd: Option<&Path>) -> Self {
        Context {
            cwd: cwd.map(Path::to_path_buf),
            vars: Vec::new(),
        }
    }

    /// Replace `$NAME` and `${NAME}` with values this script assigned; unknown
    /// variables stay literal, which `paths::written` treats as unresolvable.
    pub fn expand(&self, word: &str) -> String {
        if !word.contains('$') {
            return word.to_owned();
        }
        let mut out = String::with_capacity(word.len());
        let mut rest = word;
        while let Some(index) = rest.find('$') {
            let (before, reference) = rest.split_at(index);
            out.push_str(before);
            let after = match reference.strip_prefix('$') {
                Some(after) => after,
                None => {
                    rest = reference;
                    break;
                }
            };
            let (name, tail) = match after.strip_prefix('{') {
                Some(braced) => match braced.split_once('}') {
                    Some((name, tail)) => (name, tail),
                    None => ("", after),
                },
                None => {
                    let end = match after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                        Some(end) => end,
                        None => after.len(),
                    };
                    after.split_at(end)
                }
            };
            match self
                .vars
                .iter()
                .find(|(known, _)| !name.is_empty() && known == name)
            {
                Some((_, value)) => out.push_str(value),
                None => {
                    out.push('$');
                    match after.strip_suffix(tail) {
                        Some(reference) => out.push_str(reference),
                        None => out.push_str(after),
                    }
                }
            }
            rest = tail;
        }
        out.push_str(rest);
        out
    }

    /// Track `NAME=value` commands, `export`, fish `set`, `cd`, and `pushd`.
    /// `words` are already expanded.
    pub fn observe(&mut self, words: &[String]) {
        let (first, args) = match words.split_first() {
            Some(split) => split,
            None => return,
        };
        if words.iter().all(|word| assignment(word).is_some()) {
            for word in words {
                if let Some((name, value)) = assignment(word) {
                    self.set(name, value);
                }
            }
            return;
        }
        match first.as_str() {
            "export" => {
                for word in args {
                    if let Some((name, value)) = assignment(word) {
                        self.set(name, value);
                    }
                }
            }
            "set" => {
                let mut operands = args.iter().skip_while(|word| word.starts_with('-'));
                match (operands.next(), operands.next()) {
                    (Some(name), Some(value))
                        if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
                    {
                        self.set(name, value)
                    }
                    _ => (),
                }
            }
            "cd" | "pushd" => {
                let target = args.iter().find(|word| !word.starts_with('-'));
                self.cwd = match target {
                    None => std::env::var_os("HOME").map(PathBuf::from),
                    Some(dir) if dir.contains('$') => None,
                    Some(dir) => {
                        let home = std::env::var_os("HOME").map(PathBuf::from);
                        match (dir.strip_prefix('~'), home) {
                            (Some(rest), Some(home)) => {
                                Some(home.join(rest.trim_start_matches('/')))
                            }
                            (Some(_), None) => None,
                            (None, _) if Path::new(dir).is_absolute() => Some(PathBuf::from(dir)),
                            (None, _) => self.cwd.as_ref().map(|cwd| cwd.join(dir)),
                        }
                    }
                };
            }
            _ => (),
        }
    }

    fn set(&mut self, name: &str, value: &str) {
        let full = self.vars.len() >= MAX_VARS;
        match self.vars.iter().position(|(known, _)| known == name) {
            Some(index) => {
                if let Some(entry) = self.vars.get_mut(index) {
                    entry.1 = value.to_owned();
                }
            }
            None if !full => self.vars.push((name.to_owned(), value.to_owned())),
            None => (),
        }
    }
}

/// Split `NAME=value` into its name and value.
pub fn assignment(word: &str) -> Option<(&str, &str)> {
    let (name, value) = match word.split_once('=') {
        Some(parts) => parts,
        None => return None,
    };
    let mut chars = name.chars();
    match chars.next() {
        Some(first)
            if (first.is_ascii_alphabetic() || first == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_') =>
        {
            Some((name, value))
        }
        Some(_) | None => None,
    }
}
