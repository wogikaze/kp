use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use toml_edit::{value, ArrayOfTables, DocumentMut, Item, Table};

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
    Init {
        /// template repository URL override (also updates config)
        #[arg(long)]
        repository: Option<String>,
    },
    /// AtCoder の REVEL_SESSION を acc / oj に保存
    Login {
        /// 保存先ツール名 (例: --tools oj acc)。省略時は自動検出
        #[arg(long, num_args = 1..)]
        tools: Vec<String>,
        /// REVEL_SESSION の値。省略時は対話入力
        #[arg(long)]
        session: Option<String>,
        /// oj の cookie.jar パスを上書き
        #[arg(long)]
        oj_cookie_path: Option<PathBuf>,
        /// acc の session.json パスを上書き
        #[arg(long)]
        acc_cookie_path: Option<PathBuf>,
    },
    /// 設定を一覧または変更
    Config {
        #[command(subcommand)]
        sub: ConfigSub,
    },
    /// 新しいコンテストプロジェクトを作成
    New {
        /// contest id (e.g. abc411) or contest URL (e.g. https://atcoder.jp/contests/abc411)
        contest_id: String,
        #[arg(long)]
        open: bool,
        /// language key (e.g. rust, cpp). If omitted, use default_language from config
        #[arg(long)]
        lang: Option<String>,
    },
    /// テスト実行
    Test {
        /// contest id (optional). If omitted, current dir is used.
        #[arg(num_args = 1..=2)]
        params: Vec<String>,
        /// language key (e.g. rust, cpp). If omitted, use default_language from config
        #[arg(long)]
        lang: Option<String>,
    },
    /// cargo run を実行
    /// Usage: kp run [contest_id] <bin> [--debug]
    Run {
        /// contest id (optional). If omitted, current dir is used.
        #[arg(num_args = 1..=2)]
        params: Vec<String>,
        /// debug build で実行する (--release を付けない)
        #[arg(long)]
        debug: bool,
    },
    /// 提出
    Submit {
        contest_id: Option<String>,
        problem_id: String,
        /// language key (e.g. rust, cpp). If omitted, use default_language from config
        #[arg(long)]
        lang: Option<String>,
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
    /// legacy (kept for backward compatibility)
    #[serde(default)]
    template_repository_url: String,
    #[serde(default = "default_language")]
    default_language: String,
    #[serde(default)]
    language: BTreeMap<String, LanguageConfig>,
    minify_on_submit: bool,
}

