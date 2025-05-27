// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
// SPDX-FileCopyrightText: 2025 Seth Flynn <getchoo@tuta.io>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    borrow::Cow,
    collections::HashSet,
    env, io,
    path::{Path, PathBuf},
    sync::LazyLock,
};
use tokio::fs;

use anstream::{print, println};
use clap::{CommandFactory as _, Parser};
use eyre::{OptionExt, Result, bail};
use owo_colors::OwoColorize as _;
use regex::{Regex, RegexBuilder};

static HTTP: LazyLock<reqwest::Client> =
    LazyLock::new(|| reqwest::Client::builder().https_only(true).build().unwrap());

static USER_JS_URL: &str =
    "https://raw.githubusercontent.com/arkenfox/user.js/refs/heads/master/user.js";

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

// `env::home_dir` stabilized in latest Rust but not in Nixpkgs Rust, so we implement
// a knockoff version ourselves.
#[cfg(unix)]
fn home_dir() -> Result<PathBuf> {
    Ok(PathBuf::from(
        env::var_os("HOME").ok_or_eyre("could not obtain home directory")?,
    ))
}

#[cfg(windows)]
fn roaming_appdata() -> Result<PathBuf> {
    let appdata = env::var_os("APPDATA").ok_or_eyre("could not obtain APPDATA directory")?;
    Ok(PathBuf::from(appdata))
}

async fn default_profile() -> Result<PathBuf> {
    #[cfg(unix)]
    let home = home_dir()?;
    #[cfg(windows)]
    let roaming_appdata = roaming_appdata()?;

    let firefox_data_paths = [
        #[cfg(all(unix, not(target_os = "macos")))]
        home.join(".mozilla").join("firefox"),
        #[cfg(all(unix, not(target_os = "macos")))]
        home.join("snap")
            .join("firefox")
            .join("common")
            .join(".mozilla")
            .join("firefox"),
        #[cfg(all(unix, not(target_os = "macos")))]
        home.join(".var")
            .join("app")
            .join("org.mozilla.firefox")
            .join(".mozilla")
            .join("firefox"),
        #[cfg(target_os = "macos")]
        home.join("Library")
            .join("Application Support")
            .join("Firefox"),
        #[cfg(windows)]
        roaming_appdata.join("Mozilla").join("Firefox"),
    ];

    for path in &firefox_data_paths {
        if let Ok(ini) = fs::read_to_string(path.join("profiles.ini")).await {
            if let Some(default_profile) = ini
                .lines()
                .find(|l| l.starts_with("Default=Profiles/"))
                .and_then(|l| l.strip_prefix("Default="))
                .map(|p| path.join(p))
            {
                return Ok(default_profile);
            }
        }
    }

    bail!("could not find default profile")
}

async fn resolve_profile(cli: &Cli) -> Result<Cow<Path>> {
    let profile = if let Some(p) = &cli.profile {
        Cow::Borrowed(p.as_path())
    } else {
        let profile = default_profile().await?;
        Cow::Owned(profile)
    };

    println!("{} {}", "using profile".blue(), profile.display());
    Ok(profile)
}

async fn read_string_with_default(path: impl AsRef<Path>) -> Result<String> {
    match fs::read_to_string(path).await {
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

#[derive(clap::Parser, Debug, Clone)]
struct Cli {
    /// The Firefox profile directory to operate on (defaults to first installation's default profile in profiles.ini)
    #[clap(short, long, global = true)]
    profile: Option<PathBuf>,

    #[clap(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Command {
    /// Update the arkenfox user.js
    Update {
        /// Show a diff of the changes
        #[clap(short, long)]
        diff: bool,

        /// Don't add overrides from user-overrides.js
        #[clap(short, long)]
        no_overrides: bool,

        /// Enable preferences for Firefox ESR
        #[clap(long)]
        esr: bool,
    },

    /// Clean redundant preferences in prefs.js
    PrefsClean {
        /// Show a diff of the changes (will be large)
        #[clap(short, long)]
        diff: bool,
    },

    /// Generate shell completions
    Completions {
        /// The shell to generate completions for
        shell: clap_complete::Shell,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    match &cli.command {
        Command::Update {
            diff,
            no_overrides,
            esr,
        } => {
            let profile = resolve_profile(&cli).await?;

            let existing_user = read_string_with_default(profile.join("user.js")).await?;
            let existing_version = find_version(&existing_user);

            let backup = profile
                .join("userjs_backups")
                .join(format!("user.js.backup.{}", now()));

            fs::create_dir_all(profile.join("userjs_backups")).await?;
            fs::write(&backup, &existing_user).await?;

            println!(
                "{} user.js to {}",
                "backed up".magenta(),
                backup
                    .strip_prefix(profile.as_ref())
                    .unwrap_or(backup.as_path())
                    .display()
            );

            let mut new_user = HTTP
                .get(USER_JS_URL)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;

            let this_version = find_version(&new_user);

            if *esr {
                new_user = new_user.replace("/* ESR", "// ESR");
            }

            if !no_overrides {
                let overrides = read_string_with_default(profile.join("user-overrides.js")).await?;
                new_user += "\n";
                new_user += &overrides;
            }

            fs::write(profile.join("user.js"), &new_user).await?;

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
            let profile = resolve_profile(&cli).await?;

            let user = read_string_with_default(profile.join("user.js")).await?;
            let existing_prefs = read_string_with_default(profile.join("prefs.js")).await?;

            let backup = profile
                .join("prefsjs_backups")
                .join(format!("prefs.js.backup.{}", now()));

            fs::create_dir_all(profile.join("prefsjs_backups")).await?;
            fs::write(&backup, &existing_prefs).await?;

            println!(
                "{} prefs.js to {}",
                "backed up".magenta(),
                backup
                    .strip_prefix(profile.as_ref())
                    .unwrap_or(backup.as_path())
                    .display()
            );

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

            fs::write(profile.join("prefs.js"), &new_prefs).await?;
            println!("{} {} redundant prefs", "removed".red(), discarded_prefs);
        }

        Command::Completions { shell } => {
            clap_complete::generate(*shell, &mut Cli::command(), "arkencrab", &mut io::stdout());
        }
    }

    Ok(())
}
