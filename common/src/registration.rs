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
//! A system wide registration is additionally extended by the
//! drop-in files of the local system configuration, which are
//! read last and therefore provide the final settings:
//!
//! /etc/flakes/
//!    └── program_name.d
//!        └── local.yaml
//!
//! The registration exists either system wide or below the
//! flakes directory of the calling user. This module provides
//! the lookup of those files and turns them into the engine
//! specific configuration model of the caller.
//!
use std::env;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_saphyr::options::DuplicateKeyPolicy;

use crate::config::get_flakes_dir;
use crate::defaults;

// Upper limit for the size of a single yaml file of a registration
const MAX_CONFIG_SIZE: u64 = 1024 * 1024;

pub fn program_abs_path() -> String {
    /*!
    Lookup absolute program path on the filesystem from
    the argv binary name of the caller
    !*/
    let program = env::args().next().unwrap_or_default();
    // A program which cannot be found is provided as it was called,
    // its registration is not found then
    which::which(&program)
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or(program)
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

pub fn config_dirs(program: &str, usermode: bool) -> Vec<String> {
    /*!
    Provide the drop-in directories of the given program in
    the order they are read

    A system wide registration is finally extended by the
    drop-in directory below /etc/flakes. This allows local
    adaptions of a registration which itself is provided as
    part of the system. Registrations of the calling user
    are not affected by it
    !*/
    let mut config_dirs = vec![config_dir(program, usermode)];
    if ! usermode {
        let local_config_dir = format!(
            "{}/{}.d", defaults::FLAKES_CONFIG_DIR, program
        );
        // flakes_dir could be configured to point to /etc/flakes
        if ! config_dirs.contains(&local_config_dir) {
            config_dirs.push(local_config_dir);
        }
    }
    config_dirs
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

pub fn read_config_file(path: impl AsRef<Path>) -> io::Result<String> {
    /*!
    Read the given yaml file of a registration

    Only a regular file of at most MAX_CONFIG_SIZE bytes is read.
    The device gets opened in O_NONBLOCK such that e.g. an open
    on a FIFO does not block the reader forever. After opening
    the handle is checked to be a real file such that e.g reading
    from /dev/zero won't read until the memory is exhausted.
    !*/
    // O_NONBLOCK: opening a FIFO must not block, harmless for regular files
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    // Checked on the opened file, the path could be replaced meanwhile
    if ! file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput, "Not a regular file"
        ))
    }
    let mut content = String::new();
    file.take(MAX_CONFIG_SIZE + 1).read_to_string(&mut content)?;
    if content.len() as u64 > MAX_CONFIG_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("File exceeds the limit of {MAX_CONFIG_SIZE} bytes")
        ))
    }
    Ok(content)
}

