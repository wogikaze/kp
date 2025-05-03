// kp: AtCoder project management CLI
// ------------------------------------------------------------
// * kp new <contest_id>      : generate contest workspace
// * kp test <contest_id> <problem> : build & `oj test` a single task
// ------------------------------------------------------------

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::{BufReader, Write},
    path::{Path, PathBuf},
    process::{exit, Command},
};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Init the kp-rust template
    Init {},
    /// Create a new contest workspace
    New {
        /// Contest ID (e.g. abc300)
        contest: String,
    },
    /// Build & `oj test` a problem
    Test {
        /// Contest ID (e.g. abc300)
        contest: String,
        /// Problem ID letter (e.g. a)
        problem: String,
    },
}
#[derive(Deserialize)]
struct Input {
    tasks: Vec<Task>,
}

#[derive(Deserialize)]
struct Task {
    /// e.g. "A", "B", …
    label: String,
    directory: Directory,
}

#[derive(Deserialize)]
struct Directory {
    /// e.g. "a.rs"
    submit: String,
}
fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err}");
        exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Init {} => init_template(),
        Cmd::New { contest } => create_contest(&contest),
        Cmd::Test { contest, problem } => test_problem(&contest, &problem),
    }
}

//
// -------- sub-command implementations
//
fn command(command_str: &str) -> Command {
    if cfg!(target_os = "windows") {
        let mut cmd = Command::new("powershell");
        cmd.arg("-Command").arg(command_str);
        cmd
    } else {
        Command::new(command_str)
    }
}
/// `kp init`
fn init_template() -> Result<()> {
    // 1. Obtain the path printed by `acc config-dir`
    let output = command("acc")
        .arg("config-dir")
        .output()
        .context("failed to start `acc config-dir`")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "`acc config-dir` exited with status {}",
            output.status
        ));
    }
    let config_dir = String::from_utf8(output.stdout)
        .context("`acc config-dir` produced non-UTF-8 output")?
        .trim()
        .replace("\r\n", "")
        .replace('\n', "");
    // Remove trailing new-line(s) and convert to PathBuf
    let config_dir = PathBuf::from(config_dir.trim());

    // 2. Decide whether `kp-rust` exists
    let kp_path = config_dir.join("kp-rust");

    if kp_path.exists() {
        // 3-a. Pull the latest changes
        let status = command("git")
            .arg("pull")
            .current_dir(&kp_path)
            .status()
            .context("failed to run `git pull`")?;

        if !status.success() {
            return Err(anyhow::anyhow!("`git pull` failed with status {}", status));
        }
    } else {
        // 3-b. Clone the repository
        let status = command("git")
            .arg("clone")
            .arg("https://github.com/wogikaze/kp-rust")
            .current_dir(&config_dir)
            .status()
            .context("failed to run `git clone`")?;

        if !status.success() {
            return Err(anyhow::anyhow!("`git clone` failed with status {}", status));
        }
    }

    // 4. Set Config the template
    let default_template = command("acc")
        .arg("config")
        .arg("default-template")
        .output()
        .context("failed to run `acc config default-template`")?;

    let status = default_template.status;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "`acc config default-template` failed with status {}",
            status
        ));
    }
    let current_template = String::from_utf8(default_template.stdout)
        .context("`acc config default-template` produced non-UTF-8 output")?;
    if current_template.trim() != "kp-rust" {
        // acc config default-template
        let set_template = command("acc")
            .args(["config", "default-template", "kp-rust"])
            .status()
            .context("failed to run `acc config default-template kp-rust`")?;
        if !set_template.success() {
            return Err(anyhow::anyhow!(
                "`acc config default-template kp-rust` failed with status {}",
                set_template
            ));
        }
    }
    command("acc")
        .args(["config", "default-task-dirname-format", "./"])
        .status()
        .context("failed to run `acc config default-task-dirname-format ./`")?;

    command("acc")
        .args(["config", "default-task-choice", "all"])
        .status()
        .context("failed to run `acc config default-task-choice all`")?;

    Ok(())
}