fn default_language() -> String {
    "rust".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct LanguageConfig {
    template_repository_url: Option<String>,
    template_dir: Option<String>,
    test_command: Option<String>,
    submit_command: Option<String>,
    build_command: Option<String>,
}

impl Default for KpConfig {
    fn default() -> Self {
        let mut language = BTreeMap::new();
        language.insert(
            "rust".to_string(),
            LanguageConfig {
                template_repository_url: Some(DEFAULT_TEMPLATE_URL.to_string()),
                template_dir: Some("kp-rust".to_string()),
                test_command: Some("cargo run --bin {problem_id} --release".to_string()),
                submit_command: Some("cargo run --bin {problem_id} --release".to_string()),
                build_command: None,
            },
        );
        Self {
            template_repository_url: DEFAULT_TEMPLATE_URL.to_string(),
            default_language: default_language(),
            language,
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
        Commands::Init { repository } => cmd_init(repository.as_deref())?,
        Commands::Login {
            tools,
            session,
            oj_cookie_path,
            acc_cookie_path,
        } => cmd_login(
            &tools,
            session.as_deref(),
            oj_cookie_path.as_deref(),
            acc_cookie_path.as_deref(),
        )?,
        Commands::Config { sub } => match sub {
            ConfigSub::List => cmd_config_list()?,
            ConfigSub::Set { key, value } => cmd_config_set(&key, &value)?,
        },
        Commands::New {
            contest_id,
            open,
            lang,
        } => cmd_new(&contest_id, open, lang.as_deref())?,
        Commands::Test { params, lang } => match params.as_slice() {
            [pid] => cmd_test(None, pid, lang.as_deref())?,
            [cid, pid] => cmd_test(Some(cid), pid, lang.as_deref())?,
            _ => anyhow::bail!("Usage: kp test [contest_id] <problem_id>"),
        },
        Commands::Run { params, debug } => match params.as_slice() {
            [bin] => cmd_run(None, bin, debug)?,
            [cid, bin] => cmd_run(Some(cid), bin, debug)?,
            _ => anyhow::bail!("Usage: kp run [contest_id] <bin> [--debug]"),
        },
        Commands::Submit {
            contest_id,
            problem_id,
            lang,
        } => cmd_submit(contest_id.as_deref(), &problem_id, lang.as_deref())?,
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

fn cmd_init(repository: Option<&str>) -> Result<()> {
    ensure_tools(&["acc", "oj", "git"])?;
    let acc_conf = acc_config_dir()?;

    let cfg_path = acc_conf.join(CONFIG_FILE_NAME);
    if !cfg_path.exists() {
        save_config(&acc_conf, &KpConfig::default())?;
    }

    let mut cfg = load_config(&acc_conf)?;
    if let Some(repo) = repository {
        cfg.template_repository_url = repo.to_string();
        let lang = cfg.default_language.clone();
        cfg.language
            .entry(lang)
            .or_default()
            .template_repository_url = Some(repo.to_string());
        save_config(&acc_conf, &cfg)?;
    }
    let lang = select_language(&cfg, None)?;
    let lang_cfg = get_language_config(&cfg, &lang)?;
    let tpl_repo = lang_cfg
        .template_repository_url
        .as_deref()
        .unwrap_or(&cfg.template_repository_url);
    let tpl_dir_name = lang_cfg
        .template_dir
        .clone()
        .unwrap_or_else(|| format!("kp-{}", lang));
    let tpl_dir = acc_conf.join(&tpl_dir_name);
    if tpl_dir.exists() {
        run_in("git", &["pull"], &tpl_dir)?;
    } else {
        run_in("git", &["clone", tpl_repo, &tpl_dir_name], &acc_conf)?;
    }

    // acc 設定
    run("acc", &["config", "default-template", &tpl_dir_name])?;
    run("acc", &["config", "default-task-dirname-format", "./"])?;
    run("acc", &["config", "default-task-choice", "all"])?;
    println!("✅ Initialized successfully");
    Ok(())
}

fn cmd_login(
    specified_tools: &[String],
    session: Option<&str>,
    oj_cookie_path: Option<&Path>,
    acc_cookie_path: Option<&Path>,
) -> Result<()> {
    let tools = resolve_login_tools(specified_tools)?;
    if tools.is_empty() {
        anyhow::bail!("エラー: 対応するツールがインストールされていません");
    }

    println!("検知されたツール:");
    for tool in &tools {
        println!("- {}", tool.name());
    }

    let cookie_value = match session {
        Some(value) => normalize_revel_session_input(value),
        None => prompt_revel_session()?,
    };
    if cookie_value.is_empty() {
        anyhow::bail!("エラー: クッキーが入力されていません");
    }

    let mut success_count = 0usize;
    let mut errors = Vec::new();
    for tool in tools {
        let result = match tool {
            LoginTool::Oj => {
                let path = match oj_cookie_path {
                    Some(path) => path.to_path_buf(),
                    None => default_oj_cookie_path()?,
                };
                store_oj_session_cookie(&path, &cookie_value)
            }
            LoginTool::Acc => {
                let path = match acc_cookie_path {
                    Some(path) => path.to_path_buf(),
                    None => acc_config_dir()?.join("session.json"),
                };
                store_acc_session_cookie(&path, &cookie_value)
            }
        };
        match result {
            Ok(()) => success_count += 1,
            Err(err) => errors.push(format!("{}: {}", tool.name(), err)),
        }
    }

    if errors.is_empty() {
        println!(
            "✅ すべてのツール ({}/{}) にクッキーを保存しました",
            success_count, success_count
        );
        return Ok(());
    }

    if success_count > 0 {
        anyhow::bail!(
            "⚠️ 一部のツール ({}/{}) にクッキーを保存しました: {}",
            success_count,
            success_count + errors.len(),
            errors.join(" / ")
        );
    }

    anyhow::bail!("❌ クッキーの保存に失敗しました: {}", errors.join(" / "))
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
        "default_language" => {
            doc["default_language"] = toml_val(new_value.to_string());
        }
        "minify_on_submit" => {
            let parsed = new_value
                .parse::<bool>()
                .context("minify_on_submit must be true/false")?;
            doc["minify_on_submit"] = toml_val(parsed);
        }
        _ if key.starts_with("language.") => {
            // language.<lang>.<field>
            let rest = key.trim_start_matches("language.");
            let (lang, field) = rest
                .split_once('.')
                .context("language.* must be like language.<lang>.<field>")?;
            doc["language"][lang][field] = toml_val(new_value.to_string());
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
        cmd_init(None)?; // テンプレ取得や acc 既定設定を反映
    }

    Ok(())
}

fn cmd_new(contest_id: &str, open_flag: bool, lang: Option<&str>) -> Result<()> {
    let contest_id = normalize_contest_id_input(contest_id)?;
    let acc_conf = acc_config_dir()?;
    let cfg = load_config(&acc_conf)?;
    let lang = select_language(&cfg, lang)?;
    let lang_cfg = get_language_config(&cfg, &lang)?;
    let tpl_repo = lang_cfg
        .template_repository_url
        .as_deref()
        .unwrap_or(&cfg.template_repository_url);
    let tpl_dir_name = lang_cfg
        .template_dir
        .clone()
        .unwrap_or_else(|| format!("kp-{}", lang));
    let tpl_dir = acc_conf.join(&tpl_dir_name);
    if tpl_dir.exists() {
        run_in("git", &["pull"], &tpl_dir)?;
    } else {
        run_in("git", &["clone", tpl_repo, &tpl_dir_name], &acc_conf)?;
    }
    run("acc", &["config", "default-template", &tpl_dir_name])?;

    run("acc", &["new", &contest_id])?;
    let root = PathBuf::from(&contest_id);
    let cargo_toml = root.join("Cargo.toml");
    if cargo_toml.exists() {
        append_bins(&cargo_toml, &root, &contest_id)?;
    }

    if lang == "rust" {
        if let Err(e) = add_vscode_linked_project(&contest_id) {
            eprintln!("warning: failed to update .vscode/settings.json: {}", e);
        }
    }

    if open_flag {
        let url = format!("https://atcoder.jp/contests/{}", contest_id);
        open_in_browser(&url)?;
    }
    Ok(())
}

fn run_shell_in(command: &str, dir: &Path) -> Result<()> {
    let status = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .current_dir(dir)
            .args(["/C", command])
            .status()?
    } else {
        Command::new("sh")
            .current_dir(dir)
            .args(["-c", command])
            .status()?
    };
    if !status.success() {
        anyhow::bail!("Command failed in {:?}: {}", dir, command);
    }
    Ok(())
}

fn cmd_test(contest_id: Option<&str>, problem_id: &str, lang: Option<&str>) -> Result<()> {
    let dir = contest_id
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    let test_dir = format!("tests/{problem_id}");
    let cfg = load_config(&acc_config_dir()?)?;
    let lang = select_language(&cfg, lang)?;
    let lang_cfg = get_language_config(&cfg, &lang)?;

    if let Some(build_tpl) = lang_cfg.build_command.as_deref() {
        let build_cmd = apply_command_template(build_tpl, contest_id, problem_id);
        run_shell_in(&build_cmd, &dir)?;
    }

    let run_tpl = lang_cfg
        .test_command
        .as_deref()
        .unwrap_or("cargo run --bin {problem_id} --release");
    let run_cmd = apply_command_template(run_tpl, contest_id, problem_id);

    let args_owned: Vec<String> = if cfg!(target_os = "windows") {
        vec![
            "test".to_string(),
            "-c".to_string(),
            format!("cmd /C {}", run_cmd),
            "-d".to_string(),
            test_dir,
        ]
    } else {
        vec![
            "test".to_string(),
            "-c".to_string(),
            run_cmd,
            "-d".to_string(),
            test_dir,
        ]
    };
    let args: Vec<&str> = args_owned.iter().map(|s| s.as_str()).collect();
    run_in("oj", &args, &dir)?;
    Ok(())
}

fn cmd_run(contest_id: Option<&str>, bin: &str, debug: bool) -> Result<()> {
    let dir = contest_id
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    let args_owned = cargo_run_args(bin, debug);
    let args: Vec<&str> = args_owned.iter().map(|s| s.as_str()).collect();
    run_in("cargo", &args, &dir)?;
    Ok(())
}

fn cmd_submit(contest_id: Option<&str>, problem_id: &str, lang: Option<&str>) -> Result<()> {
    let dir = contest_id
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    let test_dir = format!("tests/{problem_id}");
    let cfg = load_config(&acc_config_dir()?)?;
    let lang = select_language(&cfg, lang)?;
    let lang_cfg = get_language_config(&cfg, &lang)?;
    let cmd_tpl = lang_cfg
        .submit_command
        .as_deref()
        .unwrap_or("cargo run --bin {problem_id} --release");
    let cmd = apply_command_template(cmd_tpl, contest_id, problem_id);
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

fn select_language(cfg: &KpConfig, override_lang: Option<&str>) -> Result<String> {
    let lang = override_lang
        .map(|s| s.to_string())
        .unwrap_or_else(|| cfg.default_language.clone());
    if cfg.language.contains_key(&lang) {
        return Ok(lang);
    }
    // fallback for legacy config: allow rust
    if lang == "rust" {
        return Ok(lang);
    }
    anyhow::bail!("Unknown language: {}", lang)
}

fn get_language_config<'a>(cfg: &'a KpConfig, lang: &str) -> Result<&'a LanguageConfig> {
    cfg.language
        .get(lang)
        .ok_or_else(|| anyhow::anyhow!("language config missing: {}", lang))
}

fn apply_command_template(cmd: &str, contest_id: Option<&str>, problem_id: &str) -> String {
    let mut out = cmd.replace("{problem_id}", problem_id);
    let contest_val = contest_id.unwrap_or("");
    out = out.replace("{contest_id}", contest_val);
    out
}

fn cargo_run_args(bin: &str, debug: bool) -> Vec<String> {
    let mut args = vec!["run".to_string(), "--bin".to_string(), bin.to_string()];
    if !debug {
        args.push("--release".to_string());
    }
    args
}

fn normalize_contest_id_input(input: &str) -> Result<String> {
    for prefix in [
        "https://atcoder.jp/contests/",
        "http://atcoder.jp/contests/",
    ] {
        if let Some(rest) = input.strip_prefix(prefix) {
            let path = rest
                .split(['?', '#'])
                .next()
                .unwrap_or_default()
                .trim_matches('/');
            let mut segments = path.split('/').filter(|segment| !segment.is_empty());
            let contest_id = segments
                .next()
                .context("contest URL must include a contest id")?;
            if segments.next().is_some() {
                anyhow::bail!("contest URL must point to a contest page: {}", input);
            }
            return Ok(contest_id.to_string());
        }
    }
    Ok(input.to_string())
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoginTool {
    Oj,
    Acc,
}

impl LoginTool {
    fn name(self) -> &'static str {
        match self {
            Self::Oj => "oj",
            Self::Acc => "acc",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "oj" => Some(Self::Oj),
            "acc" => Some(Self::Acc),
            _ => None,
        }
    }
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
        if !command_exists(tool)? {
            // Provide PATH context to help debugging
            let path = std::env::var("PATH").unwrap_or_default();
            anyhow::bail!("Required tool '{}' not found in PATH. Please install it and ensure it's on your PATH. PATH={}", tool, path);
        }
    }
    Ok(())
}

fn command_exists(cmd: &str) -> Result<bool> {
    let checker = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let status = Command::new(checker)
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("failed to run `{}` to check for {}", checker, cmd))?;
    Ok(status.success())
}

