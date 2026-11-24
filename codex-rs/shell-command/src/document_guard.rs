//! WitLex (D205): finds a shell command that reads a document, or a table or database outside a
//! git work tree, itself — so that `exec_command` can decline it and name WitLex's reader instead.
//!
//! Why: WitLex reads case files (PDF, OFD, Office and WPS files, bank statements as CSV …) only
//! through its own readers, which keep pages, OCR and the Chinese text and give citable places.
//! Instructions alone did not stop qwen3.8-flash from peeking with `head` / `iconv` / `sqlite3`
//! first (D204's eval: two rewordings, no improvement), so the engine declines the command.
//!
//! The check is deliberately narrow:
//! - only programs that read a file's contents (`cat`, `head`, `iconv`, `python3` …), never
//!   `ls`, `file`, `wc`, `stat`, `cp`, `mv` or `open`;
//! - documents always; tables and databases only outside a git work tree (a developer's
//!   repository keeps its normal shell);
//! - WitLex's export copies (`name.export.csv`) are always allowed: they exist for the shell;
//! - a file named only as an output (`> out.csv`) is not a read, nor is a path that does not
//!   exist yet, nor a quoted path given only to an obvious write in code (`df.to_csv('x.csv')`,
//!   `wb.save('报告.xlsx')`, `open('x.csv', 'w')`, `writeFileSync('x.csv', …)`): making files is
//!   what the model should do.
//!
//! `parse_command` is not used for this: it collapses any pipeline with a program it does not
//! know (`iconv … | grep …`) into `Unknown`, which are exactly the commands to find.

use once_cell::sync::Lazy;
use regex::Regex;
use std::path::Path;
use std::path::PathBuf;

/// What a declined command read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRead {
    /// The path as the command named it.
    pub named: String,
    /// The path resolved against the working directory.
    pub path: PathBuf,
    pub kind: DocumentKind,
    /// WitLex's reader for this kind, e.g. `sheet.file`.
    pub reader: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    Document,
    Table,
    Database,
}

impl DocumentRead {
    /// The one sentence the model gets in place of the command's output.
    pub fn refusal(&self) -> String {
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.named.clone());
        let kind = match self.kind {
            DocumentKind::Document => "a document",
            DocumentKind::Table => "a table",
            DocumentKind::Database => "a database",
        };
        format!(
            "WitLex blocked this command: {name} is {kind} — read it with {}(path); its export() writes a copy this shell may process.",
            self.reader
        )
    }
}

/// Programs that read a file's contents (the eval's list, D204, without the media tools and
/// without `tar` / `7z`, which also pack case files the person asked to send).
const READERS: &[&str] = &[
    "cat",
    "head",
    "tail",
    "less",
    "more",
    "bat",
    "nl",
    "iconv",
    "awk",
    "gawk",
    "mawk",
    "nawk",
    "sed",
    "gsed",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "ag",
    "cut",
    "tr",
    "sort",
    "uniq",
    "paste",
    "join",
    "column",
    "strings",
    "xxd",
    "hexdump",
    "od",
    "python",
    "python3",
    "perl",
    "ruby",
    "node",
    "jq",
    "sqlite3",
    "duckdb",
    "textutil",
    "pdftotext",
    "qlmanage",
    "unzip",
    "csvlook",
    "in2csv",
    "csvcut",
    "csvstat",
    "xsv",
];

/// Readers whose heredoc is a program (its quoted paths are files it opens).
const INTERPRETERS: &[&str] = &[
    "python", "python3", "perl", "ruby", "node", "sqlite3", "duckdb",
];

/// Words that run the next word as the command.
const WRAPPERS: &[&str] = &[
    "env", "command", "builtin", "exec", "time", "nice", "nohup", "sudo", "stdbuf", "timeout",
];

const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash", "ksh"];

