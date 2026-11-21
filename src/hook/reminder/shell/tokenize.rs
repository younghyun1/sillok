//! Quote-aware split of shell text into simple commands and redirection targets.
//!
//! An approximation of POSIX sh, bash, and fish syntax for classification only.
//! Command substitutions (`$(...)`, backticks, and fish's bare `(...)`) are split
//! as commands of their own, because `obj=$(sillok objective add ...)` is the usual
//! way a Sillok write appears. Heredoc bodies are skipped: they are data for the
//! receiving command, and a body that mentions `git push` must not count as one.

/// One simple command after quote removal.
#[derive(Debug, Default, PartialEq)]
pub struct Simple {
    /// Words in order, including leading assignments and wrapper commands.
    pub words: Vec<String>,
    /// Targets of `>`, `>>`, `>|`, and `&>` redirections.
    pub writes: Vec<String>,
}

use super::bounds;

/// Substitutions nested deeper than this are treated as plain words.
const MAX_DEPTH: usize = 8;

/// Where the word being read goes once it ends.
#[derive(Clone, Copy)]
enum Slot {
    Word,
    Write,
    /// Input redirection and here-string data, which name no command.
    Input,
    Heredoc {
        strip_tabs: bool,
    },
}

struct Splitter<'a> {
    out: &'a mut Vec<Simple>,
    depth: usize,
    current: Simple,
    word: String,
    /// A word is in progress; set by quotes too, so `""` is an empty word.
    started: bool,
    /// The word contained quotes, so a digit word before `>` is text, not a descriptor.
    quoted: bool,
    slot: Slot,
    /// Delimiters whose bodies start after the current line.
    heredocs: Vec<(String, bool)>,
}

/// Split shell text into simple commands, including those inside substitutions.
pub fn split(script: &str) -> Vec<Simple> {
    let chars: Vec<char> = script.chars().collect();
    let mut out = Vec::new();
    split_into(&chars, 0, &mut out);
    out
}

fn split_into(chars: &[char], depth: usize, out: &mut Vec<Simple>) {
    let mut splitter = Splitter {
        out,
        depth,
        current: Simple::default(),
        word: String::new(),
        started: false,
        quoted: false,
        slot: Slot::Word,
        heredocs: Vec::new(),
    };
    splitter.run(chars);
}

