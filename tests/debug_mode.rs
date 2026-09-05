#![cfg(debug_assertions)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_FILE: AtomicUsize = AtomicUsize::new(0);

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mapbox-gl-js")
}

fn config_file(contents: &str) -> PathBuf {
    let index = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("gitlsd-test-{}-{index}.conf", std::process::id()));
    fs::write(&path, contents).unwrap();
    path
}

fn run(script: &str, config: &str) -> Output {
    let script = if script.is_empty() {
        "p".to_owned()
    } else {
        format!("p;{script}")
    };
    command()
        .args([
            "--config",
            config_file(config).to_str().unwrap(),
            "--debug",
            &script,
        ])
        .current_dir(fixture())
        .output()
        .unwrap()
}

fn run_preview(script: &str, config: &str) -> Output {
    command()
        .args([
            "--config",
            config_file(config).to_str().unwrap(),
            "--debug",
            script,
        ])
        .current_dir(fixture())
        .output()
        .unwrap()
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gitlsd"));
    command.env("LC_ALL", "C").env("LANG", "C");
    command
}

fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn scripted_navigation_selects_expected_commit_and_prints_rows() {
    let initial = stdout(run("", ""));
    assert!(initial.contains("selected=0\n"));
    assert!(initial.contains("commit=446bbe66962e288ae9b2ab8b500b92dfa4da892a\n"));

    let output = stdout(run("down;down;up", ""));
    assert!(output.contains("selected=1\n"));
    assert!(output.contains("commit=aaed0069a29bd77ca37a12f4477b41eb3fa9572f\n"));
    assert!(output.contains("rows:\n"));
    assert!(output.contains("> 1 aaed006"));
}

#[test]
fn crossing_batch_boundary_loads_additional_distinct_commits() {
    let output = stdout(run("down;down", "set batch-size 2\n"));
    assert!(output.contains("loaded=4\n"));
    assert!(output.contains("selected=2\n"));
    assert!(output.contains("commit=61a574eb73905ede0e7d94fb22e1ba0ec4d9a00a\n"));
    assert_eq!(output.matches("Fix globe draping glitch\n").count(), 1);
    assert_eq!(output.matches("GL JS v3.30.0\n").count(), 1);
}

#[test]
fn search_repeats_forward_and_backward_with_wraparound() {
    let unloaded = stdout(run("/;C;o;r;r;e;c;t;l;y;enter", "set batch-size 2\n"));
    assert!(unloaded.contains("commit=f2138f311d2a3a64a4121756ffda9cbef84c527b\n"));
    assert!(unloaded.contains("loaded=4\n"));

    let next = stdout(run("/;G;L;space;J;S;enter;n", "set batch-size 10\n"));
    assert!(next.contains("commit=5b3e8f5e72efbdd144143a2cbbcd5e6477491433\n"));

    let previous = stdout(run("/;G;L;space;J;S;enter;n;N", "set batch-size 10\n"));
    assert!(previous.contains("commit=aaed0069a29bd77ca37a12f4477b41eb3fa9572f\n"));
    assert!(previous.contains("status=Match for `GL JS`\n"));
}

#[test]
fn help_reflects_effective_configuration_and_quit_is_clean() {
    let help = stdout(run(
        ":;h;e;l;p;enter",
        "set log git log --oneline --all\nset batch-size 3\nbind x move-down\n",
    ));
    assert!(help.contains("screen=help\n"));
    assert!(help.contains("setting.log=\"git\" \"log\" \"--oneline\" \"--all\"\n"));
    assert!(help.contains("setting.batch-size=3\n"));
    assert!(help.contains("binding.x=move-down\n"));

    let quit = stdout(run(":;q;enter", ""));
    assert!(quit.contains("running=false\n"));
    assert!(quit.contains("status=Quit requested\n"));
}

#[test]
fn custom_bindings_dispatch_for_literal_and_named_separator_keys() {
    let literal = stdout(run("x", "bind x move-down\n"));
    assert!(literal.contains("selected=1\n"));

    let semicolon = stdout(run("semicolon", "bind semicolon move-down\n"));
    assert!(semicolon.contains("selected=1\n"));
}

#[test]
fn configured_git_argv_preserves_format_and_filters_history() {
    let output = stdout(run(
        "",
        "set log git log --format='CUSTOM %s' --fixed-strings --grep 'Correctly validate'\n",
    ));
    assert!(output.contains("commit=f2138f311d2a3a64a4121756ffda9cbef84c527b\n"));
    assert!(output.contains("display=CUSTOM Correctly validate light transition properties\n"));
}

#[test]
fn preview_uses_selected_non_head_commit_id() {
    let output = stdout(run_preview(
        "down",
        "set preview git show --format=%H --no-patch\n",
    ));
    let selected = "aaed0069a29bd77ca37a12f4477b41eb3fa9572f";
    assert!(output.contains(&format!("commit={selected}\n")));
    assert!(
        output
            .split("preview:\n")
            .nth(1)
            .is_some_and(|preview| preview.contains(selected))
    );
}

