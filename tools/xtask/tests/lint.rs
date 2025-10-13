use std::env;
use std::path::PathBuf;
use std::process::Command;

use sys::tempfile::tempdir;

struct CommandOutput {
    stdout: String,
    stderr: String,
}

fn run_success(mut command: Command) -> CommandOutput {
    let output = command
        .output()
        .expect("failed to spawn command during lint test");
    if !output.status.success() {
        panic!(
            "command {:?} failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
            &command,
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn find_xtask_binary() -> Option<PathBuf> {
    if let Ok(path) = env::var("CARGO_BIN_EXE_xtask") {
        return Some(PathBuf::from(path));
    }

    let mut path = env::current_exe().ok()?;
    if !path.pop() {
        return None;
    }
    if !path.pop() {
        return None;
    }
    path.push(format!("xtask{}", env::consts::EXE_SUFFIX));
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

#[test]
fn detects_balance_change() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join("repo")).unwrap();
    let repo = dir.path().join("repo");
    run_success({
        let mut cmd = Command::new("git");
        cmd.args(["init"]).current_dir(&repo);
        cmd
    });
    std::fs::write(repo.join("file"), "balance = 0").unwrap();
    run_success({
        let mut cmd = Command::new("git");
        cmd.args(["add", "."]).current_dir(&repo);
        cmd
    });
    run_success({
        let mut cmd = Command::new("git");
        cmd.args([
            "-c",
            "user.email=a@a",
            "-c",
            "user.name=a",
            "commit",
            "-m",
            "init",
        ])
        .current_dir(&repo);
        cmd
    });
    std::fs::write(repo.join("file"), "balance = 1").unwrap();
    run_success({
        let mut cmd = Command::new("git");
        cmd.args(["add", "."]).current_dir(&repo);
        cmd
    });
    run_success({
        let mut cmd = Command::new("git");
        cmd.args([
            "-c",
            "user.email=a@a",
            "-c",
            "user.name=a",
            "commit",
            "-am",
            "change",
        ])
        .current_dir(&repo);
        cmd
    });

    let xtask_bin = find_xtask_binary().expect("missing xtask binary path");
    let output = run_success({
        let mut cmd = Command::new(&xtask_bin);
        cmd.current_dir(&repo)
            .args(["summary", "--base=HEAD~1", "--title=[core]"]);
        cmd
    });
    assert!(
        output.stdout.contains("\"balance_changed\": true"),
        "expected balance change flag in xtask output, stdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
}
