use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn detects_balance_change() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join("repo")).unwrap();
    Command::new("git").args(["init"]).current_dir(dir.path().join("repo")).assert().success();
    let repo = dir.path().join("repo");
    std::fs::write(repo.join("file"), "balance = 0").unwrap();
    Command::new("git").args(["add", "."]).current_dir(&repo).assert().success();
    Command::new("git").args(["-c", "user.email=a@a", "-c", "user.name=a", "commit", "-m", "init"]).current_dir(&repo).assert().success();
    std::fs::write(repo.join("file"), "balance = 1").unwrap();
    Command::new("git").args(["add", "."]).current_dir(&repo).assert().success();
    Command::new("git").args(["-c", "user.email=a@a", "-c", "user.name=a", "commit", "-am", "change"]).current_dir(&repo).assert().success();
    Command::cargo_bin("xtask")
        .unwrap()
        .current_dir(&repo)
        .args(["--base=HEAD~1", "--title=[core]"])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"balance_changed\": true"));
}
