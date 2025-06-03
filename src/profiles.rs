// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
// SPDX-FileCopyrightText: 2025 Seth Flynn <getchoo@tuta.io>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    convert::AsRef,
    env, io,
    path::{Path, PathBuf},
};

use eyre::{OptionExt, Result, bail};
use ini::Ini;

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
    Ini::load_from_file(profiles_ini)?
        .into_iter()
        .find_map(|(section_name, properties)| {
            section_name
                .is_some_and(|s| s.starts_with("Install"))
                .then(|| properties.get("Default").map(|v| v.to_string()))
        })
        .flatten()
        .ok_or_eyre("unable to obtain default profile from profiles.ini")
}

pub fn default_profile() -> Result<PathBuf> {
    #[cfg(unix)]
    let home = home_dir()?;
    #[cfg(windows)]
    let roaming_appdata = roaming_appdata()?;

    let firefox_data_paths = [
        #[cfg(all(unix, not(target_os = "macos")))]
        home.join(".mozilla").join("firefox"),
        // Snap
        #[cfg(target_os = "linux")]
        home.join("snap")
            .join("firefox")
            .join("common")
            .join(".mozilla")
            .join("firefox"),
        // Flatpak
        #[cfg(target_os = "linux")]
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

        match default_profile_path_in(&profiles_ini) {
            Ok(default_profile_path) => return Ok(path.join(default_profile_path)),
            Err(err)
                if err
                    .downcast_ref::<ini::Error>()
                    .is_some_and(|err| match err {
                        ini::Error::Io(err) => err.kind() == io::ErrorKind::NotFound,
                        ini::Error::Parse(_) => false,
                    }) => {}
            Err(err) => return Err(err),
        }
    }

    bail!("could not find default profile")
}

#[cfg(test)]
mod tests {
    use eyre::Result;
    use std::path::Path;

    #[test]
    fn can_find_default_profile_path() -> Result<()> {
        let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let profiles_ini = root_dir.join("src/profiles.test.ini");

        let result = super::default_profile_path_in(&profiles_ini)?;
        assert_eq!(result, "Profiles/arkenfox");

        Ok(())
    }
}
