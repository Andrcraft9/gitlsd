#![cfg(debug_assertions)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;

static NEXT_FILE: AtomicUsize = AtomicUsize::new(0);

#[cfg(unix)]
fn presentation_filter() -> &'static str {
    // Keep headers intact while coloring all lines and transforming content.
    "set diff-filter = sed 's/^/\x1b[31m/; s/$/\x1b[m/; s/changed/FILTERED/; s/worktree/FILTERED/; /txt/!s/untracked/FILTERED/; /txt/!s/staged/FILTERED/; s/variants/FILTERED-METADATA/'\n"
}

#[cfg(unix)]
#[test]
fn diff_filter_transforms_all_views_and_preserves_navigation_and_search() {
    let show = show_fixture();
    let config = presentation_filter();
    let preview = stdout(run_in(&show, "", config));
    assert!(preview.contains("+FILTERED"), "{preview}");
    assert!(!preview.contains('\x1b'));
    for script in [
        "d;down;enter",
        "d;enter;page-down;page-down",
        "d;enter;right",
        "d;enter;end",
    ] {
        let filtered = stdout(run_in(&show, script, config));
        let plain = stdout(run_in(&show, script, ""));
        for prefix in [
            "show.selected=",
            "show.diff.offset=",
            "show.focus=",
            "show.diff.horizontal-offset=",
        ] {
            assert_eq!(
                filtered.lines().find(|line| line.starts_with(prefix)),
                plain.lines().find(|line| line.starts_with(prefix)),
                "{script}: {prefix}"
            );
        }
        assert!(!filtered.contains("Patch location unavailable"));
        assert!(!filtered.contains('\x1b'));
    }
    let searched = stdout(run_in(&show, "d;enter;/;F;I;L;T;E;R;E;D;enter", config));
    assert!(searched.contains("status=Match for `FILTERED`"));
    assert!(searched.contains("    variants"));
    assert!(!searched.contains("    FILTERED-METADATA")); // Metadata bypasses the filter.

    let status = status_fixture();
    for script in [
        "s;enter",
        "s;down;enter",
        "s;tab;down;enter",
        "s;enter;page-down",
    ] {
        let filtered = stdout(run_in(&status, script, config));
        let plain = stdout(run_in(&status, script, ""));
        assert!(filtered.contains("+FILTERED"), "{script}: {filtered}");
        for prefix in ["status.selected=", "status.diff.offset=", "status.group="] {
            assert_eq!(
                filtered.lines().find(|line| line.starts_with(prefix)),
                plain.lines().find(|line| line.starts_with(prefix)),
                "{script}: {prefix}"
            );
        }
        assert!(!filtered.contains("Patch location unavailable"));
        assert!(!filtered.contains('\x1b'));
    }
}

#[cfg(unix)]
#[test]
fn filtered_status_refreshes_after_staging_and_unstaging_untracked_file() {
    let directory = status_fixture();
    let config = presentation_filter();
    let staged = stdout(run_in(&directory, "s;down;u;tab;end", config));
    assert!(
        git_output(&directory, &["diff", "--cached", "--name-only"])
            .windows(b"untracked file.txt".len())
            .any(|name| name == b"untracked file.txt")
    );
    assert!(staged.contains("+FILTERED"), "{staged}");
    let unstaged = stdout(run_in(&directory, "s;tab;down;down;down;u", config));
    assert!(unstaged.contains("?? untracked file.txt"), "{unstaged}");
    assert!(unstaged.contains("+FILTERED"));
    assert_eq!(
        fs::read(directory.join("untracked file.txt")).unwrap(),
        b"untracked\n"
    );
    assert!(
        !git_output(&directory, &["diff", "--cached", "--name-only"])
            .windows(b"untracked file.txt".len())
            .any(|name| name == b"untracked file.txt")
    );
}

#[test]
fn missing_diff_filter_errors_identify_every_affected_view() {
    let directory = status_fixture();
    let config = "set diff-filter = gitlsd-missing-filter-executable\n";
    for (script, operation) in [("", "preview"), ("d", "show diff"), ("s", "staged diff")] {
        let output = stdout(run_in(&directory, script, config));
        assert!(output.contains(&format!("diff filter `gitlsd-missing-filter-executable` for {operation} failed: could not launch")), "{output}");
    }
    let unstaged_directory = show_fixture();
    fs::write(unstaged_directory.join("space name.txt"), "unstaged\n").unwrap();
    let unstaged = stdout(run_in(&unstaged_directory, "s", config));
    assert!(
        unstaged.contains("for unstaged diff failed: could not launch"),
        "{unstaged}"
    );
    let disabled = stdout(run_in(
        &directory,
        "s",
        &format!("{config}set diff-filter =\n"),
    ));
    assert!(disabled.contains("screen=status"));
}

#[cfg(unix)]
#[test]
fn structural_filter_changes_report_missing_patch_and_empty_diffs_stay_empty() {
    let directory = show_fixture();
    let filtered = stdout(run_in(
        &directory,
        "d;enter",
        "set diff-filter = sed 's/diff --git/patch/'\n",
    ));
    assert!(filtered.contains("Patch location unavailable"));
    git(&directory, &["commit", "--allow-empty", "-qm", "empty"]);
    let empty = stdout(run_in(
        &directory,
        "d",
        "set diff-filter = printf DECORATION\n",
    ));
    assert!(!empty.contains("DECORATION"));
    let clean = stdout(run_in(
        &directory,
        "s",
        "set diff-filter = printf DECORATION\n",
    ));
    assert!(!clean.contains("DECORATION"));
}

