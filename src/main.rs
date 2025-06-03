// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
// SPDX-FileCopyrightText: 2025 Seth Flynn <getchoo@tuta.io>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    borrow::Cow, collections::HashSet, env, fs, io, path::Path, process::Command as StdCommand,
    sync::LazyLock,
};

use anstream::{print, println};
use clap::{CommandFactory as _, Parser};
use eyre::{Result, bail, eyre};
use owo_colors::OwoColorize as _;
use regex::{Regex, RegexBuilder};

use crate::cli::{Cli, Command};

mod cli;
mod profiles;

static USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

static USER_JS_URL: &str =
    "https://raw.githubusercontent.com/arkenfox/user.js/refs/heads/master/user.js";

#[cfg(unix)]
static DEFAULT_EDITOR: &str = "nano";
#[cfg(windows)]
static DEFAULT_EDITOR: &str = "notepad.exe";

static ARKENCRAB_START_MARKER: &str = "/** START: arkencrab overrides */";

static REGEX_VERSION: LazyLock<Regex> = LazyLock::new(|| {
    RegexBuilder::new(r"^\*\s*version:\s*(\d+)")
        .multi_line(true)
        .build()
        .unwrap()
});

static REGEX_USER_PREF: LazyLock<Regex> = LazyLock::new(|| {
    RegexBuilder::new(r#"^\s*user_pref\((".*?"),"#)
        .multi_line(true)
        .build()
        .unwrap()
});

fn resolve_profile(cli: &Cli) -> Result<Cow<Path>> {
    let profile = if let Some(p) = &cli.profile {
        Cow::Borrowed(p.as_path())
    } else {
        let profile = profiles::default_profile()?;
        Cow::Owned(profile)
    };

    println!("{} {}", "using profile".blue(), profile.display());
    Ok(profile)
}

fn read_string_with_default(path: impl AsRef<Path>) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err.into()),
    }
}

fn find_version(user_js: &str) -> String {
    REGEX_VERSION
        .captures(user_js)
        .map_or("unknown", |c| c.extract::<1>().1[0])
        .to_owned()
}

