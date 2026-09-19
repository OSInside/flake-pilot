//
// Copyright (c) 2026 Marcus Schäfer
//
// This file is part of flake-pilot
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
//
//! Access to the flake registration files
//!
//! A flake application is registered with one yaml file per
//! application, optionally extended by drop-in files:
//!
//! FLAKES_DIR/
//!    ├── program_name.d
//!    │   └── other.yaml
//!    └── program_name.yaml
//!
//! The registration exists either system wide or below the
//! flakes directory of the calling user. This module provides
//! the lookup of those files and turns them into the engine
//! specific configuration model of the caller.
//!
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::get_flakes_dir;

pub fn program_abs_path() -> String {
    /*!
    Lookup absolute program path on the filesystem from
    the argv binary name of the caller
    !*/
    let program = env::args().next().expect("Arg 0 must be present");
    which::which(program).expect("Symlink should exist")
        .to_string_lossy().to_string()
}

pub fn basename(program_path: &str) -> String {
    /*!
    Get basename from given program path
    !*/
    Path::new(program_path).file_name().unwrap().to_string_lossy().to_string()
}

pub fn program_name() -> String {
    /*!
    Name the calling program is registered as

    The registration is named after the application on the host,
    which is the symlink to the pilot the caller was started from
    !*/
    basename(&program_abs_path())
}

pub fn config_file(program: &str, usermode: bool) -> String {
    /*!
    Provide the path of the registration of the given program
    !*/
    format!("{}/{}.yaml", get_flakes_dir(usermode), program)
}

pub fn config_dir(program: &str, usermode: bool) -> String {
    /*!
    Provide the path of the drop-in directory of the given program
    !*/
    format!("{}/{}.d", get_flakes_dir(usermode), program)
}

pub fn is_usermode(program: &str) -> bool {
    /*!
    Check if the given program is registered for the calling user

    The registration of the user is only used if there is no
    system wide registration of the same application. Which one
    is in use also decides which flakes setup, and with it which
    meta data directory, belongs to the flake
    !*/
    ! Path::new(&config_file(program, false)).exists()
}

pub fn config_files(usermode: bool) -> Vec<PathBuf> {
    /*!
    Provide the registration files of the given flakes setup

    The files are provided in alpha sort order. A flakes
    directory which cannot be read provides none
    !*/
    let mut config_files: Vec<PathBuf> = fs::read_dir(get_flakes_dir(usermode))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
        .collect();
    config_files.sort();
    config_files
}

pub fn merge_config(config_file: &str, config_dir: &str) -> String {
    /*!
    Read the given registration together with its drop-in files

    The files below config_dir are read in alpha sort order and
    attached to the master config_file. The result is one yaml
    document ready to be send to the yaml parser. Files which
    cannot be read are skipped
    !*/
    let base_yaml = fs::read_to_string(config_file);
    let mut extra_yamls: Vec<_> = fs::read_dir(config_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    extra_yamls.sort();
    base_yaml.into_iter().chain(
        extra_yamls.into_iter().flat_map(fs::read_to_string)
    ).collect()
}

pub fn read_config(program: &str, usermode: bool) -> Option<String> {
    /*!
    Read the registration of the given program as one yaml document

    None is returned if the program is not registered in the
    addressed flakes setup
    !*/
    let base_file = config_file(program, usermode);
    if ! Path::new(&base_file).exists() {
        return None
    }
    Some(merge_config(&base_file, &config_dir(program, usermode)))
}

pub fn config_from_str<T>(input: &str, origin: &str) -> T
where
    T: Deserialize<'static>
{
    /*!
    Deserialize the given yaml document into the configuration
    model of the caller

    Duplicate keys are resolved first, this allows the drop-in
    files of a registration to overwrite the settings of the
    registration they extend. The origin is the source of the
    document and is only used to report on errors
    !*/
    // Parse into a generic YAML to remove duplicate keys
    let yaml_documents = yaml_rust::YamlLoader::load_from_str(input)
        .unwrap_or_else(
            |error| panic!("Failed to parse yaml input at: {origin}: {error}")
        );
    let yaml = yaml_documents.first().unwrap_or_else(
        || panic!("No configuration data provided in: {origin}")
    );
    let mut buffer = String::new();
    yaml_rust::YamlEmitter::new(&mut buffer).dump(yaml).unwrap();

    // Convert to a String and leak it to make it static
    // Can not use serde_yaml::from_value because of lifetime limitations
    // Safety: This does not cause a reocurring memory leak
    // since the configuration is only loaded once
    let content = Box::leak(buffer.into_boxed_str());

    serde_yaml::from_str(content).unwrap_or_else(
        |error| panic!("Failed to import {origin}: {error}")
    )
}

pub fn load_config<T>(program: &str, usermode: bool) -> T
where
    T: Deserialize<'static>
{
    /*!
    Read the registration of the given program into the
    configuration model of the caller
    !*/
    let yaml = read_config(program, usermode).unwrap_or_else(
        || panic!("No user/system wide flake registration found for: {program}")
    );
    config_from_str(&yaml, &config_file(program, usermode))
}

#[cfg(test)]
mod tests {
    use super::{basename, config_dir, config_file, program_abs_path};

    #[test]
    fn test_program_abs_path() {
        let program_path = program_abs_path();
        assert!(program_path.starts_with('/'));
    }

    #[test]
    fn test_basename() {
        assert_eq!("name", basename("/some/name"));
    }

    #[test]
    fn test_config_file() {
        assert_eq!("/usr/share/flakes/app.yaml", config_file("app", false));
    }

    #[test]
    fn test_config_dir() {
        assert_eq!("/usr/share/flakes/app.d", config_dir("app", false));
    }
}