#[cfg(unix)]
#[test]
fn header_preserving_filter_may_insert_lines_without_breaking_file_jumps() {
    let show = show_fixture();
    let config = "set diff-filter = sed '/^diff --git/i inserted-by-filter'\n";
    let jumped = stdout(run_in(&show, "d;down;enter", config));
    assert!(jumped.contains("show.selected=1\n"), "{jumped}");
    assert!(!jumped.contains("Patch location unavailable"));

    let status = status_fixture();
    let jumped = stdout(run_in(&status, "s;down;enter", config));
    assert!(jumped.contains("status.selected=1\n"), "{jumped}");
    assert!(!jumped.contains("Patch location unavailable"));
}

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

fn run_branch(branch: &str, script: &str, config: &str) -> Output {
    let script = if script.is_empty() {
        "p".to_owned()
    } else {
        format!("p;{script}")
    };
    command()
        .arg(branch)
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

fn run_in(directory: &Path, script: &str, config: &str) -> Output {
    command()
        .args([
            "--config",
            config_file(config).to_str().unwrap(),
            "--debug",
            script,
        ])
        .current_dir(directory)
        .output()
        .unwrap()
}

fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?}: {}",
        arguments,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(directory: &Path, arguments: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?}: {}",
        arguments,
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn show_fixture() -> PathBuf {
    let index = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "gitlsd-show-fixture-{}-{index}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    git(&directory, &["init", "-q"]);
    git(&directory, &["config", "user.email", "gitlsd@example.com"]);
    git(&directory, &["config", "user.name", "gitlsd"]);
    git(&directory, &["config", "diff.renames", "true"]);
    fs::write(directory.join("space name.txt"), "base\n").unwrap();
    fs::write(directory.join("unicode-é.txt"), "base\n").unwrap();
    fs::write(directory.join("deleted name.txt"), "delete\n").unwrap();
    fs::write(directory.join("rename source.txt"), "rename\n").unwrap();
    git(&directory, &["add", "."]);
    git(&directory, &["commit", "-qm", "base"]);
    fs::write(directory.join("space name.txt"), "changed\n").unwrap();
    fs::write(directory.join("unicode-é.txt"), "changed\n").unwrap();
    fs::remove_file(directory.join("deleted name.txt")).unwrap();
    fs::rename(
        directory.join("rename source.txt"),
        directory.join("renamed target.txt"),
    )
    .unwrap();
    fs::write(directory.join("added name.txt"), "added\n").unwrap();
    git(&directory, &["add", "-A"]);
    git(&directory, &["commit", "-qm", "variants"]);
    directory
}

fn status_fixture() -> PathBuf {
    let index = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "gitlsd-status-fixture-{}-{index}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    git(&directory, &["init", "-q"]);
    git(&directory, &["config", "user.email", "gitlsd@example.com"]);
    git(&directory, &["config", "user.name", "gitlsd"]);
    fs::write(directory.join("partial.txt"), "base\n").unwrap();
    fs::write(directory.join("staged.txt"), "base\n").unwrap();
    fs::write(directory.join("rename source.txt"), "rename\n").unwrap();
    git(&directory, &["add", "."]);
    git(&directory, &["commit", "-qm", "base"]);

    fs::write(directory.join("partial.txt"), "index\n").unwrap();
    git(&directory, &["add", "--", "partial.txt"]);
    fs::write(directory.join("partial.txt"), "index\nworktree\n").unwrap();
    fs::write(directory.join("staged.txt"), "staged\n").unwrap();
    git(&directory, &["add", "--", "staged.txt"]);
    git(
        &directory,
        &["mv", "rename source.txt", "renamed target.txt"],
    );
    fs::write(directory.join("untracked file.txt"), "untracked\n").unwrap();
    directory
}

#[cfg(unix)]
fn special_status_fixture() -> (PathBuf, OsString) {
    let index = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "gitlsd-special-status-fixture-{}-{index}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    git(&directory, &["init", "-q"]);
    git(&directory, &["config", "user.email", "gitlsd@example.com"]);
    git(&directory, &["config", "user.name", "gitlsd"]);
    fs::write(directory.join("base.txt"), "base\n").unwrap();
    git(&directory, &["add", "."]);
    git(&directory, &["commit", "-qm", "base"]);
    let name = OsString::from_vec(b"control\tname-\xff*.txt".to_vec());
    fs::write(directory.join(&name), "special\n").unwrap();
    (directory, name)
}

fn unborn_fixture() -> PathBuf {
    let index = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "gitlsd-unborn-fixture-{}-{index}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    git(&directory, &["init", "-q"]);
    fs::write(directory.join("worktree.txt"), "worktree\n").unwrap();
    directory
}