fn resolve_login_tools(specified_tools: &[String]) -> Result<Vec<LoginTool>> {
    let candidates = [LoginTool::Oj, LoginTool::Acc];
    let mut tools = Vec::new();

    if specified_tools.is_empty() {
        for tool in candidates {
            if command_exists(tool.name())? {
                tools.push(tool);
            }
        }
        return Ok(tools);
    }

    for tool_name in specified_tools {
        match LoginTool::from_name(tool_name) {
            Some(tool) => {
                if command_exists(tool.name())? {
                    if !tools.contains(&tool) {
                        tools.push(tool);
                    }
                } else {
                    eprintln!("警告: {} はインストールされていないようです", tool_name);
                }
            }
            None => eprintln!("警告: {} は未対応のツールです", tool_name),
        }
    }

    Ok(tools)
}

fn prompt_revel_session() -> Result<String> {
    print!("AtCoder の REVEL_SESSION クッキーを貼り付けてください: ");
    io::stdout().flush().context("failed to flush stdout")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read REVEL_SESSION")?;
    Ok(normalize_revel_session_input(&input))
}

fn normalize_revel_session_input(input: &str) -> String {
    let trimmed = input.trim();
    let without_prefix = trimmed.strip_prefix("REVEL_SESSION=").unwrap_or(trimmed);
    let value = without_prefix
        .split(';')
        .next()
        .unwrap_or(without_prefix)
        .trim();
    value.to_string()
}

