//
// Copyright (c) 2023 SUSE Software Solutions Germany GmbH
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
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::fs::{
    DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt
};

use uzers::get_current_uid;

use crate::defaults;
use crate::flakelog::FlakeLog;
use crate::error::FlakeError;
use crate::user::{User, octal_mode};
use crate::command::CommandExtTrait;

#[derive(Debug, Default, Clone, Copy)]
pub struct IO {
}

impl IO {
    pub fn private_dir(base: &str) -> Result<String, FlakeError> {
        /*!
        Provide a private directory for the calling user below base

        Meta data like container/VM ID files or communication sockets
        must not be writable by other users. The base directory is
        shared between all users of the system and has to be set up
        as meta_dir_problem() expects, like /tmp. It is not created
        here: it has to belong to root, which a pilot called by a
        user cannot ensure without root privileges. It is created by
        systemd-tmpfiles at boot or by 'flake-ctl init'. A missing or
        differently set up base directory is an error.

        The per user directory below it is created with 0700
        permissions and is required to be owned by the calling user.
        !*/
        if let Some(problem) = Self::meta_dir_problem(base) {
            return Err(FlakeError::IOError {
                kind: "Invalid meta data directory".to_string(),
                message: problem
            })
        }
        let private_dir = format!("{}/{}", base, get_current_uid());
        if let Err(error) = DirBuilder::new().mode(0o700).create(&private_dir) {
            if error.kind() != ErrorKind::AlreadyExists {
                return Err(FlakeError::IO(error))
            }
        }
        let attributes = fs::symlink_metadata(&private_dir)?;
        if ! attributes.is_dir() || attributes.uid() != get_current_uid() {
            return Err(FlakeError::IOError {
                kind: "Insecure meta data directory".to_string(),
                message: format!(
                    "{private_dir} is not a directory owned by the caller"
                )
            })
        }
        if attributes.permissions().mode() & 0o077 != 0 {
            fs::set_permissions(
                &private_dir, fs::Permissions::from_mode(0o700)
            )?;
        }
        Ok(private_dir)
    }