fn kind_of(ext: &str) -> Option<(DocumentKind, &'static str)> {
    let ext = ext.to_ascii_lowercase();
    Some(match ext.as_str() {
        "pdf" => (DocumentKind::Document, "pdf.file"),
        "ofd" => (DocumentKind::Document, "ofd.file"),
        "doc" | "docx" | "wps" | "rtf" | "odt" => (DocumentKind::Document, "word.file"),
        "xls" | "xlsx" | "xlsm" | "et" | "ods" => (DocumentKind::Document, "sheet.file"),
        "ppt" | "pptx" | "dps" | "odp" => (DocumentKind::Document, "slides.file"),
        // Not `.key`: on a Mac with code it is far more often a private key than Keynote.
        "pages" | "numbers" => (DocumentKind::Document, "iwork.file"),
        "eml" | "msg" | "mbox" => (DocumentKind::Document, "mail.file"),
        "epub" => (DocumentKind::Document, "book.file"),
        "csv" | "tsv" => (DocumentKind::Table, "sheet.file"),
        "parquet" => (DocumentKind::Table, "parquet.file"),
        "sqlite" | "sqlite3" | "db" => (DocumentKind::Database, "sqlite.file"),
        _ => return None,
    })
}

const EXTENSIONS: &str = "pdf|ofd|docx?|wps|rtf|odt|xlsx|xlsm|xls|et|ods|pptx?|dps|odp|pages|numbers|eml|msg|mbox|epub|csv|tsv|parquet|sqlite3?|db";

/// A quoted literal inside code (a `-c` script, a heredoc): `open('/x/流水.csv')`.
static QUOTED_PATH: Lazy<Option<Regex>> = Lazy::new(|| {
    Regex::new(&format!(
        r#"(?i)'([^'\n]*?\.(?:{EXTENSIONS}))'|"([^"\n]*?\.(?:{EXTENSIONS}))""#
    ))
    .ok()
});
/// A bare path inside code: `cat /x/流水.csv`, `--file=a.csv`.
static BARE_PATH: Lazy<Option<Regex>> = Lazy::new(|| {
    Regex::new(&format!(
        r#"(?i)(?:^|[\s=(,\[])([^\s'"(),=\[\]<>|;&]+\.(?:{EXTENSIONS}))(?:[^A-Za-z0-9_]|$)"#
    ))
    .ok()
});

