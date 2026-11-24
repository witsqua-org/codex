use super::*;
use pretty_assertions::assert_eq;
use std::path::Path;

const CWD: &str = "/Users/someone/Documents/WitLex/2026-11-25/chat";
const HOME: &str = "/Users/someone";

fn never_git(_: &Path) -> bool {
    false
}

fn always(_: &Path) -> bool {
    true
}

/// Every named file exists; nothing is in a git work tree.
const CASE: Probe<'static> = Probe {
    exists: &always,
    in_git_tree: &never_git,
};

fn check(script: &str) -> Option<DocumentRead> {
    document_read_in_script(script, Path::new(CWD), Some(Path::new(HOME)), &CASE)
}

fn blocked(script: &str) -> String {
    match check(script) {
        Some(read) => read.path.to_string_lossy().into_owned(),
        None => panic!("expected `{script}` to be declined"),
    }
}

fn allowed(script: &str) {
    assert_eq!(check(script), None, "expected `{script}` to run");
}

#[test]
fn a_peek_at_a_table_is_declined() {
    assert_eq!(
        blocked("head -5 /tmp/case/对公流水.csv; echo ---; wc -l /tmp/case/对公流水.csv"),
        "/tmp/case/对公流水.csv"
    );
    assert_eq!(
        blocked("iconv -f GBK -t UTF-8 交易明细.csv | cat -n"),
        format!("{CWD}/交易明细.csv")
    );
    assert_eq!(
        blocked(
            "ls -la /tmp/x && file /tmp/x/交易明细.csv && iconv -f GB18030 -t UTF-8 /tmp/x/交易明细.csv | head -5"
        ),
        "/tmp/x/交易明细.csv"
    );
    assert_eq!(blocked("tail -n 20 data.tsv"), format!("{CWD}/data.tsv"));
    assert_eq!(
        blocked("awk -F, '{s+=$4} END {print s}' 流水.csv"),
        format!("{CWD}/流水.csv")
    );
    assert_eq!(
        blocked("sed -n '210,218p' 流水.csv"),
        format!("{CWD}/流水.csv")
    );
    assert_eq!(
        blocked("grep -n 王某某 流水.csv"),
        format!("{CWD}/流水.csv")
    );
    assert_eq!(
        blocked("sort < 流水.csv | uniq -c"),
        format!("{CWD}/流水.csv")
    );
}

#[test]
fn a_database_and_parquet_are_declined() {
    assert_eq!(
        blocked("sqlite3 /tmp/x/cases.sqlite \".tables\""),
        "/tmp/x/cases.sqlite"
    );
    assert_eq!(
        blocked("sqlite3 -header -column cases.db 'SELECT rowid, * FROM 案件;'"),
        format!("{CWD}/cases.db")
    );
    assert_eq!(
        blocked("duckdb -c \"select * from 'rows.parquet' limit 5\""),
        format!("{CWD}/rows.parquet")
    );
}

#[test]
fn documents_are_declined() {
    assert_eq!(
        blocked("pdftotext 起诉书.pdf -"),
        format!("{CWD}/起诉书.pdf")
    );
    assert_eq!(
        blocked("unzip -p 辩护意见.docx word/document.xml | head"),
        format!("{CWD}/辩护意见.docx")
    );
    assert_eq!(
        blocked("textutil -convert txt -stdout 旧文书.doc"),
        format!("{CWD}/旧文书.doc")
    );
    assert_eq!(
        blocked("strings 判决书.ofd | head"),
        format!("{CWD}/判决书.ofd")
    );
    assert_eq!(blocked("cat 邮件.eml"), format!("{CWD}/邮件.eml"));
}

#[test]
fn quoted_paths_keep_spaces_and_chinese_names() {
    assert_eq!(
        blocked("head -3 '/tmp/my case/银行 流水.csv'"),
        "/tmp/my case/银行 流水.csv"
    );
    assert_eq!(
        blocked("cat \"/tmp/my case/起诉书 (副本).pdf\""),
        "/tmp/my case/起诉书 (副本).pdf"
    );
    assert_eq!(
        blocked("cat /tmp/my\\ case/流水.csv"),
        "/tmp/my case/流水.csv"
    );
    assert_eq!(
        blocked("head ~/Desktop/流水.csv"),
        format!("{HOME}/Desktop/流水.csv")
    );
}