pub fn merge_config(config_file: &str, config_dirs: &[String]) -> String {
    /*!
    Read the given registration together with its drop-in files

    The config_dirs are processed in the given order. The files
    below each of them are read in alpha sort order and attached
    to the master config_file. The result is one yaml document
    ready to be send to the yaml parser. Files which cannot be
    read are skipped
    !*/
    let base_yaml = read_config_file(config_file);
    let extra_yamls = config_dirs.iter().flat_map(|config_dir| {
        let mut dir_yamls: Vec<_> = fs::read_dir(config_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        dir_yamls.sort();
        dir_yamls
    });
    base_yaml.into_iter().chain(
        extra_yamls.flat_map(read_config_file)
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
    Some(merge_config(&base_file, &config_dirs(program, usermode)))
}

pub fn config_from_str<T>(input: &str, origin: &str) -> T
where
    T: Deserialize<'static>
{
    /*!
    Deserialize the given yaml document into the configuration
    model of the caller, see try_config_from_str(). An invalid
    document is a fatal error
    !*/
    try_config_from_str(input, origin).unwrap_or_else(|error| panic!("{error}"))
}

pub fn try_config_from_str<T>(input: &str, origin: &str) -> Result<T, String>
where
    T: Deserialize<'static>
{
    /*!
    Deserialize the given yaml document into the configuration
    model of the caller

    Of a key which is given more than once the last one wins,
    this allows the drop-in files of a registration to overwrite
    the settings of the registration they extend. The value of
    the key is replaced as a whole, it is not merged with the
    former one. The origin is the source of the document and is
    only used to report on errors.

    The input has to be exactly one yaml document. Empty documents,
    e.g of only comments, do not count. More than one document is
    refused, the drop-in files of a registration are part of the
    one document, see merge_config()
    !*/
    // The configuration model of the pilots borrows its strings
    // from the document for the lifetime of the program. The
    // document is therefore leaked to make it static. This does
    // not cause a recurring memory leak since the configuration
    // is only loaded once
    let content: &'static str = Box::leak(input.to_owned().into_boxed_str());
    let options = serde_saphyr::options! {
        duplicate_keys: DuplicateKeyPolicy::LastWins,
    };
    let mut documents: Vec<T> = serde_saphyr::from_multiple_with_options(
        content, options
    ).map_err(
        |error| format!("Failed to import {origin}: {error}")
    )?;
    match documents.len() {
        0 => Err(format!("No configuration data provided in: {origin}")),
        1 => Ok(documents.remove(0)),
        count => Err(format!(
            "Failed to import {origin}: expected one yaml document, \
            found {count}. A '---' line starts another document"
        ))
    }
}

pub fn load_config<T>(program: &str, usermode: bool) -> Result<T, String>
where
    T: Deserialize<'static>
{
    /*!
    Read the registration of the given program into the
    configuration model of the caller

    A pilot binary called directly, not through the link of a
    registered application, has no registration. It is told
    apart from an application which is not registered (anymore)
    to point the caller to the way a pilot is used
    !*/
    let yaml = match read_config(program, usermode) {
        Some(yaml) => yaml,
        None if is_direct_call() => return Err(format!(
            "{program} is the launcher of flake applications and is not \
            meant to be called directly. Call an application registered \
            through flake-ctl instead"
        )),
        None => return Err(format!(
            "No user/system wide flake registration found for: {program}"
        ))
    };
    try_config_from_str(&yaml, &config_file(program, usermode))
}

fn is_direct_call() -> bool {
    /*!
    Check if the calling program was started directly and not
    through a symbolic link, which is how a registered flake
    application calls its pilot
    !*/
    fs::symlink_metadata(program_abs_path())
        .map(|metadata| ! metadata.file_type().is_symlink())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;

    use tempfile::tempdir;

    use super::{
        basename, config_dir, config_dirs, config_file, merge_config,
        program_abs_path, read_config_file, try_config_from_str,
        MAX_CONFIG_SIZE
    };
    use std::collections::HashMap;

    #[test]
    fn test_program_abs_path() {
        let program_path = program_abs_path();
        assert!(program_path.starts_with('/'));
    }

    #[test]
    fn test_try_config_from_str() {
        let config: Result<HashMap<String, u32>, String> =
            try_config_from_str("app: 1\n", "app.yaml");
        assert_eq!(Some(&1), config.unwrap().get("app"));
        // invalid documents are reported, not a fatal error
        for input in [
            "app: [1", "", " \n", "# no data\n", "app: text", "app: 1\n---\napp: 2\n"
        ] {
            let config: Result<HashMap<String, u32>, String> =
                try_config_from_str(input, "app.yaml");
            assert!(config.unwrap_err().contains("app.yaml"), "{input:?}");
        }
    }

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct TestSection<'a> {
        name: &'a str,
        #[serde(default)]
        runtime: Option<HashMap<String, u32>>
    }

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct TestRegistration<'a> {
        #[serde(borrow)]
        sandbox: TestSection<'a>
    }

    #[test]
    fn test_try_config_from_str_drop_in_overrides() {
        // a drop-in file attached to the registration repeats a
        // key, the last one wins and replaces the value as a whole
        let config: TestRegistration = try_config_from_str(
            "sandbox:\n  name: base\n  runtime:\n    a: 1\n    b: 2\n\
            sandbox:\n  name: drop-in\n  runtime:\n    b: 3\n",
            "app.yaml"
        ).unwrap();
        assert_eq!("drop-in", config.sandbox.name);
        assert_eq!(
            Some(HashMap::from([("b".to_string(), 3)])), config.sandbox.runtime
        );
        // a repeated key within a section is resolved the same way
        let config: TestRegistration = try_config_from_str(
            "sandbox:\n  name: base\n  name: other\n", "app.yaml"
        ).unwrap();
        assert_eq!("other", config.sandbox.name);
        assert_eq!(None, config.sandbox.runtime);
    }

    #[test]
    fn test_try_config_from_str_merged_drop_ins() {
        // the drop-in files of a registration overwrite its settings
        // in the order they are merged
        let tmp_dir = tempdir().unwrap();
        let base_file = tmp_dir.path().join("app.yaml");
        let drop_in_dir = tmp_dir.path().join("app.d");
        fs::create_dir(&drop_in_dir).unwrap();
        fs::write(&base_file, "sandbox:\n  name: base\n").unwrap();
        fs::write(drop_in_dir.join("a.yaml"), "sandbox:\n  name: a\n").unwrap();
        fs::write(drop_in_dir.join("b.yaml"), "sandbox:\n  name: b\n").unwrap();
        let merged = merge_config(
            base_file.to_str().unwrap(),
            &[drop_in_dir.to_str().unwrap().to_string()]
        );
        let config: TestRegistration = try_config_from_str(
            &merged, "app.yaml"
        ).unwrap();
        assert_eq!("b", config.sandbox.name);
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

    #[test]
    fn test_config_dirs_system() {
        assert_eq!(
            vec!["/usr/share/flakes/app.d", "/etc/flakes/app.d"],
            config_dirs("app", false)
        );
    }

    #[test]
    fn test_merge_config_dir_order() {
        // The files of a later directory are read after all files
        // of the directories before it, regardless of their names
        let root = std::env::temp_dir().join(
            format!("flakes-merge-config-{}", std::process::id())
        );
        let flakes_dir = root.join("app.d");
        let local_dir = root.join("etc").join("app.d");
        fs::create_dir_all(&flakes_dir).unwrap();
        fs::create_dir_all(&local_dir).unwrap();
        let base_file = root.join("app.yaml");
        fs::write(&base_file, "base: 1\n").unwrap();
        fs::write(flakes_dir.join("b.yaml"), "flakes_b: 1\n").unwrap();
        fs::write(flakes_dir.join("c.yaml"), "flakes_c: 1\n").unwrap();
        fs::write(local_dir.join("a.yaml"), "local_a: 1\n").unwrap();
        let merged = merge_config(
            &base_file.to_string_lossy(),
            &[
                flakes_dir.to_string_lossy().to_string(),
                local_dir.to_string_lossy().to_string()
            ]
        );
        fs::remove_dir_all(&root).unwrap();
        assert_eq!("base: 1\nflakes_b: 1\nflakes_c: 1\nlocal_a: 1\n", merged);
    }

    #[test]
    fn test_read_config_file() {
        let workdir = tempdir().unwrap();
        let config = workdir.path().join("app.yaml");
        fs::write(&config, "app: 1\n").unwrap();
        assert_eq!("app: 1\n", read_config_file(&config).unwrap());
        // a link to a regular file is fine
        let link = workdir.path().join("link.yaml");
        symlink(&config, &link).unwrap();
        assert_eq!("app: 1\n", read_config_file(&link).unwrap());
        // a file of exactly the limit is fine, one byte more is not
        let large = workdir.path().join("large.yaml");
        fs::write(&large, vec![b'#'; MAX_CONFIG_SIZE as usize]).unwrap();
        assert!(read_config_file(&large).is_ok());
        fs::write(&large, vec![b'#'; MAX_CONFIG_SIZE as usize + 1]).unwrap();
        assert!(read_config_file(&large).is_err());
    }

    #[test]
    fn test_read_config_file_no_regular_file() {
        // Neither is read: /dev/zero is endless and a
        // FIFO blocks until there is a writer
        let workdir = tempdir().unwrap();
        let zero = workdir.path().join("zero.yaml");
        symlink("/dev/zero", &zero).unwrap();
        let fifo = workdir.path().join("fifo.yaml");
        let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(0, unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) });
        for path in [&zero, &fifo, &workdir.path().to_path_buf()] {
            assert!(read_config_file(path).is_err(), "{path:?}");
        }
    }

    #[test]
    fn test_merge_config_skips_no_regular_file() {
        let workdir = tempdir().unwrap();
        let config_dir = workdir.path().join("app.d");
        fs::create_dir(&config_dir).unwrap();
        let base_file = workdir.path().join("app.yaml");
        fs::write(&base_file, "base: 1\n").unwrap();
        symlink("/dev/zero", config_dir.join("a.yaml")).unwrap();
        fs::create_dir(config_dir.join("b.yaml")).unwrap();
        fs::write(config_dir.join("c.yaml"), "c: 1\n").unwrap();
        let merged = merge_config(
            &base_file.to_string_lossy(),
            &[config_dir.to_string_lossy().to_string()]
        );
        assert_eq!("base: 1\nc: 1\n", merged);
    }
}