/// Calls in code whose first argument is a file written, not read.
const WRITE_CALLS: &[&str] = &[
    "to_csv(",
    "to_excel(",
    "to_parquet(",
    ".save(",
    "writeFileSync(",
    "writeFile(",
    "createWriteStream(",
    "ExcelWriter(",
    "Workbook(",
];
/// What follows a path given to `open(` in write or append mode: `, 'w'`, `, mode="a"`, `, 'wb'`.
static OPEN_FOR_WRITING: Lazy<Option<Regex>> =
    Lazy::new(|| Regex::new(r#"^\s*,\s*(?:mode\s*=\s*)?['"][rbt+]*[wax]"#).ok());

#[derive(Debug, Default)]
struct Segment {
    /// The words, quotes removed; words that are only an output target are left out.
    words: Vec<String>,
    /// Heredoc bodies fed to this command.
    heredocs: Vec<String>,
}

/// Splits a script at unquoted `;`, `&`, `|`, newlines, parentheses and backticks into simple
/// commands, keeping quoted words whole and heredoc bodies with their command.
fn segments(script: &str) -> Vec<Segment> {
    let chars: Vec<char> = script.chars().collect();
    let mut out: Vec<Segment> = Vec::new();
    let mut current = Segment::default();
    let mut word = String::new();
    let mut in_word = false;
    let mut output_next = false;
    // Heredocs waiting for the next newline: (segment index once pushed, delimiter, strip tabs).
    let mut pending: Vec<(usize, String, bool)> = Vec::new();
    let mut i = 0;

    fn end_word(
        word: &mut String,
        in_word: &mut bool,
        output_next: &mut bool,
        current: &mut Segment,
    ) {
        if *in_word {
            if *output_next {
                *output_next = false;
            } else {
                current.words.push(std::mem::take(word));
            }
        }
        word.clear();
        *in_word = false;
    }

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' => {
                in_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    word.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '"' => {
                in_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\'
                        && i + 1 < chars.len()
                        && matches!(chars[i + 1], '"' | '\\' | '$' | '`')
                    {
                        i += 1;
                    }
                    word.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '\\' => {
                if i + 1 < chars.len() && chars[i + 1] != '\n' {
                    word.push(chars[i + 1]);
                    in_word = true;
                }
                i += 2;
            }
            '#' if !in_word => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ' ' | '\t' => {
                end_word(&mut word, &mut in_word, &mut output_next, &mut current);
                i += 1;
            }
            '>' => {
                // `2>`, `&>`: the fd before it is not a word.
                if in_word && (word.chars().all(|ch| ch.is_ascii_digit()) || word == "&") {
                    word.clear();
                    in_word = false;
                }
                end_word(&mut word, &mut in_word, &mut output_next, &mut current);
                i += 1;
                while i < chars.len() && matches!(chars[i], '>' | '|') {
                    i += 1;
                }
                if i < chars.len() && chars[i] == '&' {
                    // `>&2`, `>&-`: a descriptor, not a file.
                    i += 1;
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '-') {
                        i += 1;
                    }
                } else {
                    output_next = true;
                }
            }
            '<' if i + 1 < chars.len()
                && chars[i + 1] == '<'
                && !(i + 2 < chars.len() && chars[i + 2] == '<') =>
            {
                end_word(&mut word, &mut in_word, &mut output_next, &mut current);
                i += 2;
                let strip_tabs = i < chars.len() && chars[i] == '-';
                if strip_tabs {
                    i += 1;
                }
                while i < chars.len() && matches!(chars[i], ' ' | '\t') {
                    i += 1;
                }
                let mut delimiter = String::new();
                while i < chars.len()
                    && !matches!(
                        chars[i],
                        ' ' | '\t' | '\n' | ';' | '|' | '&' | ')' | '<' | '>'
                    )
                {
                    if !matches!(chars[i], '\'' | '"' | '\\') {
                        delimiter.push(chars[i]);
                    }
                    i += 1;
                }
                if !delimiter.is_empty() {
                    pending.push((out.len(), delimiter, strip_tabs));
                }
            }
            '<' => {
                // `<<<` (a here-string) and `<` (an input file): the next word is read.
                end_word(&mut word, &mut in_word, &mut output_next, &mut current);
                i += 1;
                while i < chars.len() && chars[i] == '<' {
                    i += 1;
                }
            }
            '$' if i + 1 < chars.len() && chars[i + 1] == '(' => {
                end_word(&mut word, &mut in_word, &mut output_next, &mut current);
                out.push(std::mem::take(&mut current));
                i += 2;
            }
            '\n' | ';' | '&' | '|' | '(' | ')' | '`' => {
                end_word(&mut word, &mut in_word, &mut output_next, &mut current);
                output_next = false;
                out.push(std::mem::take(&mut current));
                i += 1;
                if c == '\n' && !pending.is_empty() {
                    i = read_heredocs(&chars, i, &mut pending, &mut out);
                }
            }
            _ => {
                word.push(c);
                in_word = true;
                i += 1;
            }
        }
    }
    end_word(&mut word, &mut in_word, &mut output_next, &mut current);
    out.push(current);
    // A heredoc left open at the end of the script: its segment keeps nothing more.
    out.retain(|s| !s.words.is_empty() || !s.heredocs.is_empty());
    out
}