fn default_oj_cookie_path() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .context("LOCALAPPDATA or APPDATA is not set")?;
        Ok(PathBuf::from(base)
            .join("online-judge-tools")
            .join("cookie.jar"))
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").context("HOME is not set")?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("online-judge-tools")
            .join("cookie.jar"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .context("XDG_DATA_HOME or HOME is not set")?;
        Ok(base.join("online-judge-tools").join("cookie.jar"))
    }
}

fn store_acc_session_cookie(path: &Path, cookie_value: &str) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("failed to determine parent of {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let existing = if path.exists() {
        Some(
            fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?,
        )
    } else {
        None
    };
    let updated = update_acc_session_json(existing.as_deref(), cookie_value)?;
    fs::write(path, updated).with_context(|| format!("failed to write {}", path.display()))?;
    println!("✅ acc: クッキーを {} に保存しました", path.display());
    Ok(())
}

fn update_acc_session_json(existing: Option<&str>, cookie_value: &str) -> Result<String> {
    use serde_json::{Map, Value};

    let mut value = match existing {
        Some(text) => serde_json::from_str(text).unwrap_or_else(|_| Value::Object(Map::new())),
        None => Value::Object(Map::new()),
    };

    let object = value
        .as_object_mut()
        .context("acc session.json must contain a JSON object")?;

    let mut cookies = Vec::new();
    let mut has_flash = false;
    let mut has_session = false;
    if let Some(existing_cookies) = object.get("cookies").and_then(|value| value.as_array()) {
        for entry in existing_cookies {
            let Some(cookie) = entry.as_str() else {
                continue;
            };
            if cookie.starts_with("REVEL_SESSION=") {
                if !has_session {
                    cookies.push(format!("REVEL_SESSION={}", cookie_value));
                    has_session = true;
                }
                continue;
            }
            if cookie.starts_with("REVEL_FLASH=") {
                has_flash = true;
            }
            cookies.push(cookie.to_string());
        }
    }

    if !has_flash {
        cookies.insert(0, "REVEL_FLASH=".to_string());
    }
    if !has_session {
        cookies.push(format!("REVEL_SESSION={}", cookie_value));
    }

    object.insert(
        "cookies".to_string(),
        Value::Array(cookies.into_iter().map(Value::String).collect()),
    );
    let text = serde_json::to_string_pretty(&value).context("failed to format acc session.json")?;
    Ok(format!("{}\n", text))
}

