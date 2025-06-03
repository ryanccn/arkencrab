// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
// SPDX-FileCopyrightText: 2025 Seth Flynn <getchoo@tuta.io>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

#[derive(clap::Parser, Debug, Clone)]
pub struct Cli {
    /// The Firefox profile directory to operate on; defaults to first installation's default profile in profiles.ini
    #[clap(short, long, global = true, env = "ARKENCRAB_PROFILE")]
    pub profile: Option<PathBuf>,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum Command {
    /// Update the arkenfox user.js
    Update {
        /// Show a diff of the changes
        #[clap(short, long, env = "ARKENCRAB_DIFF")]
        diff: bool,

        /// Don't add overrides from user-overrides.js
        #[clap(short, long, env = "ARKENCRAB_UPDATE_NO_OVERRIDES")]
        no_overrides: bool,

        /// Enable preferences for Firefox ESR
        #[clap(long, env = "ARKENCRAB_ESR")]
        esr: bool,
    },

    /// Clean redundant preferences in prefs.js
    PrefsClean {
        /// Show a diff of the changes (will be large)
        #[clap(short, long, env = "ARKENCRAB_DIFF")]
        diff: bool,
    },

    /// Edit the arkenfox user-overrides.js with an editor
    Edit {
        /// Don't apply the new overrides after the editor is closed
        #[clap(short, long, env = "ARKENCRAB_EDIT_NO_APPLY")]
        no_apply: bool,

        /// The editor to open user-overrides.js with
        #[clap(short, long, env = "EDITOR")]
        editor: Option<String>,
    },

    /// Print the profile being used
    Profile {},

    /// Generate shell completions
    Completions {
        /// The shell to generate completions for
        shell: clap_complete::Shell,
    },
}
