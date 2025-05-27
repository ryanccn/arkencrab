// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
// SPDX-FileCopyrightText: 2025 Seth Flynn <getchoo@tuta.io>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    borrow::Cow,
    collections::HashSet,
    convert::AsRef,
    env, fs, io,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use anstream::{print, println};
use clap::{CommandFactory as _, Parser};
use eyre::{OptionExt, Result, bail};
use ini::Ini;
use owo_colors::OwoColorize as _;
use regex::{Regex, RegexBuilder};

static USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

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

fn default_profile_path_in<T: AsRef<Path>>(profiles_ini: T) -> Result<String> {
    let ini = Ini::load_from_file(profiles_ini)?;

    ini.iter()
        .find_map(|(maybe_section_name, properties)| {
            let section_name = maybe_section_name?;

            if section_name.starts_with("Install") {
                properties.get("Default").map(ToString::to_string)
            } else {
                None
            }
        })
        .ok_or_eyre("unable to obtain default profile from profiles.ini")
}

fn default_profile() -> Result<PathBuf> {
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
        let profiles_ini = path.join("profiles.ini");

        if profiles_ini.exists() {
            let default_profile_path = default_profile_path_in(&profiles_ini)?;
            return Ok(path.join(default_profile_path));
        }
    }

    bail!("could not find default profile")
}

fn resolve_profile(cli: &Cli) -> Result<Cow<Path>> {
    let profile = if let Some(p) = &cli.profile {
        Cow::Borrowed(p.as_path())
    } else {
        let profile = default_profile()?;
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

            let backup = profile
                .join("userjs_backups")
                .join(format!("user.js.backup.{}", now()));

            fs::create_dir_all(profile.join("userjs_backups"))?;
            fs::write(&backup, &existing_user)?;

            println!(
                "{} user.js to {}",
                "backed up".magenta(),
                backup
                    .strip_prefix(profile.as_ref())
                    .unwrap_or(backup.as_path())
                    .display()
            );

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

            let backup = profile
                .join("prefsjs_backups")
                .join(format!("prefs.js.backup.{}", now()));

            fs::create_dir_all(profile.join("prefsjs_backups"))?;
            fs::write(&backup, &existing_prefs)?;

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

            fs::write(profile.join("prefs.js"), &new_prefs)?;
            println!("{} {} redundant prefs", "removed".red(), discarded_prefs);
        }

        Command::Completions { shell } => {
            clap_complete::generate(*shell, &mut Cli::command(), "arkencrab", &mut io::stdout());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use eyre::Result;

    #[test]
    fn can_find_default_profile_path() -> Result<()> {
        let root_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let profiles_ini = root_dir.join("src/profiles.ini");

        let result = super::default_profile_path_in(&profiles_ini)?;
        let expected = String::from("Profiles/arkenfox");

        assert_eq!(result, expected);
        Ok(())
    }
}