fn store_oj_session_cookie(path: &Path, cookie_value: &str) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("failed to determine parent of {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let existing = if path.exists() {
        Some(
            fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?,
        )
    } else {
        None
    };
    let updated = update_oj_cookie_jar(existing.as_deref(), cookie_value);
    fs::write(path, updated).with_context(|| format!("failed to write {}", path.display()))?;
    println!("✅ oj: クッキーを {} に保存しました", path.display());
    Ok(())
}

fn update_oj_cookie_jar(existing: Option<&str>, cookie_value: &str) -> String {
    let mut lines = vec!["#LWP-Cookies-2.0".to_string()];

    if let Some(text) = existing {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || trimmed == "#LWP-Cookies-2.0"
                || trimmed.starts_with("Set-Cookie3: REVEL_SESSION=")
            {
                continue;
            }
            lines.push(line.to_string());
        }
    }

    lines.push(format!(
        "Set-Cookie3: {}",
        format_oj_revel_session_cookie(cookie_value)
    ));
    lines.push(String::new());
    lines.join("\n")
}

fn format_oj_revel_session_cookie(cookie_value: &str) -> String {
    format!(
        "REVEL_SESSION={}; path={}; domain={}; path_spec; secure; expires={}; HttpOnly=None; version=0",
        format_lwp_cookie_value(cookie_value),
        format_lwp_cookie_value("/"),
        format_lwp_cookie_value("atcoder.jp"),
        format_lwp_cookie_value("2099-12-31 23:59:59Z"),
    )
}

fn format_lwp_cookie_value(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return value.to_string();
    }
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
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
    let mut cfg: KpConfig = toml_edit::de::from_str(&text)?;
    // migrate legacy fields into language.rust if missing
    if !cfg.language.contains_key("rust") {
        cfg.language.insert(
            "rust".to_string(),
            LanguageConfig {
                template_repository_url: Some(cfg.template_repository_url.clone()),
                template_dir: Some("kp-rust".to_string()),
                test_command: Some("cargo run --bin {problem_id} --release".to_string()),
                submit_command: Some("cargo run --bin {problem_id} --release".to_string()),
                build_command: None,
            },
        );
    }
    if cfg.default_language.is_empty() {
        cfg.default_language = default_language();
    }
    Ok(cfg)
}

fn save_config(acc_conf: &Path, cfg: &KpConfig) -> Result<()> {
    fs::create_dir_all(acc_conf)?;
    let text = toml_edit::ser::to_string_pretty(cfg)?;
    fs::write(acc_conf.join(CONFIG_FILE_NAME), text)?;
    Ok(())
}