#[test]
fn rows_match_git_with_pagination_and_pathspec() {
    for options in [
        vec!["--oneline", "--decorate"],
        vec!["--format=LABEL %h %s", "--", "package.json"],
    ] {
        let config = format!(
            "set batch-size 2\nset log git log {}\n",
            options
                .iter()
                .map(|arg| format!("'{arg}'"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let output = stdout(run("down;down", &config));
        let mut expected = Vec::new();
        for skip in [0, 2] {
            let mut args = options.clone();
            let skip = format!("--skip={skip}");
            let insertion = args
                .iter()
                .position(|arg| *arg == "--")
                .unwrap_or(args.len());
            args.splice(insertion..insertion, [&skip, "--max-count=2"]);
            let git = Command::new("git")
                .arg("--no-pager")
                .arg("log")
                .args(args)
                .current_dir(fixture())
                .output()
                .unwrap();
            assert!(git.status.success());
            expected.extend(
                String::from_utf8(git.stdout)
                    .unwrap()
                    .lines()
                    .map(str::to_owned),
            );
        }
        let actual: Vec<_> = output
            .split("rows:\n")
            .nth(1)
            .unwrap()
            .lines()
            .map(|line| line[2..].split_once(' ').unwrap().1.to_owned())
            .collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn custom_display_search_and_color_are_plain_in_debug() {
    let config = "set batch-size 2\nset log git log --color=always --format='%C(red)ONLYDISPLAY%C(reset) %s'\n";
    let output = stdout(run("/;O;N;L;Y;D;I;S;P;L;A;Y;enter;n;N", config));
    assert!(output.contains("selected=1\n"));
    assert!(output.contains("display=ONLYDISPLAY GL JS v3.30.0\n"));
    assert!(!output.contains('\x1b'));
}

#[test]
fn incompatible_configuration_has_location_and_actionable_error() {
    for command in [
        "log --oneline",
        "git status",
        "git log",
        "git log --oneline --graph",
        "git log --oneline --stat",
        "git log --oneline -p",
        "git log --oneline --notes",
        "git log --oneline --show-signature",
        "git log --format=%B",
        "git log --format=%h%n%s",
    ] {
        let output = run("", &format!("set log {command}\n"));
        assert!(!output.status.success(), "{command}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(".conf:1:"), "{error}");
        assert!(
            error.contains("one-line") || error.contains("required form"),
            "{error}"
        );
    }
}

#[test]
fn discovers_default_config_from_home() {
    let index = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let home = std::env::temp_dir().join(format!("gitlsd-home-{}-{index}", std::process::id()));
    let config_directory = home.join(".config/gitlsd");
    fs::create_dir_all(&config_directory).unwrap();
    fs::write(
        config_directory.join("config"),
        "set batch-size 1\nbind x move-down\n",
    )
    .unwrap();

    let output = command()
        .args(["--debug", "x"])
        .env("HOME", &home)
        .current_dir(fixture())
        .output()
        .unwrap();
    let output = stdout(output);
    assert!(output.contains("selected=1\n"));
    assert!(output.contains("loaded=2\n"));
}

#[test]
fn invalid_script_key_and_config_return_actionable_errors() {
    let invalid_key = run_preview("not-a-key", "");
    assert!(!invalid_key.status.success());
    assert!(
        String::from_utf8_lossy(&invalid_key.stderr)
            .contains("script key 1: unknown key `not-a-key`")
    );

    let invalid_config = run("", "set batch-size nope\n");
    assert!(!invalid_config.status.success());
    let error = String::from_utf8_lossy(&invalid_config.stderr);
    assert!(error.contains(":1: batch size must be a positive integer"));
}

#[test]
fn git_failure_outside_repository_is_actionable() {
    let directory = std::env::temp_dir().join(format!("gitlsd-not-repo-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let output = command()
        .args(["--config", config_file("").to_str().unwrap(), "--debug", ""])
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("Git log failed"));
    assert!(error.contains("not a git repository"));
}

#[test]
fn preview_is_enabled_by_default_and_custom_command_is_used() {
    let default = stdout(run_preview("", ""));
    assert!(default.contains("preview.visible=true\n"));
    assert!(default.contains("preview:\n"));
    assert!(default.contains("Fix globe draping glitch"));

    let custom = stdout(run_preview(
        "",
        "set preview git show --format='CUSTOM %H' --no-patch\n",
    ));
    assert!(custom.contains("  CUSTOM 446bbe66962e288ae9b2ab8b500b92dfa4da892a\n"));
}

#[test]
fn preview_toggle_focus_scrolling_and_search_are_scriptable() {
    let hidden = stdout(run_preview("p", ""));
    assert!(hidden.contains("preview.visible=false\n"));
    assert!(!hidden.contains("preview:\n"));

    let focused = stdout(run_preview("enter;down;/;F;i;x;enter", ""));
    assert!(focused.contains("preview.focused=true\n"));
    assert!(focused.contains("preview.offset="));
    assert!(focused.contains("selected=0\n"));
    assert!(focused.contains("status=Match for `Fix`\n"));
}

#[test]
fn preview_command_failure_is_recoverable() {
    let output = stdout(run_preview(
        "down",
        "set preview git show --not-a-real-option\n",
    ));
    assert!(output.contains("selected=1\n"));
    assert!(output.contains("status=Could not load preview:"));
}