#[test]
fn scripts_that_open_the_file_in_their_own_code_are_declined() {
    let heredoc = "python3 - <<'EOF'\nimport csv\nrows = list(csv.reader(open('/tmp/x/对公流水.csv', encoding='gb18030')))\nprint(len(rows))\nEOF";
    assert_eq!(blocked(heredoc), "/tmp/x/对公流水.csv");
    assert_eq!(
        blocked("python3 -c \"import openpyxl; openpyxl.load_workbook('证据清单.xlsx')\""),
        format!("{CWD}/证据清单.xlsx")
    );
    assert_eq!(
        blocked("/usr/bin/python3 analyse.py 流水.csv"),
        format!("{CWD}/流水.csv")
    );
    assert_eq!(
        blocked("python3.12 -c 'import pandas as pd; pd.read_csv(\"a b.csv\")'"),
        format!("{CWD}/a b.csv")
    );
    assert_eq!(
        blocked("node -e \"require('fs').readFileSync('/tmp/x.csv')\""),
        "/tmp/x.csv"
    );
    // A heredoc body is not a command: its own lines are only text fed to python.
    allowed("cat > notes.md <<'EOF'\nhead -5 流水.csv\nEOF");
    // A shell fed a script by heredoc runs it.
    assert_eq!(
        blocked("bash <<'EOF'\ncd /tmp/case\nhead -5 流水.csv\nEOF"),
        "/tmp/case/流水.csv"
    );
    assert_eq!(
        blocked("sqlite3 <<'EOF'\n.open '/tmp/x/cases.db'\n.tables\nEOF"),
        "/tmp/x/cases.db"
    );
}

#[test]
fn shell_wrappers_and_cd_are_followed() {
    assert_eq!(
        blocked("bash -lc \"head -5 /tmp/x/流水.csv\""),
        "/tmp/x/流水.csv"
    );
    assert_eq!(
        blocked("sh -c 'cd /tmp/case && iconv -f GBK -t UTF-8 流水.csv'"),
        "/tmp/case/流水.csv"
    );
    assert_eq!(blocked("cd /tmp/case; head 流水.csv"), "/tmp/case/流水.csv");
    assert_eq!(
        blocked("LANG=C env -i sudo head 流水.csv"),
        format!("{CWD}/流水.csv")
    );
    assert_eq!(
        blocked("timeout 10 cat 流水.csv"),
        format!("{CWD}/流水.csv")
    );
    assert_eq!(
        blocked("count=$(wc -l < a.csv); cat a.csv"),
        format!("{CWD}/a.csv")
    );
    let argv: Vec<String> = ["/bin/zsh", "-lc", "head -5 流水.csv"]
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        document_read_in_command(&argv, Path::new("/tmp/case"), None, &CASE).map(|r| r.path),
        Some(PathBuf::from("/tmp/case/流水.csv"))
    );
}

#[test]
fn looking_at_a_file_or_moving_it_is_not_a_read() {
    allowed("ls -la /tmp/x/流水.csv");
    allowed("file 流水.csv && wc -l 流水.csv && stat 起诉书.pdf && du -h 起诉书.pdf");
    allowed("cp 起诉书.pdf /tmp/out/ && mv a.csv b.csv && open 起诉书.pdf");
    allowed("wc -l < 流水.csv");
    allowed("tar -czf 案卷.tar.gz 起诉书.pdf 流水.csv");
    allowed("mdls 起诉书.pdf");
}

#[test]
fn an_output_is_not_a_read() {
    allowed("cat > 汇总.csv <<'EOF'\na,b\n1,2\nEOF");
    allowed("head -1 notes.txt > 汇总.csv 2>&1");
    allowed("echo x >> log.csv");
    allowed("sort names.txt >| 排序.csv");
    allowed("awk '{print}' notes.txt &> out.csv");
}

#[test]
fn export_copies_are_for_the_shell() {
    allowed("awk -F, 'NR>1 {s+=$5} END {print s}' 对公流水.export.csv");
    allowed("python3 -c \"import csv; print(sum(1 for _ in open('/tmp/x/起诉书.export.jsonl')))\"");
    allowed("sort -t, -k2 /tmp/x/流水.export.csv | head");
}

#[test]
fn a_file_only_written_by_code_is_not_a_read() {
    allowed(
        "python3 - <<'EOF'\nimport pandas as pd\ndf = pd.DataFrame({'a': [1]})\ndf.to_csv('totals.csv', index=False)\nEOF",
    );
    allowed("python3 -c \"import openpyxl; wb = openpyxl.Workbook(); wb.save('报告.xlsx')\"");
    allowed(
        "python3 -c \"from docx import Document; d = Document(); d.save( '/tmp/x/意见书.docx' )\"",
    );
    allowed("python3 -c \"df.to_excel(\\\"汇总 表.xlsx\\\")\"");
    allowed("python3 -c \"import csv; w = csv.writer(open('out.csv', 'w', newline=''))\"");
    allowed("python3 -c \"open('log.csv', mode='a').write('x')\"");
    allowed("node -e \"require('fs').writeFileSync('/tmp/x/汇总.csv', rows.join('\\n'))\"");
    allowed("python3 -c \"import xlsxwriter; xlsxwriter.Workbook('out.xlsx')\"");
    // Reading an export copy and writing a table of totals is what D203 asks for.
    allowed(
        "python3 - <<'EOF'\nimport csv\nrows = list(csv.reader(open('流水.export.csv')))\nwith open('totals.csv', 'w') as f:\n    f.write('x')\nEOF",
    );
    allowed("awk -F, 'NR>1 {s+=$5} END {print s}' 'cases.export 2.csv'");
    // A read beside a write is still a read; `open(…)` without a write mode reads.
    assert_eq!(
        blocked("python3 -c \"import pandas as pd; pd.read_csv('流水.csv').to_csv('totals.csv')\""),
        format!("{CWD}/流水.csv")
    );
    assert_eq!(
        blocked("python3 -c \"open('流水.csv', 'r').read(); open('b.csv', encoding='gbk')\""),
        format!("{CWD}/流水.csv")
    );
    assert_eq!(
        blocked("python3 -c \"open('/tmp/x/b.csv', encoding='gbk').read()\""),
        "/tmp/x/b.csv"
    );
}