fn append_bins(cargo_toml: &Path, contest_dir: &Path, contest_id: &str) -> Result<()> {
    let text = fs::read_to_string(cargo_toml)
        .with_context(|| format!("failed to read {}", cargo_toml.display()))?;
    let mut doc: DocumentMut = text
        .parse()
        .with_context(|| format!("failed to parse {}", cargo_toml.display()))?;

    match doc.get_mut("package") {
        Some(item) => {
            let package = item.as_table_mut().context("`package` must be a table")?;
            package["name"] = value(contest_id);
        }
        None => {
            let mut package = Table::new();
            package["name"] = value(contest_id);
            doc["package"] = Item::Table(package);
        }
    }

    let had_bins = match doc.get("bin") {
        Some(item) => {
            let bins = item
                .as_array_of_tables()
                .context("`bin` must be an array of tables when present")?;
            !bins.is_empty()
        }
        None => false,
    };

    if !had_bins {
        let contest_json = contest_dir.join("contest.acc.json");
        if contest_json.exists() {
            let cj_text = fs::read_to_string(&contest_json)
                .with_context(|| format!("failed to read {}", contest_json.display()))?;
            let cf: ContestFile =
                serde_json::from_str(&cj_text).context("failed to parse contest.acc.json")?;

            if !cf.tasks.is_empty() {
                let mut bins = ArrayOfTables::new();
                for task in &cf.tasks {
                    let name = if let Some(s) = task.id.strip_prefix(&format!("{}_", contest_id)) {
                        s.to_string()
                    } else if let Some(label) = &task.label {
                        label.to_lowercase()
                    } else {
                        task.id.clone()
                    };
                    let path = format!("src/{}.rs", name);
                    let mut bin = Table::new();
                    bin["name"] = value(name);
                    bin["path"] = value(path);
                    bins.push(bin);
                }
                doc["bin"] = Item::ArrayOfTables(bins);
            }
        }
    }

    fs::write(cargo_toml, doc.to_string())
        .with_context(|| format!("failed to write {}", cargo_toml.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Result<Self> {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("system clock is before UNIX_EPOCH")?
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("kp-{}-{}-{}", name, std::process::id(), unique));
            fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn append_bins_updates_package_name_and_adds_bins() -> Result<()> {
        let dir = TestDir::new("append-bins-adds")?;
        let cargo_toml = dir.path().join("Cargo.toml");
        fs::write(
            &cargo_toml,
            r#"[package]
# keep this comment
version = "0.1.0"
edition = "2021"

[dependencies]
"#,
        )?;
        fs::write(
            dir.path().join("contest.acc.json"),
            r#"{
  "contest": { "id": "abc999", "title": "ABC999", "url": "https://example.com/contest" },
  "tasks": [
    { "id": "abc999_a", "label": "A", "title": "A", "url": "https://example.com/a", "directory": null },
    { "id": "custom_task", "label": "B", "title": "B", "url": "https://example.com/b", "directory": null }
  ]
}"#,
        )?;

        append_bins(&cargo_toml, dir.path(), "abc999")?;

        let text = fs::read_to_string(&cargo_toml)?;
        assert!(text.contains("# keep this comment"));

        let doc: DocumentMut = text.parse()?;
        assert_eq!(doc["package"]["name"].as_str(), Some("abc999"));

        let bins = doc["bin"].as_array_of_tables().unwrap();
        assert_eq!(bins.len(), 2);
        let first = bins.iter().next().unwrap();
        let second = bins.iter().nth(1).unwrap();
        assert_eq!(first["name"].as_str(), Some("a"));
        assert_eq!(first["path"].as_str(), Some("src/a.rs"));
        assert_eq!(second["name"].as_str(), Some("b"));
        assert_eq!(second["path"].as_str(), Some("src/b.rs"));
        Ok(())
    }

    #[test]
    fn append_bins_keeps_existing_bin_entries() -> Result<()> {
        let dir = TestDir::new("append-bins-keeps-existing")?;
        let cargo_toml = dir.path().join("Cargo.toml");
        fs::write(
            &cargo_toml,
            r#"[package]
edition = "2021"
name = "template"

[[bin]]
name = "existing"
path = "src/existing.rs"
"#,
        )?;
        fs::write(
            dir.path().join("contest.acc.json"),
            r#"{
  "contest": { "id": "abc999", "title": "ABC999", "url": "https://example.com/contest" },
  "tasks": [
    { "id": "abc999_a", "label": "A", "title": "A", "url": "https://example.com/a", "directory": null }
  ]
}"#,
        )?;

        append_bins(&cargo_toml, dir.path(), "abc999")?;

        let text = fs::read_to_string(&cargo_toml)?;
        let doc: DocumentMut = text.parse()?;
        assert_eq!(doc["package"]["name"].as_str(), Some("abc999"));

        let bins = doc["bin"].as_array_of_tables().unwrap();
        assert_eq!(bins.len(), 1);
        let first = bins.iter().next().unwrap();
        assert_eq!(first["name"].as_str(), Some("existing"));
        assert_eq!(first["path"].as_str(), Some("src/existing.rs"));
        Ok(())
    }

    #[test]
    fn cargo_run_args_use_release_by_default() {
        assert_eq!(
            cargo_run_args("a", false),
            vec!["run", "--bin", "a", "--release"]
        );
    }

    #[test]
    fn cargo_run_args_skip_release_in_debug_mode() {
        assert_eq!(cargo_run_args("a", true), vec!["run", "--bin", "a"]);
    }

    #[test]
    fn normalize_contest_id_keeps_plain_id() -> Result<()> {
        assert_eq!(normalize_contest_id_input("abc411")?, "abc411");
        Ok(())
    }

    #[test]
    fn normalize_contest_id_accepts_contest_url() -> Result<()> {
        assert_eq!(
            normalize_contest_id_input("https://atcoder.jp/contests/abc411")?,
            "abc411"
        );
        Ok(())
    }

    #[test]
    fn normalize_contest_id_accepts_trailing_slash_and_query() -> Result<()> {
        assert_eq!(
            normalize_contest_id_input("https://atcoder.jp/contests/abc411/?lang=ja")?,
            "abc411"
        );
        Ok(())
    }

    #[test]
    fn normalize_contest_id_rejects_non_contest_url() {
        let err = normalize_contest_id_input("https://atcoder.jp/contests/abc411/tasks/abc411_a")
            .expect_err("task URL should be rejected");
        assert!(err
            .to_string()
            .contains("contest URL must point to a contest page"));
    }

    #[test]
    fn cli_run_parses_bin_only() -> Result<()> {
        let cli = Cli::try_parse_from(["kp", "run", "a"])?;
        match cli.command {
            Commands::Run { params, debug } => {
                assert_eq!(params, vec!["a"]);
                assert!(!debug);
            }
            other => anyhow::bail!("unexpected command: {:?}", other),
        }
        Ok(())
    }

    #[test]
    fn cli_run_parses_contest_bin_and_debug() -> Result<()> {
        let cli = Cli::try_parse_from(["kp", "run", "abc300", "a", "--debug"])?;
        match cli.command {
            Commands::Run { params, debug } => {
                assert_eq!(params, vec!["abc300", "a"]);
                assert!(debug);
            }
            other => anyhow::bail!("unexpected command: {:?}", other),
        }
        Ok(())
    }

    #[test]
    fn cli_login_parses_options() -> Result<()> {
        let cli = Cli::try_parse_from([
            "kp",
            "login",
            "--tools",
            "oj",
            "acc",
            "--session",
            "REVEL_SESSION=abc123",
            "--oj-cookie-path",
            "/tmp/oj-cookie.jar",
            "--acc-cookie-path",
            "/tmp/acc-session.json",
        ])?;
        match cli.command {
            Commands::Login {
                tools,
                session,
                oj_cookie_path,
                acc_cookie_path,
            } => {
                assert_eq!(tools, vec!["oj", "acc"]);
                assert_eq!(session.as_deref(), Some("REVEL_SESSION=abc123"));
                assert_eq!(oj_cookie_path, Some(PathBuf::from("/tmp/oj-cookie.jar")));
                assert_eq!(
                    acc_cookie_path,
                    Some(PathBuf::from("/tmp/acc-session.json"))
                );
            }
            other => anyhow::bail!("unexpected command: {:?}", other),
        }
        Ok(())
    }

    #[test]
    fn normalize_revel_session_input_accepts_cookie_assignment() {
        assert_eq!(
            normalize_revel_session_input("REVEL_SESSION=abc123"),
            "abc123"
        );
        assert_eq!(
            normalize_revel_session_input("REVEL_SESSION=abc123; Path=/; Secure"),
            "abc123"
        );
        assert_eq!(normalize_revel_session_input("  abc123  "), "abc123");
    }

    #[test]
    fn update_acc_session_json_preserves_other_fields() -> Result<()> {
        let updated = update_acc_session_json(
            Some(
                r#"{
  "cookies": [
    "REVEL_FLASH=",
    "foo=bar",
    "REVEL_SESSION=old"
  ],
  "metadata": {
    "user": "wogikaze"
  }
}"#,
            ),
            "new",
        )?;

        let value: serde_json::Value = serde_json::from_str(&updated)?;
        assert_eq!(
            value["cookies"],
            serde_json::json!(["REVEL_FLASH=", "foo=bar", "REVEL_SESSION=new"])
        );
        assert_eq!(value["metadata"]["user"], "wogikaze");
        Ok(())
    }

    #[test]
    fn update_acc_session_json_adds_flash_cookie_when_missing() -> Result<()> {
        let updated = update_acc_session_json(Some(r#"{"cookies":["foo=bar"]}"#), "new")?;
        let value: serde_json::Value = serde_json::from_str(&updated)?;
        assert_eq!(
            value["cookies"],
            serde_json::json!(["REVEL_FLASH=", "foo=bar", "REVEL_SESSION=new"])
        );
        Ok(())
    }

    #[test]
    fn update_oj_cookie_jar_replaces_revel_session_and_keeps_other_lines() {
        let updated = update_oj_cookie_jar(
            Some(
                "#LWP-Cookies-2.0\nSet-Cookie3: other=value; path=\"/\"; domain=\"example.com\"; path_spec; version=0\nSet-Cookie3: REVEL_SESSION=old; path=\"/\"; domain=\"atcoder.jp\"; path_spec; secure; version=0\n",
            ),
            "new=value",
        );

        assert!(updated.contains("#LWP-Cookies-2.0\n"));
        assert!(updated.contains(
            "Set-Cookie3: other=value; path=\"/\"; domain=\"example.com\"; path_spec; version=0\n"
        ));
        assert!(!updated.contains("REVEL_SESSION=old"));
        assert!(updated.contains("Set-Cookie3: REVEL_SESSION=\"new=value\"; path=\"/\"; domain=\"atcoder.jp\"; path_spec; secure; expires=\"2099-12-31 23:59:59Z\"; HttpOnly=None; version=0\n"));
        assert!(updated.ends_with('\n'));
    }

    #[test]
    fn update_vscode_settings_accepts_jsonc_and_formats_output() -> Result<()> {
        let dir = TestDir::new("vscode-settings-jsonc")?;
        let settings_path = dir.path().join("settings.json");
        fs::write(
            &settings_path,
            r#"{
  // keep parsing comments
  "editor.tabSize": 2,
  "rust-analyzer.linkedProjects": [
    "./zzz/Cargo.toml",
    "./aaa/Cargo.toml"
  ]
}
"#,
        )?;

        update_vscode_linked_project_settings(&settings_path, "bbb")?;

        let text = fs::read_to_string(&settings_path)?;
        assert!(text.ends_with('\n'));

        let value: serde_json::Value = serde_json::from_str(&text)?;
        assert_eq!(value["editor.tabSize"], 2);
        assert_eq!(
            value["rust-analyzer.linkedProjects"],
            serde_json::json!(["./aaa/Cargo.toml", "./bbb/Cargo.toml", "./zzz/Cargo.toml"])
        );
        Ok(())
    }

    #[test]
    fn update_vscode_settings_does_not_overwrite_invalid_jsonc() -> Result<()> {
        let dir = TestDir::new("vscode-settings-invalid")?;
        let settings_path = dir.path().join("settings.json");
        let original = "{\n  \"editor.tabSize\": 2,,\n}\n";
        fs::write(&settings_path, original)?;

        let err = update_vscode_linked_project_settings(&settings_path, "abc999")
            .expect_err("invalid JSONC should return an error");
        assert!(err.to_string().contains("failed to parse"));
        assert_eq!(fs::read_to_string(&settings_path)?, original);
        Ok(())
    }

    #[test]
    fn update_vscode_settings_rejects_invalid_linked_projects_type() -> Result<()> {
        let dir = TestDir::new("vscode-settings-invalid-linked-projects")?;
        let settings_path = dir.path().join("settings.json");
        let original = r#"{
  "rust-analyzer.linkedProjects": 1
}
"#;
        fs::write(&settings_path, original)?;

        let err = update_vscode_linked_project_settings(&settings_path, "abc999")
            .expect_err("invalid linkedProjects type should return an error");
        assert!(err
            .to_string()
            .contains("must be a string or an array of strings"));
        assert_eq!(fs::read_to_string(&settings_path)?, original);
        Ok(())
    }
}