fn print_diff(old: &str, new: &str) {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(old, new);

    print!(
        "{}",
        diff.unified_diff()
            .context_radius(2)
            .iter_hunks()
            .map(|hunk| hunk
                .iter_changes()
                .map(|change| {
                    let plain = format!("{}\t{}", change.tag(), change);
                    match change.tag() {
                        ChangeTag::Equal => plain.to_string(),
                        ChangeTag::Insert => plain.green().to_string(),
                        ChangeTag::Delete => plain.red().to_string(),
                    }
                })
                .collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d-%H-%M-%S").to_string()
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    match &cli.command {
        Command::Update {
            diff,
            no_overrides,
            esr,
        } => {
            let profile = resolve_profile(&cli)?;

            let existing_user = read_string_with_default(profile.join("user.js"))?;
            let existing_version = find_version(&existing_user);

            let backup = Path::new("userjs_backups").join(format!("user.js.backup.{}", now()));

            fs::create_dir_all(profile.join("userjs_backups"))?;
            fs::write(profile.join(&backup), &existing_user)?;

            println!("{} user.js to {}", "backed up".magenta(), backup.display());

            let http = reqwest::blocking::Client::builder()
                .https_only(true)
                .user_agent(USER_AGENT)
                .build()?;

            let mut new_user = http.get(USER_JS_URL).send()?.error_for_status()?.text()?;

            let this_version = find_version(&new_user);

            if *esr {
                new_user = new_user.replace("/* ESR", "// ESR");
            }

            if !no_overrides {
                let overrides = read_string_with_default(profile.join("user-overrides.js"))?;
                new_user += "\n";
                new_user += ARKENCRAB_START_MARKER;
                new_user += "\n\n";
                new_user += &overrides;
            }

            fs::write(profile.join("user.js"), &new_user)?;

            if *diff {
                print_diff(&existing_user, &new_user);
            }

            println!(
                "{} arkenfox v{} {} v{}{}",
                "updated".green(),
                if existing_version == this_version {
                    existing_version.to_string()
                } else {
                    existing_version.yellow().to_string()
                },
                "->".dimmed(),
                this_version.green(),
                if existing_version == this_version {
                    if existing_user == new_user {
                        " (unchanged)".dimmed().to_string()
                    } else {
                        " (changed)".yellow().to_string()
                    }
                } else {
                    String::new()
                }
            );
        }

        Command::PrefsClean { diff } => {
            let profile = resolve_profile(&cli)?;

            let user = read_string_with_default(profile.join("user.js"))?;
            let existing_prefs = read_string_with_default(profile.join("prefs.js"))?;

            let backup = Path::new("prefsjs_backups").join(format!("prefs.js.backup.{}", now()));

            fs::create_dir_all(profile.join("prefsjs_backups"))?;
            fs::write(profile.join(&backup), &existing_prefs)?;

            println!("{} prefs.js to {}", "backed up".magenta(), backup.display());

            let user_pref_keys = REGEX_USER_PREF
                .captures_iter(&user)
                .map(|c| c.extract::<1>().1[0])
                .collect::<HashSet<_>>();

            let (discarded_prefs, new_prefs): (Vec<_>, Vec<_>) = existing_prefs
                .lines()
                .partition(|l| user_pref_keys.iter().any(|k| l.contains(k)));

            let discarded_prefs = discarded_prefs.len();
            let new_prefs = new_prefs.join("\n") + "\n";

            if *diff {
                print_diff(&existing_prefs, &new_prefs);
            }

            fs::write(profile.join("prefs.js"), &new_prefs)?;
            println!("{} {} redundant prefs", "removed".red(), discarded_prefs);
        }

        Command::Edit { editor, no_apply } => {
            let profile = resolve_profile(&cli)?;

            let mut editor = editor
                .as_ref()
                .and_then(|s| shlex::split(s))
                .unwrap_or_else(|| vec![DEFAULT_EDITOR.to_owned()]);

            let program = editor
                .pop()
                .ok_or_else(|| eyre!("invalid editor provided"))?;

            let status = StdCommand::new(&program)
                .args(&editor)
                .arg(profile.join("user-overrides.js"))
                .status()?;

            if !status.success() {
                bail!("editor failed with status code {:?}", status.code())
            }

            let existing_user = read_string_with_default(profile.join("user.js"))?;
            let existing_version = find_version(&existing_user);

            let mut new_user = existing_user
                .lines()
                .take_while(|l| l.trim() != ARKENCRAB_START_MARKER)
                .collect::<Vec<_>>()
                .join("\n");

            if !no_apply {
                if new_user.trim() == existing_user.trim() {
                    println!(
                        "{} automatically update user.js with new overrides; run {}",
                        "could not".yellow(),
                        "`arkencrab update`".cyan()
                    );
                } else {
                    let backup =
                        Path::new("userjs_backups").join(format!("user.js.backup.{}", now()));

                    fs::create_dir_all(profile.join("userjs_backups"))?;
                    fs::write(profile.join(&backup), &existing_user)?;

                    println!("{} user.js to {}", "backed up".magenta(), backup.display());

                    let overrides = read_string_with_default(profile.join("user-overrides.js"))?;
                    new_user += "\n";
                    new_user += ARKENCRAB_START_MARKER;
                    new_user += "\n\n";
                    new_user += &overrides;

                    fs::write(profile.join("user.js"), &new_user)?;

                    println!(
                        "{} arkenfox v{} with new overrides",
                        "updated".green(),
                        existing_version.green(),
                    );
                }
            }
        }

        Command::Completions { shell } => {
            clap_complete::generate(*shell, &mut Cli::command(), "arkencrab", &mut io::stdout());
        }
    }

    Ok(())
}