/// `kp new`
fn create_contest(contest: &str) -> Result<()> {
    let root = Path::new(contest);
    if root.exists() {
        bail!("Directory {contest} already exists");
    }
    // Remove directories
    // Create the contest directory
    command("acc")
        .args(["new", contest])
        .status()
        .context(format!("failed to run `acc new {}`", contest))?;

    // -------- 0. get directory argument --------
    let json_path = Path::new(contest).join("contest.acc.json");

    // -------- 1. read JSON --------
    let file =
        fs::File::open(&json_path).with_context(|| format!("cannot open {:?}", json_path))?;
    let input: Input = serde_json::from_reader(file)?;

    // -------- 2. load Cargo.toml (project root) --------
    let cargo_path = Path::new(contest).join("Cargo.toml");
    let mut doc = fs::read_to_string(&cargo_path)?.parse::<DocumentMut>()?;

    // ① Ensure [[bin]] is an ArrayOfTables, not a Value::Array
    if doc.get("bin").is_none() {
        doc["bin"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    let bins = doc["bin"]
        .as_array_of_tables_mut() // ✅ correct accessor
        .expect("`bin` must be an array-of-tables");

    for task in input.tasks {
        let name = task.label.to_lowercase();
        let path = format!("{}", task.directory.submit);

        // ② Each element is &Table, so we can inspect keys normally
        if bins
            .iter()
            .any(|tbl: &Table| tbl.get("name").and_then(|v| v.as_str()) == Some(name.as_str()))
        {
            continue; // already present
        }

        // ③ Push a new table
        let mut t = Table::new();
        t["name"] = name.clone().into();
        t["path"] = path.into();
        t.set_implicit(true); // no '{}' braces
        bins.push(t);
    }

    fs::write(&cargo_path, doc.to_string())?;

    // .vscode/settings.jsonに追加

    // Construct the path we want to add: "./<contest>/Cargo.toml".
    let new_entry = format!("./{contest}/Cargo.toml");

    // Path to VS Code settings.
    let settings_path = Path::new(".vscode/settings.json");

    // Ensure the .vscode directory exists.
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Open the file for reading if it exists; otherwise start with an empty JSON object.
    let mut root: Value = if settings_path.exists() {
        let file = File::open(settings_path)
            .with_context(|| format!("Failed to open {}", settings_path.display()))?;
        serde_json::from_reader(BufReader::new(file))
            .with_context(|| format!("{} is not valid JSON", settings_path.display()))?
    } else {
        json!({})
    };

    // Navigate to rust-analyzer.linkedProjects, creating intermediate objects as needed.
    let linked_projects = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json must be a JSON object"))?
        .entry("rust-analyzer.linkedProjects")
        .or_insert_with(|| Value::Array(Vec::new()));

    // Ensure the field is an array.
    let arr = linked_projects
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("rust-analyzer.linkedProjects must be an array"))?;

    // Append if not already present.
    if !arr.iter().any(|v| v == &Value::String(new_entry.clone())) {
        arr.push(Value::String(new_entry));
    } else {
        println!("Entry already present; nothing to do.");
        return Ok(());
    }

    // Write back atomically: serialize pretty-printed JSON then rename.
    let tmp_path = settings_path.with_extension("json.tmp");
    let mut tmp_file = File::create(&tmp_path)?;
    tmp_file.write_all(serde_json::to_string_pretty(&root)?.as_bytes())?;
    fs::rename(tmp_path, settings_path)?;

    println!("Added new linked project successfully.");

    Ok(())
}

/// `kp test`
fn test_problem(contest: &str, problem: &str) -> Result<()> {
    let dir = Path::new(contest);
    if !dir.exists() {
        bail!("{} does not exist", dir.display());
    }
    // oj test -c "cargo run --bin a -d "testcases/a"
    println!("🧪  oj test");
    
    let run_cmd = if cfg!(target_os = "windows") {
        format!("\"cargo run --bin {problem}\"")
    } else {
        format!("cargo run --bin {problem}")
    };

    command("oj")
        .current_dir(Path::new(&dir))
        .args(["test", "-c", &run_cmd])
        .args(["-d", &format!("testcases/{problem}")])
        .status()?
        .success()
        .then_some(());

    Ok(())
}