fn add_vscode_linked_project(contest_id: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let vscode_dir = cwd.join(".vscode");
    let settings_path = vscode_dir.join("settings.json");

    // Ensure .vscode exists
    fs::create_dir_all(&vscode_dir)?;

    update_vscode_linked_project_settings(&settings_path, contest_id)
}

fn update_vscode_linked_project_settings(settings_path: &Path, contest_id: &str) -> Result<()> {
    use serde_json::{Map, Value};

    let new_entry = format!("./{}/Cargo.toml", contest_id);
    let mut value = if settings_path.exists() {
        let text = fs::read_to_string(settings_path)
            .with_context(|| format!("failed to read {}", settings_path.display()))?;
        match jsonc_parser::parse_to_serde_value(&text, &Default::default())
            .with_context(|| format!("failed to parse {} as JSONC", settings_path.display()))?
        {
            Some(value) => value,
            None => Value::Object(Map::new()),
        }
    } else {
        Value::Object(Map::new())
    };

    let root = value
        .as_object_mut()
        .with_context(|| format!("{} must contain a JSON object", settings_path.display()))?;

    let key = "rust-analyzer.linkedProjects";
    let existing = root.remove(key);
    let mut linked_projects = match existing {
        None => Vec::new(),
        Some(Value::String(path)) => vec![path],
        Some(Value::Array(values)) => values
            .into_iter()
            .map(|value| match value {
                Value::String(path) => Ok(path),
                _ => anyhow::bail!(
                    "`{}` in {} must be an array of strings",
                    key,
                    settings_path.display()
                ),
            })
            .collect::<Result<Vec<_>>>()?,
        Some(_) => anyhow::bail!(
            "`{}` in {} must be a string or an array of strings",
            key,
            settings_path.display()
        ),
    };

    linked_projects.push(new_entry);
    linked_projects.sort();
    linked_projects.dedup();

    root.insert(
        key.to_string(),
        Value::Array(linked_projects.into_iter().map(Value::String).collect()),
    );

    let formatted = serde_json::to_string_pretty(&value)
        .with_context(|| format!("failed to format {}", settings_path.display()))?;
    fs::write(settings_path, format!("{}\n", formatted))
        .with_context(|| format!("failed to write {}", settings_path.display()))?;
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
        if let Ok(status) = Command::new("xdg-open").arg(url).status() {
            if status.success() {
                return Ok(());
            }
        }
        Command::new("/mnt/c/Windows/explorer.exe")
            .arg(url)
            .spawn()?;
    }
    Ok(())
}
