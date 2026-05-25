use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn binary_shows_help() {
    let mut cmd = Command::cargo_bin("mikrom").unwrap();
    cmd.arg("--help");
    cmd.assert().success().stdout(contains("Usage:"));
}

#[test]
fn binary_shows_version() {
    let mut cmd = Command::cargo_bin("mikrom").unwrap();
    cmd.arg("--version");
    cmd.assert().success().stdout(contains("0.3.0"));
}

#[test]
fn completion_subcommand_generates_bash() {
    let mut cmd = Command::cargo_bin("mikrom").unwrap();
    cmd.args(["completion", "bash"]);
    cmd.assert().success().stdout(contains("_mikrom"));
}

#[test]
fn completion_subcommand_generates_zsh() {
    let mut cmd = Command::cargo_bin("mikrom").unwrap();
    cmd.args(["completion", "zsh"]);
    cmd.assert().success().stdout(contains("#compdef"));
}

#[test]
fn completion_subcommand_generates_fish() {
    let mut cmd = Command::cargo_bin("mikrom").unwrap();
    cmd.args(["completion", "fish"]);
    cmd.assert().success().stdout(contains("complete"));
}

#[test]
fn system_health_with_no_server_fails() {
    let mut cmd = Command::cargo_bin("mikrom").unwrap();
    cmd.args(["--no-color", "system", "health"]);
    // Should fail because there is no server running
    cmd.assert().failure().stderr(contains("Error:"));
}

#[test]
fn verbose_flag_is_accepted() {
    let mut cmd = Command::cargo_bin("mikrom").unwrap();
    cmd.args(["-v", "--help"]);
    cmd.assert().success();
}

#[test]
fn no_color_flag_is_accepted() {
    let mut cmd = Command::cargo_bin("mikrom").unwrap();
    cmd.args(["--no-color", "--help"]);
    cmd.assert().success();
}

#[test]
fn config_show_without_config_file_works() {
    let mut cmd = Command::cargo_bin("mikrom").unwrap();
    cmd.env_remove("MIKROM_API_URL");
    cmd.args(["config", "show", "--output", "json"]);
    cmd.assert().success().stdout(contains("api_url"));
}

#[test]
fn unknown_subcommand_fails() {
    let mut cmd = Command::cargo_bin("mikrom").unwrap();
    cmd.arg("foobar");
    cmd.assert().failure();
}
