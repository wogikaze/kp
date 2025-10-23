use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::fs;
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
        contest_id: Option<String>,
        problem_id: String,
    },
    /// 提出
    Submit {
        contest_id: Option<String>,
        problem_id: String,
    },
    /// 問題ページを開く
    Open { problem_id: String },
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
        Commands::Test {
            contest_id,
            problem_id,
        } => cmd_test(contest_id.as_deref(), &problem_id)?,
        Commands::Submit {
            contest_id,
            problem_id,
        } => cmd_submit(contest_id.as_deref(), &problem_id)?,
        Commands::Open { problem_id } => cmd_open(&problem_id)?,
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
        append_bins(&cargo_toml)?;
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
    let test_dir = format!("{problem_id}/tests");
    let cmd = format!("cargo run --bin {problem_id}");
    run_in("oj", &["test", "-c", &cmd, "-d", &test_dir], &dir)?;
    Ok(())
}

fn cmd_submit(contest_id: Option<&str>, problem_id: &str) -> Result<()> {
    let dir = contest_id
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    let test_dir = format!("{problem_id}/tests");
    let cmd = format!("cargo run --bin {problem_id}");
    let cfg = load_config(&acc_config_dir()?)?;
    if cfg.minify_on_submit {
        println!("⚠️ minify_on_submit=true, but minify is not implemented yet");
    }
    run_in("oj", &["submit", "-c", &cmd, "-d", &test_dir], &dir)?;
    Ok(())
}

fn cmd_open(problem_id: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let contest_id = cwd
        .file_name()
        .and_then(|s| s.to_str())
        .context("contest_id not found (current dir)")?;
    let url = format!("https://atcoder.jp/contests/{contest_id}/tasks/{problem_id}");
    open_in_browser(&url)?;
    Ok(())
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

fn append_bins(cargo_toml: &Path) -> Result<()> {
    let mut text = fs::read_to_string(cargo_toml)?;
    if text.contains("[[bin]]") {
        return Ok(()); // 既に追加済み
    }
    text.push_str("\n[[bin]]\nname = \"a\"\npath = \"a/src/main.rs\"\n");
    fs::write(cargo_toml, text)?;
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