fn deletion_status_fixture() -> PathBuf {
    let index = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "gitlsd-deletion-status-fixture-{}-{index}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    git(&directory, &["init", "-q"]);
    git(&directory, &["config", "user.email", "gitlsd@example.com"]);
    git(&directory, &["config", "user.name", "gitlsd"]);
    fs::write(directory.join("deleted.txt"), "delete me\n").unwrap();
    git(&directory, &["add", "."]);
    git(&directory, &["commit", "-qm", "base"]);
    fs::remove_file(directory.join("deleted.txt")).unwrap();
    directory
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
fn branch_argument_loads_commits_reachable_from_that_branch() {
    let output = stdout(run_branch("main", "", "set batch-size 1\n"));
    assert!(output.contains("commit=83a053085fed98911e07d443b9c9da2f590800ce\n"));
    assert!(output.contains("Add changelog check\n"));
    assert!(!output.contains("Fix globe draping glitch\n"));
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
fn goto_command_loads_and_selects_a_commit_from_later_history() {
    let output = stdout(run(
        ":;g;o;t;o;space;f;2;1;3;8;f;3;enter",
        "set batch-size 2\n",
    ));

    assert!(output.contains("loaded=4\n"));
    assert!(output.contains("selected=3\n"));
    assert!(output.contains("commit=f2138f311d2a3a64a4121756ffda9cbef84c527b\n"));
    assert!(output.contains("status=Jumped to commit `f2138f3`\n"));
}

#[test]
fn search_repeats_without_crossing_boundaries() {
    let unloaded = stdout(run("/;C;o;r;r;e;c;t;l;y;enter", "set batch-size 2\n"));
    assert!(unloaded.contains("commit=f2138f311d2a3a64a4121756ffda9cbef84c527b\n"));
    assert!(unloaded.contains("loaded=4\n"));

    let next = stdout(run("/;G;L;space;J;S;enter;n", "set batch-size 10\n"));
    assert!(next.contains("commit=5b3e8f5e72efbdd144143a2cbbcd5e6477491433\n"));

    let previous = stdout(run("/;G;L;space;J;S;enter;n;N", "set batch-size 10\n"));
    assert!(previous.contains("commit=aaed0069a29bd77ca37a12f4477b41eb3fa9572f\n"));
    assert!(previous.contains("status=Match for `GL JS`\n"));

    let end = stdout(run(
        "/;v;3;.;3;0;.;0;-;r;c;.;1;enter;n;n",
        "set log git log --oneline HEAD~5..HEAD\nset batch-size 10\n",
    ));
    assert!(end.contains("commit=5b3e8f5e72efbdd144143a2cbbcd5e6477491433\n"));
    assert!(end.contains("status=(END)\n"));

    let top = stdout(run(
        "/;v;3;.;3;0;.;0;-;r;c;.;1;enter;N",
        "set log git log --oneline HEAD~5..HEAD\nset batch-size 10\n",
    ));
    assert!(top.contains("commit=61a574eb73905ede0e7d94fb22e1ba0ec4d9a00a\n"));
    assert!(top.contains("status=(TOP)\n"));
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
    assert!(help.contains("setting.show-commit=\"git\" \"show\""));
    assert!(help.contains("setting.show=\"git\" \"show\""));
    assert!(help.contains("binding.d=show-mode\n"));
    assert!(help.contains("binding.x=move-down\n"));

    let quit = stdout(run("q", ""));
    assert!(quit.contains("running=false\n"));
    assert!(quit.contains("status=Quit requested\n"));
}

#[test]
fn q_backs_out_through_focus_levels_before_quitting() {
    let preview = stdout(run_preview("enter;q", ""));
    assert!(preview.contains("screen=log\n"));
    assert!(preview.contains("preview.focused=false\n"));
    assert!(preview.contains("running=true\n"));

    let show = stdout(run("d;enter;q;q", ""));
    assert!(show.contains("screen=log\n"));
    assert!(show.contains("running=true\n"));

    let help = stdout(run("?;q", ""));
    assert!(help.contains("screen=log\n"));
    assert!(help.contains("running=true\n"));

    let quit = stdout(run("d;enter;q;q;q", ""));
    assert!(quit.contains("screen=log\n"));
    assert!(quit.contains("running=false\n"));
}

#[test]
fn custom_bindings_dispatch_for_literal_and_named_separator_keys() {
    let literal = stdout(run("x", "bind x move-down\n"));
    assert!(literal.contains("selected=1\n"));

    let semicolon = stdout(run("semicolon", "bind semicolon move-down\n"));
    assert!(semicolon.contains("selected=1\n"));
}

#[test]
fn horizontal_navigation_is_configurable_and_reported_in_debug_output() {
    let output = stdout(run("x", "bind x scroll-right\nbind y scroll-left\n"));
    assert!(output.contains("log.horizontal-offset=35\n"), "{output}");
    let help = stdout(run(":;h;e;l;p;enter", "bind x scroll-right\n"));
    assert!(help.contains("binding.x=scroll-right\n"));
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
fn creates_default_config_once() {
    let index = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let home = std::env::temp_dir().join(format!(
        "gitlsd-create-config-{}-{index}",
        std::process::id()
    ));
    let path = home.join(".config/gitlsd/config");

    let created = command()
        .arg("--create-config")
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    assert_eq!(
        String::from_utf8(created.stdout).unwrap(),
        format!("Created configuration at {}\n", path.display())
    );
    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains("set batch-size = 100\n"));
    assert!(contents.contains("bind j = move-down\n"));
    assert!(contents.contains("bind ctrl-c = quit\n"));

    let exists = command()
        .arg("--create-config")
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(
        exists.status.success(),
        "{}",
        String::from_utf8_lossy(&exists.stderr)
    );
    assert_eq!(
        String::from_utf8(exists.stdout).unwrap(),
        format!("Configuration already exists at {}\n", path.display())
    );
    assert_eq!(fs::read_to_string(path).unwrap(), contents);
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

#[test]
fn show_opens_full_screen_with_metadata_files_and_all_diff() {
    let output = stdout(run("d", ""));
    assert!(output.contains("screen=show\n"));
    assert!(output.contains("show.focus=explorer\n"));
    assert!(output.contains("show.selected=0\n"));
    assert!(output.contains("status=\n"));
    assert!(output.contains("show.metadata:\n"));
    assert!(output.contains("show.files:\n"));
    assert!(output.contains("M src/style/style.ts\n"));
    assert!(output.contains("show.diff:\n"));
    assert!(output.contains("diff --git a/src/style/style.ts b/src/style/style.ts"));
    assert!(output.contains("diff --git a/src/ui/map.ts b/src/ui/map.ts"));
}

#[test]
fn configured_editor_request_uses_selected_show_file_and_repository_root() {
    let output = stdout(run(
        "d;e",
        "set editor = code -g --goto file:line\nbind e open-editor",
    ));
    assert!(
        output.contains(&format!("editor.directory={}", fixture().display())),
        "{output}"
    );
    assert!(output.contains("editor.command=[\"code\", \"-g\", \"--goto\""));
    assert!(output.contains(":1\"]"), "{output}");
}

#[test]
fn show_uses_custom_commands_for_non_head_selection_but_fixed_file_list() {
    let output = stdout(run(
        "down;d",
        "set show-commit git show --format='META %H' --no-patch\nset show git show --format='DIFF %H' --patch\n",
    ));
    let selected = "aaed0069a29bd77ca37a12f4477b41eb3fa9572f";
    assert!(output.contains("screen=show\n"));
    assert!(output.contains(&format!("commit={selected}\n")));
    assert!(output.contains(&format!("META {selected}\n")));
    assert!(output.contains(&format!("DIFF {selected}\n")));
    assert!(output.contains("show.files:\n"));
    assert!(output.contains("M CHANGELOG.md\n"));
}

#[test]
fn show_real_git_handles_space_and_quoted_paths_without_filtering_the_diff() {
    let directory = show_fixture();
    let output = stdout(run_in(&directory, "d", ""));
    assert!(output.contains("screen=show\n"));
    assert!(output.contains("M space name.txt\n"));
    assert!(output.contains("M \"unicode-\\303\\251.txt\"\n"));
    assert!(output.contains("show.diff:\n"));
    assert!(output.contains("diff --git a/space name.txt b/space name.txt"));
    assert!(
        output.contains("diff --git \"a/unicode-\\303\\251.txt\" \"b/unicode-\\303\\251.txt\"")
    );

    git(&directory, &["commit", "--allow-empty", "-qm", "empty"]);
    let empty = stdout(run_in(&directory, "d", ""));
    assert!(empty.contains("screen=show\n"));
    assert!(empty.contains("show.selected=\n"));
    assert!(empty.contains("show.metadata:\n"));
    assert!(empty.contains("show.files:\n"));
}

#[test]
fn show_file_navigation_focus_and_escape_preserve_log_selection() {
    let output = stdout(run("d;down;enter;down;esc;esc", ""));
    assert!(output.contains("screen=log\n"));
    assert!(output.contains("selected=0\n"));
    assert!(output.contains("commit=446bbe66962e288ae9b2ab8b500b92dfa4da892a\n"));
    assert!(output.contains("status=\n"));
}

#[test]
fn focused_show_diff_expands_fullscreen_and_back_returns_to_files() {
    for back in ["esc", "q"] {
        let output = stdout(run(&format!("d;enter;enter;{back}"), ""));
        assert!(output.contains("screen=show\n"));
        assert!(output.contains("show.focus=explorer\n"));
        assert!(output.contains("show.diff.fullscreen=false\n"));
    }

    let fullscreen = stdout(run("d;enter;enter", ""));
    assert!(fullscreen.contains("show.focus=diff\n"));
    assert!(fullscreen.contains("show.diff.fullscreen=true\n"));
}

#[test]
fn braces_navigate_files_in_focused_diffs() {
    let show = show_fixture();
    let status = status_fixture();
    for (directory, open, mode, last) in [
        (&show, "d", "show", 4),
        (&status, "s", "status", 1),
        (&status, "s;tab", "status", 2),
    ] {
        for fullscreen in [false, true] {
            let focus = if fullscreen { "enter;enter" } else { "enter" };
            for (keys, selection) in [("}", 1), ("};{", 0), ("{", 0), ("};};};};}", last)] {
                let actual = stdout(run_in(directory, &format!("{open};{focus};{keys}"), ""));
                let moves = "down;".repeat(selection);
                let expected = stdout(run_in(directory, &format!("{open};{moves}{focus}"), ""));
                for field in ["selected", "diff.offset", "explorer.offset", "group"] {
                    let prefix = format!("{mode}.{field}=");
                    assert_eq!(
                        actual.lines().find(|line| line.starts_with(&prefix)),
                        expected.lines().find(|line| line.starts_with(&prefix)),
                        "{open};{focus};{keys}: {field}"
                    );
                }
                assert!(actual.contains(&format!("{mode}.focus=diff\n")));
                assert!(actual.contains(&format!("{mode}.diff.fullscreen={fullscreen}\n")));
            }
        }
        let actual = stdout(run_in(
            directory,
            &format!("{open};enter;x"),
            "bind x next-file\n",
        ));
        let expected = stdout(run_in(directory, &format!("{open};down;enter"), ""));
        for field in ["selected", "diff.offset"] {
            let prefix = format!("{mode}.{field}=");
            assert_eq!(
                actual.lines().find(|line| line.starts_with(&prefix)),
                expected.lines().find(|line| line.starts_with(&prefix)),
            );
        }
        assert_eq!(
            stdout(run_in(directory, &format!("{open};}};{{"), "")),
            stdout(run_in(directory, open, "")),
        );
    }
    fs::remove_dir_all(show).unwrap();
    fs::remove_dir_all(status).unwrap();
}

#[test]
fn show_diff_scrolling_updates_the_file_explorer_selection() {
    let output = stdout(run(
        "d;enter;page-down;page-down;page-down;page-down;page-down;page-down;page-down;page-down;page-down;page-down",
        "",
    ));
    assert!(output.contains("screen=show\n"));
    assert!(output.contains("show.focus=diff\n"));
    assert!(output.contains("show.selected=2\n"));
    assert!(output.contains("show.explorer.offset=2\n"));
    assert!(output.contains("selected=0\n"));
}

#[test]
fn show_jump_reports_missing_headers_without_filtering_diff() {
    let output = stdout(run(
        "d;down;enter",
        "set show git show --format='CUSTOM %H' --no-patch\n",
    ));
    assert!(output.contains("screen=show\n"));
    assert!(output.contains("show.focus=diff\n"));
    assert!(output.contains("status=Patch location unavailable for"));
    assert!(output.contains("CUSTOM"));
}

#[test]
fn show_load_failure_keeps_log_recoverable_and_identifies_stage() {
    let output = stdout(run("d", "set show-commit git show --not-a-real-option\n"));
    assert!(output.contains("screen=log\n"));
    assert!(output.contains("status=Could not load show: Git show metadata failed:"));
    assert!(output.contains("selected=0\n"));
}

#[test]
fn show_diff_failure_is_reported_without_entering_partial_show_mode() {
    let output = stdout(run("d", "set show git show --not-a-real-option\n"));
    assert!(output.contains("screen=log\n"));
    assert!(output.contains("status=Could not load show: Git show diff failed:"));
}

#[test]
fn status_lists_both_groups_and_keeps_the_complete_active_diff() {
    let directory = status_fixture();
    let output = stdout(run_in(&directory, "s", ""));

    assert!(output.contains("screen=status\n"));
    assert!(output.contains("status.focus=explorer\n"));
    assert!(output.contains("status.group=unstaged\n"));
    assert!(output.contains("MM partial.txt\n"));
    assert!(output.contains("M  staged.txt\n"));
    assert!(output.contains("R  rename source.txt -> renamed target.txt\n"));
    assert!(output.contains("?? untracked file.txt\n"));
    assert!(output.matches("MM partial.txt\n").count() >= 2);
    assert!(output.contains("diff --git a/partial.txt b/partial.txt"));
    assert!(!output.contains("diff --git a/staged.txt b/staged.txt"));

    let focused = stdout(run_in(&directory, "s;enter;down", ""));
    assert!(focused.contains("status.focus=diff\n"));
    assert!(focused.contains("status.selected=0\n"));
    assert!(focused.contains("status.diff.offset=1\n"));

    let untracked = stdout(run_in(&directory, "s;down;enter", ""));
    assert!(untracked.contains("status.focus=diff\n"));
    assert!(untracked.contains("status=\n"));
    assert!(untracked.contains("diff --git a/untracked file.txt b/untracked file.txt"));
    assert!(untracked.contains("+untracked"));
}

#[test]
fn status_uses_the_staged_diff_and_syncs_selection_across_multiple_patches() {
    let directory = status_fixture();
    let focused = stdout(run_in(&directory, "s;tab;down;enter", ""));
    assert!(focused.contains("status.focus=diff\n"));
    assert!(focused.contains("status.group=staged\n"));
    assert!(focused.contains("status.selected=1\n"));
    assert!(focused.contains("M  staged.txt\n"));
    assert!(focused.contains("diff --git a/staged.txt b/staged.txt"));

    let searched = stdout(run_in(&directory, "s;tab;enter;/;s;t;a;g;e;d;enter", ""));
    assert!(searched.contains("status.group=staged\n"));
    assert!(searched.contains("status.selected=2\n"));
    assert!(searched.contains("status.staged.selected=2\n"));
    assert!(searched.contains("status=Match for `staged`\n"));
}

#[test]
fn focused_status_diff_expands_fullscreen_and_back_returns_to_active_group() {
    for back in ["esc", "q"] {
        let directory = status_fixture();
        let output = stdout(run_in(&directory, "s;tab;enter;enter", ""));
        assert!(output.contains("status.focus=diff\n"));
        assert!(output.contains("status.group=staged\n"));
        assert!(output.contains("status.diff.fullscreen=true\n"));

        let output = stdout(run_in(&directory, &format!("s;tab;enter;enter;{back}"), ""));
        assert!(output.contains("screen=status\n"));
        assert!(output.contains("status.focus=explorer\n"));
        assert!(output.contains("status.group=staged\n"));
        assert!(output.contains("status.diff.fullscreen=false\n"));
    }
}

#[test]
fn status_closes_back_to_the_same_log_selection() {
    let output = stdout(run("down;s;s", ""));
    assert!(output.contains("screen=log\n"));
    assert!(output.contains("selected=1\n"));
    assert!(output.contains("commit=aaed0069a29bd77ca37a12f4477b41eb3fa9572f\n"));
}

#[test]
fn status_close_preserves_log_preview_state() {
    let output = stdout(run_preview("s;s", ""));
    assert!(output.contains("screen=log\n"));
    assert!(output.contains("preview.visible=true\n"));
    assert!(output.contains("preview.focused=false\n"));
}

#[test]
fn status_renders_empty_groups_in_a_clean_repository() {
    let directory = show_fixture();
    let output = stdout(run_in(&directory, "s", ""));
    assert!(output.contains("screen=status\n"));
    assert!(output.contains("status.selected=\n"));
    assert!(output.contains("status.staged.selected=\n"));
    assert!(output.contains("status.unstaged.selected=\n"));
    assert!(output.contains("status.staged:\n"));
    assert!(output.contains("status.unstaged:\n"));
    assert!(output.contains("status.diff:\n"));
}

#[test]
fn status_diff_search_keeps_the_active_file_selected() {
    let directory = status_fixture();
    let output = stdout(run_in(&directory, "s;enter;/;w;o;r;k;t;r;e;e;enter", ""));
    assert!(output.contains("status.focus=diff\n"));
    assert!(output.contains("status.selected=0\n"));
    assert!(output.contains("status=Match for `worktree`\n"));
    assert!(output.contains("+worktree"));
}

#[test]
fn status_reads_and_mutates_from_the_repository_root_when_started_in_a_subdirectory() {
    let directory = status_fixture();
    let subdirectory = directory.join("nested");
    fs::create_dir_all(&subdirectory).unwrap();

    let output = stdout(run_in(&subdirectory, "s;u", ""));
    assert!(output.contains("M  partial.txt\n"));
    assert!(
        git_output(&directory, &["diff", "--cached", "--name-only"])
            .split(|byte| *byte == b'\n')
            .any(|name| name == b"partial.txt")
    );
}

#[test]
fn status_diff_is_configurable_but_discovery_remains_fixed() {
    let directory = status_fixture();
    let output = stdout(run_in(&directory, "s", "set status-diff git diff --stat\n"));
    assert!(output.contains("status.unstaged:\n"));
    assert!(output.contains("file changed") || output.contains("files changed"));

    let missing_header = stdout(run_in(
        &directory,
        "s;enter",
        "set status-diff git diff --no-patch\n",
    ));
    assert!(missing_header.contains("status.focus=diff\n"));
    assert!(missing_header.contains("status=Patch location unavailable for"));
    assert!(missing_header.contains("MM partial.txt\n"));
}

#[test]
fn status_reverts_unstaged_changes_from_index_and_staged_changes_from_head() {
    let directory = status_fixture();
    let path = directory.join("partial.txt");
    let output = stdout(run_in(&directory, "s;enter;R", ""));
    assert!(output.contains("status.focus=diff\n"));
    assert_eq!(fs::read(&path).unwrap(), b"index\n");
    assert_eq!(
        git_output(&directory, &["show", ":partial.txt"]),
        b"index\n"
    );

    let output = stdout(run_in(&directory, "s;tab;R", ""));
    assert!(!output.contains("M  partial.txt\n"));
    assert_eq!(fs::read(&path).unwrap(), b"base\n");
    assert_eq!(git_output(&directory, &["show", ":partial.txt"]), b"base\n");
    assert_eq!(fs::read(directory.join("staged.txt")).unwrap(), b"staged\n");
}

#[test]
fn revert_is_status_only_and_can_be_rebound() {
    let directory = status_fixture();
    let path = directory.join("partial.txt");
    stdout(run_in(&directory, "R;d;R", ""));
    assert_eq!(fs::read(&path).unwrap(), b"index\nworktree\n");
    stdout(run_in(&directory, "s;x", "bind x status-revert\n"));
    assert_eq!(fs::read(&path).unwrap(), b"index\n");
}

#[test]
fn status_reverts_renames_and_removes_untracked_files() {
    let directory = status_fixture();
    stdout(run_in(&directory, "s;tab;down;R", ""));
    assert_eq!(
        fs::read(directory.join("rename source.txt")).unwrap(),
        b"rename\n"
    );
    assert!(!directory.join("renamed target.txt").exists());
    stdout(run_in(&directory, "s;down;R", ""));
    assert!(!directory.join("untracked file.txt").exists());
}

#[test]
fn status_reverts_added_files_with_and_without_head() {
    for committed in [false, true] {
        let directory = status_fixture();
        if !committed {
            git(&directory, &["checkout", "--orphan", "unborn"]);
            git(&directory, &["rm", "--cached", "-rf", "."]);
        }
        fs::write(directory.join("added.txt"), "new\n").unwrap();
        git(&directory, &["add", "--", "added.txt"]);
        stdout(run_in(&directory, "s;tab;R", ""));
        assert!(!directory.join("added.txt").exists());
    }
}

#[test]
fn status_stages_and_unstages_whole_files_without_changing_worktree_bytes() {
    let directory = status_fixture();
    let path = directory.join("partial.txt");
    let before = fs::read(&path).unwrap();

    let staged = stdout(run_in(&directory, "s;u", ""));
    assert!(staged.contains("status.staged:\n"));
    assert!(staged.contains("M  partial.txt\n"));
    assert!(!staged.contains("MM partial.txt\n"));
    assert!(
        git_output(&directory, &["diff", "--cached", "--name-only"])
            .split(|byte| *byte == b'\n')
            .any(|name| name == b"partial.txt")
    );
    assert!(
        git_output(&directory, &["diff", "--name-only"])
            .split(|byte| *byte == b'\n')
            .all(|name| name != b"partial.txt")
    );
    assert!(staged.contains("M  staged.txt\n"));
    assert!(staged.contains("R  rename source.txt -> renamed target.txt\n"));
    assert!(staged.contains("?? untracked file.txt\n"));
    assert_eq!(fs::read(&path).unwrap(), before);

    let unstaged = stdout(run_in(&directory, "s;tab;u", ""));
    assert!(unstaged.contains(" M partial.txt\n"));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(
        git_output(&directory, &["diff", "--cached", "--name-only"])
            .split(|byte| *byte == b'\n')
            .all(|name| name != b"partial.txt")
    );
    assert!(unstaged.contains(" M partial.txt\n"));
    assert!(unstaged.contains("M  staged.txt\n"));
    assert!(unstaged.contains("R  rename source.txt -> renamed target.txt\n"));
    assert!(unstaged.contains("?? untracked file.txt\n"));
}

#[test]
fn status_mutations_work_from_diff_focus() {
    let directory = status_fixture();
    let path = directory.join("partial.txt");
    let output = stdout(run_in(&directory, "s;enter;u", ""));
    assert!(output.contains("status.focus=diff\n"));
    assert!(
        git_output(&directory, &["diff", "--cached", "--name-only"])
            .split(|byte| *byte == b'\n')
            .any(|name| name == b"partial.txt")
    );
    assert_eq!(fs::read(&path).unwrap(), b"index\nworktree\n");

    let directory = status_fixture();
    let output = stdout(run_in(&directory, "s;tab;enter;u", ""));
    assert!(output.contains("status.focus=diff\n"));
    assert!(
        git_output(&directory, &["diff", "--cached", "--name-only"])
            .split(|byte| *byte == b'\n')
            .all(|name| name != b"partial.txt")
    );
}

#[test]
fn status_mutates_deleted_files_without_touching_worktree_paths() {
    let directory = deletion_status_fixture();
    let output = stdout(run_in(&directory, "s;u", ""));
    assert!(output.contains("status.staged:\n"));
    assert!(
        git_output(&directory, &["diff", "--cached", "--name-status"])
            .windows(b"D\tdeleted.txt".len())
            .any(|row| row == b"D\tdeleted.txt")
    );
    assert!(!directory.join("deleted.txt").exists());

    let output = stdout(run_in(&directory, "s;tab;u", ""));
    assert!(output.contains("status.unstaged:\n"));
    assert!(
        git_output(&directory, &["diff", "--cached", "--name-only"])
            .split(|byte| *byte == b'\n')
            .all(|name| name != b"deleted.txt")
    );
    assert!(!directory.join("deleted.txt").exists());
}

#[test]
fn status_unstages_renames_using_both_raw_endpoints() {
    let directory = status_fixture();
    let output = stdout(run_in(&directory, "s;tab;down;u", ""));
    assert!(output.contains("status.group=staged\n"));
    assert!(!output.contains("R  rename source.txt -> renamed target.txt\n"));

    let cached = git_output(&directory, &["diff", "--cached", "--name-only"]);
    assert!(
        !cached
            .split(|byte| *byte == b'\n')
            .any(|path| path == b"rename source.txt" || path == b"renamed target.txt")
    );
    assert!(!directory.join("rename source.txt").exists());
    assert_eq!(
        fs::read(directory.join("renamed target.txt")).unwrap(),
        b"rename\n"
    );
}

#[test]
fn status_handles_unborn_repositories_and_preserves_untracked_contents() {
    let directory = unborn_fixture();
    let path = directory.join("worktree.txt");
    let before = fs::read(&path).unwrap();

    let output = stdout(run_in(&directory, "s", ""));
    assert!(output.contains("screen=status\n"));
    assert!(output.contains("loaded=0\n"));
    assert!(output.contains("?? worktree.txt\n"));

    let focused = stdout(run_in(&directory, "s;enter", ""));
    assert!(focused.contains("status.focus=diff\n"));
    assert!(focused.contains("diff --git a/worktree.txt b/worktree.txt"));
    assert!(focused.contains("+worktree"));

    let staged = stdout(run_in(&directory, "s;u", ""));
    assert!(staged.contains("A  worktree.txt\n"));
    assert_eq!(fs::read(&path).unwrap(), before);

    fs::write(&path, "changed after staging\n").unwrap();

    let unstaged = stdout(run_in(&directory, "s;tab;u", ""));
    assert!(unstaged.contains("?? worktree.txt\n"));
    assert_eq!(fs::read(&path).unwrap(), b"changed after staging\n");
}

#[cfg(unix)]
#[test]
fn status_mutations_use_exact_special_paths() {
    let (directory, name) = special_status_fixture();
    let before = fs::read(directory.join(&name)).unwrap();

    let focused = stdout(run_in(&directory, "s;enter", ""));
    assert!(focused.contains("status.focus=diff\n"));
    assert!(focused.contains("status=\n"));
    assert!(focused.contains("+special"));

    let output = stdout(run_in(&directory, "s;u", ""));
    assert!(output.contains("status.staged:\n"));
    assert_eq!(fs::read(directory.join(&name)).unwrap(), before);

    let tracked = git_output(&directory, &["ls-files", "-z"]);
    assert!(
        tracked
            .split(|byte| *byte == 0)
            .any(|path| path == name.as_encoded_bytes())
    );

    let output = stdout(run_in(&directory, "s;tab;u", ""));
    assert!(output.contains("status.unstaged:\n"));
    assert_eq!(fs::read(directory.join(&name)).unwrap(), before);
    let tracked = git_output(&directory, &["ls-files", "-z"]);
    assert!(
        !tracked
            .split(|byte| *byte == 0)
            .any(|path| path == name.as_encoded_bytes())
    );
}

// Two separated chunks per file in each independently loaded diff document.
fn chunk_fixture() -> PathBuf {
    let directory = status_fixture();
    git(&directory, &["add", "."]);
    git(&directory, &["commit", "-qm", "existing files"]);
    for version in 0..4 {
        for file in ["a.txt", "b.txt"] {
            let text = (1..=40)
                .map(|line| {
                    if line == 2 || line == 30 {
                        format!("version {version} line {line} {}\n", "x".repeat(120))
                    } else {
                        format!("line {line}\n")
                    }
                })
                .collect::<String>();
            fs::write(directory.join(file), text).unwrap();
        }
        if version < 3 {
            git(&directory, &["add", "."]);
        }
        if version < 2 {
            git(&directory, &["commit", "-qm", "chunks"]);
        }
    }
    directory
}

fn snapshot_offset(output: &str, mode: &str) -> usize {
    output
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{mode}.diff.offset=")))
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn chunk_navigation_in_show_and_both_status_groups() {
    let directory = chunk_fixture();
    for (open, mode) in [("d", "show"), ("s", "status"), ("s;tab", "status")] {
        let initial = stdout(run_in(&directory, &format!("{open};enter"), ""));
        let diff = initial.split_once(&format!("{mode}.diff:\n")).unwrap().1;
        let headers = diff
            .lines()
            .enumerate()
            .filter_map(|(row, line)| line.starts_with("  @@ ").then_some(row))
            .collect::<Vec<_>>();
        assert_eq!(headers.len(), 4, "{initial}");
        for fullscreen in [false, true] {
            let focus = if fullscreen {
                "enter;enter;right"
            } else {
                "enter;right"
            };
            for (keys, target, selected) in [
                ("shift-up", snapshot_offset(&initial, mode), 0),
                ("shift-down", headers[0], 0),
                ("shift-down;down;shift-up", headers[0], 0),
                ("shift-down;shift-down;shift-up", headers[0], 0),
                ("shift-down;shift-down", headers[1], 0),
                ("shift-down;shift-down;shift-down", headers[2], 1),
                ("shift-down;shift-down;shift-down;shift-up", headers[1], 0),
                (
                    "shift-down;shift-down;shift-down;shift-down;shift-down",
                    headers[3],
                    1,
                ),
            ] {
                let output = stdout(run_in(&directory, &format!("{open};{focus};{keys}"), ""));
                assert_eq!(
                    snapshot_offset(&output, mode),
                    target,
                    "{open};{keys}: {output}"
                );
                assert!(
                    output.contains(&format!("{mode}.selected={selected}\n")),
                    "{output}"
                );
                assert!(output.contains(&format!("{mode}.focus=diff\n")));
                assert!(output.contains(&format!("{mode}.diff.fullscreen={fullscreen}\n")));
                assert!(
                    output.contains(&format!("{mode}.diff.horizontal-offset=40\n")),
                    "{output}"
                );
            }
        }
        let custom = stdout(run_in(
            &directory,
            &format!("{open};enter;x;x;y"),
            "bind x next-chunk\nbind y previous-chunk\n",
        ));
        assert_eq!(snapshot_offset(&custom, mode), headers[0]);
        assert_eq!(
            stdout(run_in(
                &directory,
                &format!("{open};shift-down;shift-up"),
                ""
            )),
            stdout(run_in(&directory, open, ""))
        );
        let omitted = "set diff-filter = sed '/@@/d'\n";
        assert_eq!(
            stdout(run_in(
                &directory,
                &format!("{open};enter;shift-down;shift-up"),
                omitted
            )),
            stdout(run_in(&directory, &format!("{open};enter"), omitted))
        );
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn refreshed_status_rebuilds_chunk_targets() {
    let directory = chunk_fixture();
    // Staging the first file removes its chunks from the unstaged document.
    let output = stdout(run_in(
        &directory,
        "s;u;enter;shift-down;shift-down;shift-down",
        "",
    ));
    let diff = output.split_once("status.diff:\n").unwrap().1;
    let headers = diff
        .lines()
        .enumerate()
        .filter_map(|(row, line)| line.starts_with("  @@ ").then_some(row))
        .collect::<Vec<_>>();
    assert_eq!(headers.len(), 2);
    assert_eq!(snapshot_offset(&output, "status"), headers[1]);
    assert!(output.contains("status.group=unstaged\n"));
    assert!(output.contains("status.selected=0\n"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn chunk_actions_leave_other_panes_and_empty_documents_unchanged() {
    for open in ["", "enter", "?"] {
        let keys = if open.is_empty() {
            "shift-down;shift-up".to_owned()
        } else {
            format!("{open};shift-down;shift-up")
        };
        let visible = |output: String| {
            output
                .lines()
                .filter(|line| !line.contains("setting.config="))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            visible(stdout(run(&keys, ""))),
            visible(stdout(run(open, "")))
        );
    }
    let directory = chunk_fixture();
    git(&directory, &["add", "."]);
    git(&directory, &["commit", "-qm", "worktree"]);
    git(&directory, &["commit", "--allow-empty", "-qm", "empty"]);
    for open in ["d;enter", "s;enter", "s;tab;enter"] {
        assert_eq!(
            stdout(run_in(
                &directory,
                &format!("{open};shift-down;shift-up"),
                ""
            )),
            stdout(run_in(&directory, open, ""))
        );
    }
    fs::write(directory.join("binary.bin"), b"old\0binary").unwrap();
    git(&directory, &["add", "."]);
    git(&directory, &["commit", "-qm", "binary"]);
    fs::write(directory.join("binary.bin"), b"new\0binary").unwrap();
    git(&directory, &["add", "."]);
    git(&directory, &["commit", "-qm", "binary change"]);
    assert_eq!(
        stdout(run_in(&directory, "d;enter;shift-down;shift-up", "")),
        stdout(run_in(&directory, "d;enter", ""))
    );
    fs::remove_dir_all(directory).unwrap();
}