impl Splitter<'_> {
    fn run(&mut self, chars: &[char]) {
        let mut i = 0;
        while let Some(&c) = chars.get(i) {
            let next = chars.get(i + 1).copied();
            i = match (c, next) {
                ('\'', _) => self.single(chars, i + 1),
                ('"', _) => self.double(chars, i + 1),
                ('\\', Some('\n')) => i + 2,
                ('\\', Some(escaped)) => {
                    self.push(escaped);
                    i + 2
                }
                ('\\', None) => i + 1,
                ('$', Some('(')) => self.substitute(chars, i + 2),
                ('`', _) => self.backtick(chars, i + 1),
                ('#', _) if !self.started => bounds::line_end(chars, i),
                (' ' | '\t' | '\r', _) => {
                    self.finish_word();
                    i + 1
                }
                ('\n', _) => {
                    self.finish_command();
                    self.skip_heredocs(chars, i + 1)
                }
                (';' | '(' | ')', _) => {
                    self.finish_command();
                    i + 1
                }
                ('|', Some('|' | '&')) | ('&', Some('&')) => {
                    self.finish_command();
                    i + 2
                }
                ('|', _) => {
                    self.finish_command();
                    i + 1
                }
                ('&', Some('>')) => {
                    self.finish_word();
                    self.redirect_target(chars, i + 2)
                }
                ('&', _) => {
                    self.finish_command();
                    i + 1
                }
                ('>', _) => self.redirect(chars, i + 1),
                ('<', Some('<')) => self.here(chars, i + 2),
                ('<', Some('(')) => {
                    self.finish_word();
                    self.substitute(chars, i + 2)
                }
                ('<', _) => {
                    self.finish_word();
                    self.slot = Slot::Input;
                    i + 1
                }
                (other, _) => {
                    self.push(other);
                    i + 1
                }
            };
        }
        self.finish_command();
    }

    fn push(&mut self, c: char) {
        self.word.push(c);
        self.started = true;
    }

    fn single(&mut self, chars: &[char], mut i: usize) -> usize {
        self.started = true;
        self.quoted = true;
        while let Some(&c) = chars.get(i) {
            if c == '\'' {
                return i + 1;
            }
            self.word.push(c);
            i += 1;
        }
        i
    }

    fn double(&mut self, chars: &[char], mut i: usize) -> usize {
        self.started = true;
        self.quoted = true;
        while let Some(&c) = chars.get(i) {
            let next = chars.get(i + 1).copied();
            i = match (c, next) {
                ('"', _) => return i + 1,
                ('\\', Some('\n')) => i + 2,
                ('\\', Some(escaped @ ('"' | '\\' | '$' | '`'))) => {
                    self.word.push(escaped);
                    i + 2
                }
                ('$', Some('(')) => self.substitute(chars, i + 2),
                ('`', _) => self.backtick(chars, i + 1),
                (other, _) => {
                    self.word.push(other);
                    i + 1
                }
            };
        }
        i
    }

    /// Split a `$(...)`, `<(...)`, or `>(...)` body starting at `start`.
    fn substitute(&mut self, chars: &[char], start: usize) -> usize {
        let (end, resume) = bounds::enclosed(chars, start);
        self.nested(chars, start, end);
        resume
    }

    fn backtick(&mut self, chars: &[char], start: usize) -> usize {
        let end = bounds::backtick_end(chars, start);
        self.nested(chars, start, end);
        (end + 1).min(chars.len())
    }

    fn nested(&mut self, chars: &[char], start: usize, end: usize) {
        self.started = true;
        if self.depth >= MAX_DEPTH {
            return;
        }
        if let Some(inner) = chars.get(start..end) {
            split_into(inner, self.depth + 1, self.out)
        }
    }

    /// Handle the text after `>`: a descriptor duplication, a process substitution,
    /// or a file target.
    fn redirect(&mut self, chars: &[char], i: usize) -> usize {
        let descriptor = self.started
            && !self.quoted
            && !self.word.is_empty()
            && self.word.chars().all(|c| c.is_ascii_digit());
        if descriptor {
            // The `2` in `2>` names a descriptor; it is not a command word.
            self.word.clear();
            self.started = false;
        } else {
            self.finish_word();
        }
        match chars.get(i) {
            Some('&') => {
                // `>&2` and `>&-` duplicate or close descriptors and write no file.
                let mut j = i + 1;
                while matches!(chars.get(j), Some(c) if c.is_ascii_digit() || *c == '-') {
                    j += 1;
                }
                j
            }
            Some('(') => self.substitute(chars, i + 1),
            _ => self.redirect_target(chars, i),
        }
    }

    fn redirect_target(&mut self, chars: &[char], i: usize) -> usize {
        self.slot = Slot::Write;
        match chars.get(i) {
            Some('>' | '|') => i + 1,
            _ => i,
        }
    }

    /// Handle the text after `<<`: a here-string `<<<` or a heredoc delimiter.
    fn here(&mut self, chars: &[char], i: usize) -> usize {
        self.finish_word();
        match chars.get(i) {
            Some('<') => {
                self.slot = Slot::Input;
                i + 1
            }
            Some('-') => {
                self.slot = Slot::Heredoc { strip_tabs: true };
                i + 1
            }
            _ => {
                self.slot = Slot::Heredoc { strip_tabs: false };
                i
            }
        }
    }

    fn finish_word(&mut self) {
        if !self.started {
            return;
        }
        let word = std::mem::take(&mut self.word);
        match self.slot {
            Slot::Word => self.current.words.push(word),
            Slot::Write => self.current.writes.push(word),
            Slot::Input => (),
            Slot::Heredoc { strip_tabs } => self.heredocs.push((word, strip_tabs)),
        }
        self.slot = Slot::Word;
        self.started = false;
        self.quoted = false;
    }

    fn finish_command(&mut self) {
        self.finish_word();
        self.slot = Slot::Word;
        let simple = std::mem::take(&mut self.current);
        if !simple.words.is_empty() || !simple.writes.is_empty() {
            self.out.push(simple);
        }
    }

    fn skip_heredocs(&mut self, chars: &[char], i: usize) -> usize {
        bounds::skip_heredocs(chars, i, &std::mem::take(&mut self.heredocs))
    }
}
