use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::tempdir;

fn formatter_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pythonjsx-format"))
}

const BLOCK_A: &str = "x = (\n    <a href={url}>\n        Home\n    </a>\n)\n";

#[test]
fn stdout_mode_formats_file() {
    let td = tempdir().unwrap();
    let input = td.path().join("input.px");
    fs::write(&input, "x = <div><Header /><Content /></div>\n").unwrap();

    let output = formatter_cmd().arg(&input).output().unwrap();

    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "x = (\n    <div>\n        <Header />\n        <Content />\n    </div>\n)\n"
    );
}

#[test]
fn check_mode_reports_unformatted_input() {
    let td = tempdir().unwrap();
    let input = td.path().join("input.px");
    fs::write(&input, "x = <div><Header /><Content /></div>\n").unwrap();

    let output = formatter_cmd().arg("--check").arg(&input).output().unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("would be reformatted"));
}

#[test]
fn check_mode_accepts_formatted_input() {
    let td = tempdir().unwrap();
    let input = td.path().join("input.px");
    fs::write(&input, "x = <a href={url}>Home</a>\n").unwrap();

    let output = formatter_cmd().arg("--check").arg(&input).output().unwrap();

    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn output_mode_writes_requested_file() {
    let td = tempdir().unwrap();
    let input = td.path().join("input.px");
    let output_path = td.path().join("output.px");
    fs::write(&input, "x = <a\n    href={url}\n>\n    Home\n</a>\n").unwrap();

    let output = formatter_cmd()
        .arg("-o")
        .arg(&output_path)
        .arg(&input)
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read_to_string(output_path).unwrap(), BLOCK_A);
}

#[test]
fn in_place_mode_rewrites_input_file() {
    let td = tempdir().unwrap();
    let input = td.path().join("input.px");
    fs::write(&input, "x = <a\n    href={url}\n>\n    Home\n</a>\n").unwrap();

    let output = formatter_cmd().arg("--in-place").arg(&input).output().unwrap();

    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read_to_string(input).unwrap(), BLOCK_A);
}

#[test]
fn stdin_mode_formats_to_stdout() {
    let mut child = formatter_cmd()
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"x = <a\n    href={url}\n>\n    Home\n</a>\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), BLOCK_A);
}

#[test]
fn collapse_multiline_mode_collapses_simple_multiline_input() {
    let td = tempdir().unwrap();
    let input = td.path().join("input.px");
    fs::write(&input, "x = <a\n    href={url}\n>\n    Home\n</a>\n").unwrap();

    let output = formatter_cmd()
        .arg("--collapse-multiline")
        .arg(&input)
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "x = <a href={url}>Home</a>\n");
}

#[test]
fn invalid_jsx_exits_nonzero_without_output() {
    let td = tempdir().unwrap();
    let input = td.path().join("input.px");
    fs::write(&input, "x = <div></span>\n").unwrap();

    let output = formatter_cmd().arg(&input).output().unwrap();

    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid PythonJSX"));
}
