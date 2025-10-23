use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use toml_edit::DocumentMut;

/// ==============================
/// 定義
/// ==============================

const DEFAULT_TEMPLATE_URL: &str = "https://github.com/wogikaze/kp-rust";
const CONFIG_FILE_NAME: &str = "kp-config.toml";

#[derive(Parser, Debug)]
#[command(name = "kp", version, about = "AtCoder Rust CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// 初期設定
    Init,
    /// 設定を一覧または変更
    Config {
        #[command(subcommand)]
        sub: ConfigSub,
    },
    /// 新しいコンテストプロジェクトを作成
    New {
        contest_id: String,
        #[arg(long)]
        open: bool,
    },
    /// テスト実行
    Test {
        /// contest id (optional). If omitted, current dir is used.
        #[arg(num_args = 1..=2)]
        params: Vec<String>,
    },
    /// 提出
    Submit {
        contest_id: Option<String>,
        problem_id: String,
    },
    /// 問題ページを開く
    /// Usage: kp open [contest_id] [problem_id]
    Open {
        /// contest id (e.g. abc411). If omitted, look for contest.acc.json in current dir
        contest_id: Option<String>,
        /// problem id (e.g. a or abc411_a). If omitted, open contest URL from contest.acc.json
        problem_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigSub {
    /// 現在の設定を表示
    List,
    /// 設定を変更
    Set { key: String, value: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct KpConfig {
    template_repository_url: String,
    minify_on_submit: bool,
}

impl Default for KpConfig {
    fn default() -> Self {
        Self {
            template_repository_url: DEFAULT_TEMPLATE_URL.to_string(),
            minify_on_submit: false,
        }
    }
}

/// ==============================
/// メイン処理
/// ==============================

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init => cmd_init()?,
        Commands::Config { sub } => match sub {
            ConfigSub::List => cmd_config_list()?,
            ConfigSub::Set { key, value } => cmd_config_set(&key, &value)?,
        },
        Commands::New { contest_id, open } => cmd_new(&contest_id, open)?,
        Commands::Test { params } => match params.as_slice() {
            [pid] => cmd_test(None, pid)?,
            [cid, pid] => cmd_test(Some(cid), pid)?,
            _ => anyhow::bail!("Usage: kp test [contest_id] <problem_id>"),
        },
        Commands::Submit {
            contest_id,
            problem_id,
        } => cmd_submit(contest_id.as_deref(), &problem_id)?,
        Commands::Open {
            contest_id,
            problem_id,
        } => cmd_open(contest_id.as_deref(), problem_id.as_deref())?,
    }
    Ok(())
}

/// ==============================
/// サブコマンド
/// ==============================

fn cmd_init() -> Result<()> {
    ensure_tools(&["acc", "oj", "git", "cargo"])?;
    let acc_conf = acc_config_dir()?;

    let cfg_path = acc_conf.join(CONFIG_FILE_NAME);
    if !cfg_path.exists() {
        save_config(&acc_conf, &KpConfig::default())?;
    }

    let cfg = load_config(&acc_conf)?;

    let tpl_dir = acc_conf.join("kp-rust");
    if tpl_dir.exists() {
        run_in("git", &["pull"], &tpl_dir)?;
    } else {
        run_in(
            "git",
            &["clone", &cfg.template_repository_url, "kp-rust"],
            &acc_conf,
        )?;
    }

    // acc 設定
    run("acc", &["config", "default-template", "kp-rust"])?;
    run("acc", &["config", "default-task-dirname-format", "./"])?;
    run("acc", &["config", "default-task-choice", "all"])?;
    println!("✅ Initialized successfully");
    Ok(())
}

fn cmd_config_list() -> Result<()> {
    let cfg = load_config(&acc_config_dir()?)?;
    println!("{}", serde_json::to_string_pretty(&cfg)?);
    Ok(())
}

fn cmd_config_set(key: &str, new_value: &str) -> Result<()> {
    use toml_edit::value as toml_val;

    let acc_conf = acc_config_dir()?;
    let path = acc_conf.join(CONFIG_FILE_NAME);

    // 既存の設定を読み込み（なければ空の Document）
    let original_text = fs::read_to_string(&path).unwrap_or_default();
    let mut doc: DocumentMut = original_text.parse().unwrap_or_else(|_| DocumentMut::new());

    // 旧値を控えておき、必要なら init を最小限で再実行
    let old_template = doc
        .get("template_repository_url")
        .and_then(|it| it.as_str())
        .map(|s| s.to_string());

    match key {
        "template_repository_url" => {
            doc["template_repository_url"] = toml_val(new_value.to_string());
        }
        "minify_on_submit" => {
            let parsed = new_value
                .parse::<bool>()
                .context("minify_on_submit must be true/false")?;
            doc["minify_on_submit"] = toml_val(parsed);
        }
        _ => anyhow::bail!("Unknown key: {}", key),
    }

    // 保存
    fs::create_dir_all(&acc_conf)?;
    fs::write(&path, doc.to_string())?;
    println!("🔧 Updated config: {} = {}", key, new_value);

    // テンプレURLが変わったときだけ init 相当を実行
    let changed_template = match (old_template.as_deref(), key) {
        (Some(old), "template_repository_url") => old != new_value,
        (None, "template_repository_url") => true,
        _ => false,
    };
    if changed_template {
        cmd_init()?; // テンプレ取得や acc 既定設定を反映
    }

    Ok(())
}

fn cmd_new(contest_id: &str, open_flag: bool) -> Result<()> {
    run("acc", &["new", contest_id])?;
    let root = PathBuf::from(contest_id);
    let cargo_toml = root.join("Cargo.toml");
    if cargo_toml.exists() {
        append_bins(&cargo_toml, &root, contest_id)?;
    }
    if open_flag {
        let url = format!("https://atcoder.jp/contests/{}", contest_id);
        open_in_browser(&url)?;
    }
    Ok(())
}

fn cmd_test(contest_id: Option<&str>, problem_id: &str) -> Result<()> {
    let dir = contest_id
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    let test_dir = format!("tests/{problem_id}");
    let cmd = format!("cargo run --bin {problem_id}");
    // On Windows, ask oj to run `cmd /C <command>` so it executes via cmd
    let args_owned: Vec<String> = if cfg!(target_os = "windows") {
        let wrapped = format!("cmd /C {}", cmd);
        vec![
            "test".to_string(),
            "-c".to_string(),
            wrapped,
            "-d".to_string(),
            test_dir.clone(),
        ]
    } else {
        vec![
            "test".to_string(),
            "-c".to_string(),
            cmd.clone(),
            "-d".to_string(),
            test_dir.clone(),
        ]
    };
    let args: Vec<&str> = args_owned.iter().map(|s| s.as_str()).collect();
    run_in("oj", &args, &dir)?;
    Ok(())
}

fn cmd_submit(contest_id: Option<&str>, problem_id: &str) -> Result<()> {
    let dir = contest_id
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    let test_dir = format!("tests/{problem_id}");
    let cmd = format!("cargo run --bin {problem_id}");
    let cfg = load_config(&acc_config_dir()?)?;
    if cfg.minify_on_submit {
        println!("⚠️ minify_on_submit=true, but minify is not implemented yet");
    }
    let args_owned: Vec<String> = if cfg!(target_os = "windows") {
        let wrapped = format!("cmd /C {}", cmd);
        vec![
            "submit".to_string(),
            "-c".to_string(),
            wrapped,
            "-d".to_string(),
            test_dir.clone(),
        ]
    } else {
        vec![
            "submit".to_string(),
            "-c".to_string(),
            cmd.clone(),
            "-d".to_string(),
            test_dir.clone(),
        ]
    };
    let args: Vec<&str> = args_owned.iter().map(|s| s.as_str()).collect();
    run_in("oj", &args, &dir)?;
    Ok(())
}

// JSON structures for contest.acc.json (partial)
#[derive(Deserialize, Debug)]
struct ContestFile {
    contest: ContestEntry,
    tasks: Vec<TaskEntry>,
}

#[derive(Deserialize, Debug)]
struct ContestEntry {
    id: String,
    title: Option<String>,
    url: String,
}

#[derive(Deserialize, Debug)]
struct TaskEntry {
    id: String,
    label: Option<String>,
    title: Option<String>,
    url: String,
    directory: Option<serde_json::Value>,
}

/// Open logic per user's spec:
/// - kp open (contest_id) (problem_id)
/// - If contest_id omitted: look for contest.acc.json in cwd, error if missing
/// - If problem_id omitted: open contest.url from contest.acc.json
/// - If contest_id present but problem_id omitted: look for contest_id/contest.acc.json and open contest.url
/// - If both present: open the specific task url found in the contest's contest.acc.json
fn cmd_open(contest_id: Option<&str>, problem_id: Option<&str>) -> Result<()> {
    // Helper to read contest.acc.json from a directory
    let read_contest_file = |dir: &Path| -> Result<ContestFile> {
        let p = dir.join("contest.acc.json");
        if !p.exists() {
            anyhow::bail!("contest.acc.json not found in {}", dir.display());
        }
        let txt = fs::read_to_string(&p)?;
        let cf: ContestFile =
            serde_json::from_str(&txt).context("failed to parse contest.acc.json")?;
        Ok(cf)
    };

    let cwd = std::env::current_dir()?;

    match (contest_id, problem_id) {
        (None, None) => anyhow::bail!("Either contest_id or problem_id must be provided (or contest.acc.json must exist in current dir)"),
        (None, Some(_)) => {
            // Use contest.acc.json in cwd
            let cf = read_contest_file(&cwd)?;
            // find task by problem_id: accept suffix match (e.g., 'a' -> abc411_a) or full id
            let pid = problem_id.unwrap();
            let task = cf.tasks.iter().find(|t| t.id == pid || t.id.ends_with(&format!("_{}", pid)));
            if let Some(t) = task {
                open_in_browser(&t.url)?;
                return Ok(());
            }
            anyhow::bail!("Problem '{}' not found in contest.acc.json in {}", pid, cwd.display());
        }
        (Some(cid), None) => {
            // Look for <cid>/contest.acc.json
            let dir = cwd.join(cid);
            if !dir.exists() || !dir.is_dir() {
                anyhow::bail!("Contest directory '{}' not found", cid);
            }
            let cf = read_contest_file(&dir)?;
            open_in_browser(&cf.contest.url)?;
            return Ok(());
        }
        (Some(cid), Some(pid)) => {
            // Look for contest file in cid dir
            let dir = cwd.join(cid);
            if !dir.exists() || !dir.is_dir() {
                anyhow::bail!("Contest directory '{}' not found", cid);
            }
            let cf = read_contest_file(&dir)?;
            let task = cf.tasks.iter().find(|t| t.id == pid || t.id.ends_with(&format!("_{}", pid)));
            if let Some(t) = task {
                open_in_browser(&t.url)?;
                return Ok(());
            }
            anyhow::bail!("Problem '{}' not found in contest '{}'", pid, cid);
        }
    }
}

/// ==============================
/// ユーティリティ
/// ==============================

fn ensure_tools(tools: &[&str]) -> Result<()> {
    for tool in tools {
        let checker = if cfg!(target_os = "windows") {
            "where"
        } else {
            "which"
        };
        let status = Command::new(checker)
            .arg(tool)
            .status()
            .with_context(|| format!("failed to run `{}` to check for {}", checker, tool))?;
        if !status.success() {
            // Provide PATH context to help debugging
            let path = std::env::var("PATH").unwrap_or_default();
            anyhow::bail!("Required tool '{}' not found in PATH. Please install it and ensure it's on your PATH. PATH={}", tool, path);
        }
    }
    Ok(())
}

// Run a command in a platform-appropriate way. On Windows, use `cmd /C` so
// shims like npm's `.cmd`/.ps1 are resolved the same way an interactive shell
// would. On Unix, run directly.
fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let status = if cfg!(target_os = "windows") {
        let mut all = vec!["/C", cmd];
        all.extend(args.iter().map(|s| *s));
        Command::new("cmd").args(all).status()?
    } else {
        Command::new(cmd).args(args).status()?
    };
    if !status.success() {
        anyhow::bail!("Command failed: {} {:?}", cmd, args);
    }
    Ok(())
}