/// Reads the bodies of the pending heredocs from `start` (the line after their command) and
/// attaches each to its segment. Returns where the script goes on.
fn read_heredocs(
    chars: &[char],
    start: usize,
    pending: &mut Vec<(usize, String, bool)>,
    out: &mut [Segment],
) -> usize {
    let mut i = start;
    for (segment, delimiter, strip_tabs) in pending.drain(..) {
        let mut body = String::new();
        loop {
            if i >= chars.len() {
                break;
            }
            let line_end = chars[i..]
                .iter()
                .position(|&ch| ch == '\n')
                .map_or(chars.len(), |p| i + p);
            let line: String = chars[i..line_end].iter().collect();
            i = (line_end + 1).min(chars.len());
            let compared = if strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line.as_str()
            };
            if compared.trim_end() == delimiter {
                break;
            }
            body.push_str(&line);
            body.push('\n');
        }
        if let Some(target) = out.get_mut(segment) {
            target.heredocs.push(body);
        }
    }
    i
}

fn program_name(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn is_reader(program: &str) -> bool {
    if READERS.contains(&program) {
        return true;
    }
    // python3.12, python2
    program.strip_prefix("python").is_some_and(|rest| {
        !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
    })
}

fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && !name.starts_with(|ch: char| ch.is_ascii_digit())
}

/// The command's words from its program on (assignments and wrappers like `env`, `sudo` dropped).
fn command_words(words: &[String]) -> &[String] {
    let mut start = 0;
    while start < words.len() {
        let word = words[start].as_str();
        if is_assignment(word) {
            start += 1;
            continue;
        }
        let program = program_name(word);
        if WRAPPERS.contains(&program) {
            start += 1;
            while start < words.len()
                && (words[start].starts_with('-')
                    || is_assignment(&words[start])
                    || (program == "timeout"
                        && words[start].starts_with(|ch: char| ch.is_ascii_digit())))
            {
                start += 1;
            }
            continue;
        }
        break;
    }
    &words[start..]
}

fn resolve(named: &str, cwd: &Path, home: Option<&Path>) -> PathBuf {
    if let Some(rest) = named.strip_prefix("~/")
        && let Some(home) = home
    {
        return home.join(rest);
    }
    let path = Path::new(named);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// The paths of guarded kinds a command's text names.
fn mentions(words: &[String], heredocs: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    let ends_with_kind = |text: &str| {
        text.rsplit_once('.').is_some_and(|(stem, ext)| {
            !stem.is_empty() && !stem.ends_with('/') && kind_of(ext).is_some()
        })
    };
    for word in words {
        let trimmed = word.trim();
        if ends_with_kind(trimmed) && !trimmed.contains(['\n', '\'', '"', ';']) {
            // A whole word: spaces and Chinese names kept (`'/x/my file.csv'`).
            found.push(
                trimmed
                    .trim_start_matches("--")
                    .rsplit('=')
                    .next()
                    .unwrap_or(trimmed)
                    .to_string(),
            );
        } else {
            scan(trimmed, &mut found);
        }
    }
    for body in heredocs {
        scan(body, &mut found);
    }
    found
}

/// Whether the quoted literal at `start..end` of `text` is only the file a call writes.
fn written(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].trim_end();
    if WRITE_CALLS.iter().any(|call| before.ends_with(call)) {
        return true;
    }
    before.ends_with("open(")
        && OPEN_FOR_WRITING
            .as_ref()
            .is_some_and(|re| re.is_match(&text[end..]))
}

/// Paths of guarded kinds inside code: quoted literals (not those only written), then bare words
/// outside them.
fn scan(text: &str, found: &mut Vec<String>) {
    let mut rest = text.to_string();
    if let Some(re) = QUOTED_PATH.as_ref() {
        for caps in re.captures_iter(text) {
            let (Some(whole), Some(m)) = (caps.get(0), caps.get(1).or_else(|| caps.get(2))) else {
                continue;
            };
            if !written(text, whole.start(), whole.end()) {
                found.push(m.as_str().to_string());
            }
            // The bare-word pass must not find the same literal again (or a piece of it).
            rest.replace_range(whole.range(), &" ".repeat(whole.len()));
        }
    }
    let text = rest.as_str();
    if let Some(re) = BARE_PATH.as_ref() {
        for caps in re.captures_iter(text) {
            if let Some(m) = caps.get(1) {
                found.push(m.as_str().to_string());
            }
        }
    }
}