#[test]
fn a_file_that_does_not_exist_yet_is_made_not_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("流水.csv"), "a,b\n").expect("statement");
    let exists = |path: &Path| path.exists();
    let probe = Probe {
        exists: &exists,
        in_git_tree: &never_git,
    };
    let run = |script: &str| document_read_in_script(script, dir.path(), None, &probe);
    assert_eq!(run("sqlite3 out.db 'CREATE TABLE t (a)'"), None);
    assert_eq!(
        run("python3 make.py --out 报告.xlsx && cat > 汇总.csv <<'EOF'\na\nEOF"),
        None
    );
    assert_eq!(
        run("head -3 流水.csv").map(|r| r.path),
        Some(dir.path().join("流水.csv"))
    );
}

#[test]
fn code_and_text_files_are_not_guarded() {
    allowed("cat src/main.rs README.md package.json notes.txt data.json");
    allowed("grep -rn TODO . && rg -n 'fn main' src");
    allowed("python3 scripts/build.py --out dist");
    allowed("head -5 server.key && cat id_rsa.pub");
    allowed("sed -n '1,40p' report.md");
}

#[test]
fn tables_and_databases_in_a_git_work_tree_stay_readable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).expect("git dir");
    std::fs::create_dir_all(repo.join("fixtures")).expect("fixtures");
    let worktree = dir.path().join("worktree");
    std::fs::create_dir_all(&worktree).expect("worktree");
    std::fs::write(
        worktree.join(".git"),
        "gitdir: /elsewhere/.git/worktrees/w\n",
    )
    .expect("git file");
    let outside = dir.path().join("case");
    std::fs::create_dir_all(&outside).expect("case");
    let in_git = |path: &Path| {
        let mut dir = if path.is_dir() {
            Some(path)
        } else {
            path.parent()
        };
        while let Some(d) = dir {
            if d.join(".git").exists() {
                return true;
            }
            dir = d.parent();
        }
        false
    };
    let probe = Probe {
        exists: &always,
        in_git_tree: &in_git,
    };
    let run = |script: &str, cwd: &Path| document_read_in_script(script, cwd, None, &probe);
    assert_eq!(
        run(
            "head -5 fixtures/rows.csv && sqlite3 fixtures/test.db .tables",
            &repo
        ),
        None
    );
    assert_eq!(run("cat data.parquet", &worktree), None);
    // A document is a document in a repository too.
    assert_eq!(
        run("pdftotext fixtures/sample.pdf -", &repo).map(|r| r.kind),
        Some(DocumentKind::Document)
    );
    // The same table outside it is declined, also when named from inside the repository.
    let table = outside.join("流水.csv");
    assert_eq!(
        run(&format!("head -5 '{}'", table.display()), &repo).map(|r| r.path),
        Some(table.clone())
    );
    assert_eq!(
        run("head -5 流水.csv", &outside).map(|r| r.path),
        Some(table)
    );
}

#[test]
fn the_refusal_names_the_reader_and_export() {
    let read = check("head -5 /tmp/x/对公流水.csv").expect("declined");
    assert_eq!(read.kind, DocumentKind::Table);
    assert_eq!(read.reader, "sheet.file");
    assert_eq!(
        read.refusal(),
        "WitLex blocked this command: 对公流水.csv is a table — read it with sheet.file(path); its export() writes a copy this shell may process."
    );
    assert_eq!(
        check("sqlite3 a.db .tables").map(|r| r.reader),
        Some("sqlite.file")
    );
    assert_eq!(check("cat a.PDF").map(|r| r.reader), Some("pdf.file"));
    assert_eq!(check("cat a.pptx").map(|r| r.reader), Some("slides.file"));
    assert_eq!(check("cat a.wps").map(|r| r.reader), Some("word.file"));
}