fn run_in(cmd: &str, args: &[&str], dir: &Path) -> Result<()> {
    let status = if cfg!(target_os = "windows") {
        let mut all = vec!["/C", cmd];
        all.extend(args.iter().map(|s| *s));
        Command::new("cmd").current_dir(dir).args(all).status()?
    } else {
        Command::new(cmd).current_dir(dir).args(args).status()?
    };
    if !status.success() {
        anyhow::bail!("Command failed in {:?}: {} {:?}", dir, cmd, args);
    }
    Ok(())
}

fn acc_config_dir() -> Result<PathBuf> {
    // Use same platform-aware invocation as run/run_in so Windows shims work.
    let out = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", "acc", "config-dir"])
            .output()
            .context("failed to run acc config-dir")?
    } else {
        Command::new("acc")
            .arg("config-dir")
            .output()
            .context("failed to run acc config-dir")?
    };
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(PathBuf::from(s))
}

fn load_config(acc_conf: &Path) -> Result<KpConfig> {
    let path = acc_conf.join(CONFIG_FILE_NAME);
    if !path.exists() {
        return Ok(KpConfig::default());
    }
    let text = fs::read_to_string(&path)?;
    Ok(toml_edit::de::from_str(&text)?)
}

fn save_config(acc_conf: &Path, cfg: &KpConfig) -> Result<()> {
    fs::create_dir_all(acc_conf)?;
    let text = toml_edit::ser::to_string_pretty(cfg)?;
    fs::write(acc_conf.join(CONFIG_FILE_NAME), text)?;
    Ok(())
}

