//! Span scanning for the tokenizer: line ends, parenthesized bodies, backticks,
//! and heredoc bodies. Every function returns an index no greater than the input
//! length, including on unterminated input.

/// Index of the newline at or after `i`, or the end of input.
pub fn line_end(chars: &[char], i: usize) -> usize {
    let mut j = i;
    while let Some(&c) = chars.get(j) {
        if c == '\n' {
            return j;
        }
        j += 1;
    }
    chars.len()
}

/// End of the parenthesized body starting at `start` and the index after its `)`;
/// quotes and escapes inside it do not close it. Unterminated input runs to the end.
pub fn enclosed(chars: &[char], start: usize) -> (usize, usize) {
    let mut depth = 1_usize;
    let mut i = start;
    while let Some(&c) = chars.get(i) {
        match c {
            '\\' => i += 2,
            '\'' | '"' => {
                i += 1;
                while let Some(&inner) = chars.get(i) {
                    if inner == c {
                        break;
                    }
                    i += if inner == '\\' && c == '"' { 2 } else { 1 };
                }
                i += 1;
            }
            '(' => {
                depth += 1;
                i += 1;
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return (i, i + 1);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    (chars.len(), chars.len())
}

/// Index of the backtick closing a substitution that starts at `start`.
pub fn backtick_end(chars: &[char], start: usize) -> usize {
    let mut i = start;
    while let Some(&c) = chars.get(i) {
        match c {
            '\\' => i += 2,
            '`' => return i,
            _ => i += 1,
        }
    }
    chars.len()
}

/// Skip the heredoc bodies that begin at `i`, in order, returning the index after
/// the last delimiter line. `<<-` bodies may indent their delimiter with tabs.
pub fn skip_heredocs(chars: &[char], mut i: usize, delimiters: &[(String, bool)]) -> usize {
    for (delimiter, strip_tabs) in delimiters {
        loop {
            if i >= chars.len() {
                return chars.len();
            }
            let end = line_end(chars, i);
            let line: String = match chars.get(i..end) {
                Some(line) => line.iter().collect(),
                None => return chars.len(),
            };
            i = end + 1;
            let line = line.trim_end_matches('\r');
            let candidate = if *strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line
            };
            if candidate == delimiter {
                break;
            }
        }
    }
    i.min(chars.len())
}
