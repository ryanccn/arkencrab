// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
// SPDX-FileCopyrightText: 2025 Seth Flynn <getchoo@tuta.io>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

#[derive(clap::Parser, Debug, Clone)]
pub struct Cli {
    /// The Firefox profile directory to operate on (defaults to first installation's default profile in profiles.ini)
    #[clap(short, long, global = true)]
    pub profile: Option<PathBuf>,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum Command {
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