fn append_bins(cargo_toml: &Path, contest_dir: &Path, contest_id: &str) -> Result<()> {
    // Read Cargo.toml text
    let mut text = fs::read_to_string(cargo_toml)?;
    let had_bins = text.contains("[[bin]]");

    // Set or insert package.name using simple string manipulation
    if let Some(pkg_start) = text.find("[package]") {
        // find end of package table (next table header like '\n[') or EOF
        let rest = &text[pkg_start..];
        let next_table = rest.find("\n[").map(|n| pkg_start + n);
        let pkg_end = next_table.unwrap_or(text.len());
        let pkg_section = &text[pkg_start..pkg_end];

        // Rebuild package section with name replaced/inserted
        let mut new_pkg = String::new();
        let mut name_replaced = false;
        for (i, line) in pkg_section.lines().enumerate() {
            if i == 0 {
                new_pkg.push_str(line);
                new_pkg.push('\n');
                continue;
            }
            if !name_replaced && line.trim_start().starts_with("name") {
                new_pkg.push_str(&format!("name = \"{}\"\n", contest_id));
                name_replaced = true;
            } else {
                new_pkg.push_str(line);
                new_pkg.push('\n');
            }
        }
        if !name_replaced {
            // insert name after the [package] header
            let after_header = new_pkg.find('\n').map(|n| n + 1).unwrap_or(new_pkg.len());
            new_pkg.insert_str(after_header, &format!("name = \"{}\"\n", contest_id));
        }
        text = format!("{}{}{}", &text[..pkg_start], new_pkg, &text[pkg_end..]);
    } else {
        // no package table: prepend one
        text = format!("[package]\nname = \"{}\"\n\n{}", contest_id, text);
    }
    fs::write(cargo_toml, &text)?;

    // If binary entries already exist, do not add more
    if had_bins {
        return Ok(());
    }

    // Read contest.acc.json to get tasks
    let contest_json = contest_dir.join("contest.acc.json");
    if !contest_json.exists() {
        return Ok(());
    }
    let cj_text = fs::read_to_string(&contest_json)?;
    let cf: ContestFile =
        serde_json::from_str(&cj_text).context("failed to parse contest.acc.json")?;

    // Build [[bin]] entries
    let mut bins_text = String::new();
    for task in cf.tasks.iter() {
        // Determine bin name: prefer short suffix (after contest_id + '_'), else label (lowercase), else full id
        let name = if let Some(s) = task.id.strip_prefix(&format!("{}_", contest_id)) {
            s.to_string()
        } else if let Some(lbl) = &task.label {
            lbl.to_lowercase()
        } else {
            task.id.clone()
        };
        let path = format!("src/{}.rs", name);
        bins_text.push_str("\n[[bin]]\n");
        bins_text.push_str(&format!("name = \"{}\"\n", name));
        bins_text.push_str(&format!("path = \"{}\"\n", path));
    }

    let mut f = fs::OpenOptions::new().append(true).open(cargo_toml)?;
    f.write_all(bins_text.as_bytes())?;
    Ok(())
}

fn open_in_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", url]).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}