    pub fn meta_dir_problem(base: &str) -> Option<String> {
        /*!
        Tell what is wrong with the given meta data base directory,
        None if it is set up as expected

        Every user creates a private directory in it, which requires
        it to be writable for everybody. The sticky bit restricts the
        deletion of an entry to its owner and to the owner of the
        directory. The latter therefore has to be root. A user who
        owns the directory can delete and replace the private
        directories of all other users
        !*/
        let attributes = match fs::symlink_metadata(base) {
            Ok(attributes) => attributes,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Some(format!(
                    "{base} does not exist. It is created at boot by \
                    systemd-tmpfiles, see {}, or by calling \
                    'flake-ctl init' as root",
                    defaults::META_DIR_TMPFILES_CONF
                ))
            },
            Err(error) => return Some(format!("Cannot access {base}: {error}"))
        };
        if ! attributes.is_dir() {
            return Some(format!(
                "{base} is not a directory, please remove it as root"
            ))
        }
        if attributes.uid() != 0 {
            return Some(format!(
                "{base} does not belong to root, please fix this as \
                root with: chown root:root {base}"
            ))
        }
        let mode = attributes.permissions().mode() & 0o7777;
        if octal_mode(defaults::META_DIR_MODE).ok() != Some(mode) {
            return Some(format!(
                "{base} has the permissions {mode:o} instead of {}, \
                please fix this as root with: chmod {} {base}",
                defaults::META_DIR_MODE, defaults::META_DIR_MODE
            ))
        }
        None
    }

    pub fn no_symlink(path: &str) -> Result<(), FlakeError> {
        /*!
        Make sure the given meta data path is not a symbolic link

        Meta data files are stored below directories which are also
        used by other users. Following a link placed there by someone
        else would cause reads and writes to an unexpected target
        !*/
        if let Ok(attributes) = fs::symlink_metadata(path) {
            if attributes.file_type().is_symlink() {
                return Err(FlakeError::IOError {
                    kind: "Insecure meta data file".to_string(),
                    message: format!(
                        "{path} is a symbolic link, refusing to use it"
                    )
                })
            }
        }
        Ok(())
    }

    pub fn create_meta_file(path: &str) -> Result<File, FlakeError> {
        /*!
        Create/Truncate the given meta data file

        The file is opened with O_NOFOLLOW to never write through a
        symbolic link somebody else could have placed at that path
        !*/
        Self::no_symlink(path)?;
        Ok(
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)?
        )
    }

    pub fn sync_includes(
        target: &String, tar_includes: Vec<&str>, path_includes: Vec<&str>, user: User
    ) -> Result<(), FlakeError> {
        /*!
        Sync custom include data to target path
        !*/
        for tar in tar_includes {
            FlakeLog::debug(&format!("Provision tar archive: [{tar}]"));
            let mut call = user.run(defaults::TAR_TOOL);
            call.arg("-C").arg(target)
                .arg("-xf").arg(tar);
            FlakeLog::debug(
                &format!("{:?} {:?}", call.get_program(), call.get_args())
            );
            let output = call.perform()?;
            FlakeLog::debug(
                &format!("{}", String::from_utf8_lossy(&output.stdout))
            );
            FlakeLog::debug(
                &format!("{}", String::from_utf8_lossy(&output.stderr))
            );
        }
        for path in path_includes {
            FlakeLog::debug(&format!("Provision path: [{path}]"));
            Self::sync_data(
                path, &format!("{target}/{path}"),
                ["--mkpath"].to_vec(), user
            )?;
        }
        Ok(())
    }

    pub fn sync_data(
        source: &str, target: &str, options: Vec<&str>, user: User
    ) -> Result<(), FlakeError> {
        /*!
        Sync data from source path to target path
        !*/
        let mut call = user.run(defaults::RSYNC_TOOL);
        call.args(defaults::RSYNC_OPTIONS);
        for option in options {
            call.arg(option);
        }
        call.arg(source).arg(target);
        FlakeLog::debug(
            &format!("{:?} {:?}", call.get_program(), call.get_args())
        );
        let output = call.output()?;
        FlakeLog::debug(
            &format!("{}", String::from_utf8_lossy(&output.stdout))
        );
        FlakeLog::debug(
            &format!("{}", String::from_utf8_lossy(&output.stderr))
        );
        if !output.status.success() {
            return Err(FlakeError::SyncFailed)
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::IO;
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::Path;
    use tempfile::tempdir;
    use uzers::get_current_uid;

    fn meta_dir(base: &str, mode: u32) -> String {
        let ids_dir = format!("{base}/flakes");
        fs::create_dir(&ids_dir).unwrap();
        fs::set_permissions(&ids_dir, fs::Permissions::from_mode(mode))
            .unwrap();
        ids_dir
    }

    #[test]
    fn test_private_dir_refuses_a_missing_base() {
        // The base directory is not created by the pilots
        let workdir = tempdir().unwrap();
        let base = format!("{}/flakes", workdir.path().to_str().unwrap());
        let error = IO::private_dir(&base).unwrap_err();
        assert!(format!("{error}").contains("flake-ctl init"));
        assert!(! Path::new(&base).exists());
    }

    #[test]
    fn test_private_dir_refuses_a_symlink() {
        // A link is refused even if it points to a proper directory
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let ids_dir = meta_dir(base, 0o1777);
        let link = format!("{base}/link");
        symlink(&ids_dir, &link).unwrap();
        assert!(
            IO::meta_dir_problem(&link).unwrap()
                .contains("is not a directory")
        );
        assert!(IO::private_dir(&link).is_err());
    }

    #[test]
    fn test_private_dir_owner_and_mode_of_the_base() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let ids_dir = meta_dir(base, 0o1777);
        let private_dir = format!("{ids_dir}/{}", get_current_uid());
        if get_current_uid() == 0 {
            // The base belongs to root, only the mode matters
            assert_eq!(private_dir, IO::private_dir(&ids_dir).unwrap());
            fs::set_permissions(&ids_dir, fs::Permissions::from_mode(0o777))
                .unwrap();
            assert!(
                IO::meta_dir_problem(&ids_dir).unwrap().contains("chmod")
            );
        } else {
            // A base which belongs to the caller is refused, whatever
            // its mode, and nothing is created in it
            assert!(
                IO::meta_dir_problem(&ids_dir).unwrap()
                    .contains("chown root")
            );
            assert!(IO::private_dir(&ids_dir).is_err());
            assert!(! Path::new(&private_dir).exists());
        }
    }
}