/// The first read of a guarded file in a shell script, or `None`.
///
/// `exists` tells whether a path exists (one that does not is made, not read); `in_git_tree`
/// whether it lies in a git work tree (tables and databases there are a developer's own and stay
/// readable).
pub fn document_read_in_script(
    script: &str,
    cwd: &Path,
    home: Option<&Path>,
    probe: &Probe<'_>,
) -> Option<DocumentRead> {
    document_read_depth(script, cwd, home, probe, 0)
}

/// What the guard asks of the file system.
pub struct Probe<'a> {
    pub exists: &'a dyn Fn(&Path) -> bool,
    pub in_git_tree: &'a dyn Fn(&Path) -> bool,
}

fn document_read_depth(
    script: &str,
    cwd: &Path,
    home: Option<&Path>,
    probe: &Probe<'_>,
    depth: usize,
) -> Option<DocumentRead> {
    if depth > 4 {
        return None;
    }
    let mut cwd = cwd.to_path_buf();
    for segment in segments(script) {
        let words = command_words(&segment.words);
        let Some(first) = words.first() else {
            continue;
        };
        let program = program_name(first);
        if program == "cd" {
            cwd = match words.get(1) {
                Some(dir) if dir != "-" => resolve(dir, &cwd, home),
                Some(_) => cwd,
                None => home.map(Path::to_path_buf).unwrap_or(cwd),
            };
            continue;
        }
        if SHELLS.contains(&program) {
            // `bash <<EOF`: the heredoc is the script.
            for body in &segment.heredocs {
                if let Some(found) = document_read_depth(body, &cwd, home, probe, depth + 1) {
                    return Some(found);
                }
            }
            // `bash -lc "…"`: the script is the word after the flag that holds `c`.
            if let Some(flag) = words
                .iter()
                .position(|w| w.starts_with('-') && !w.starts_with("--") && w.contains('c'))
                && let Some(inner) = words.get(flag + 1)
                && let Some(found) = document_read_depth(inner, &cwd, home, probe, depth + 1)
            {
                return Some(found);
            }
            continue;
        }
        if !is_reader(program) {
            continue;
        }
        // A heredoc is code only for an interpreter; for `cat > x <<EOF` it is the text written.
        let code: &[String] = if INTERPRETERS.contains(&program) || program.starts_with("python") {
            &segment.heredocs
        } else {
            &[]
        };
        for named in mentions(&words[1..], code) {
            let base = named.rsplit('/').next().unwrap_or(&named);
            // An export copy, also as WitLex renames one beside an older copy (`a.export 2.csv`).
            if base.contains(".export.") || base.contains(".export ") {
                continue;
            }
            let Some((_, ext)) = base.rsplit_once('.') else {
                continue;
            };
            let Some((kind, reader)) = kind_of(ext) else {
                continue;
            };
            let path = resolve(&named, &cwd, home);
            if !(probe.exists)(&path)
                || (kind != DocumentKind::Document && (probe.in_git_tree)(&path))
            {
                continue;
            }
            return Some(DocumentRead {
                named,
                path,
                kind,
                reader,
            });
        }
    }
    None
}

/// As [`document_read_in_script`] for a command's argv (`["zsh", "-lc", "…"]`, or a program and
/// its arguments).
pub fn document_read_in_command(
    command: &[String],
    cwd: &Path,
    home: Option<&Path>,
    probe: &Probe<'_>,
) -> Option<DocumentRead> {
    if let Some((_, script)) = crate::bash::extract_bash_command(command) {
        return document_read_in_script(script, cwd, home, probe);
    }
    let joined = crate::parse_command::shlex_join(command);
    document_read_in_script(&joined, cwd, home, probe)
}

#[cfg(test)]
#[path = "document_guard_tests.rs"]
mod tests;
