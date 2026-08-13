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
use std::path::Path;

use uzers::get_current_uid;

use crate::flakelog::FlakeLog;
use crate::error::FlakeError;
use crate::user::{User, mkdir};
use crate::command::CommandExtTrait;

#[derive(Debug, Default, Clone, Copy)]
pub struct IO {
}

impl IO {
    pub fn private_dir(base: &str, user: User) -> Result<String, FlakeError> {
        /*!
        Provide a private directory for the calling user below base

        Meta data like container/VM ID files or communication sockets
        must not be writable by other users. As the base directory is
        shared between all users of the system it is created with the
        sticky bit set, like /tmp. The per user directory below it is
        created with 0700 permissions and is required to be owned by
        the calling user.
        !*/
        if ! Path::new(base).exists() {
            mkdir(base, "1777", user)?;
        }
        let base_attributes = fs::symlink_metadata(base)?;
        if ! base_attributes.is_dir() {
            return Err(FlakeError::IOError {
                kind: "Insecure meta data directory".to_string(),
                message: format!("{base} is not a directory")
            })
        }
        let base_mode = base_attributes.permissions().mode();
        if base_mode & 0o002 != 0 && base_mode & 0o1000 == 0 {
            // A directory which everybody can write to allows to
            // delete and replace the meta data of other users unless
            // the sticky bit restricts this to the file owner
            return Err(FlakeError::IOError {
                kind: "Insecure meta data directory".to_string(),
                message: format!(
                    "{base} is world writable without the sticky bit set. \
                    Please fix this with: chmod 1777 {base}"
                )
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
            let mut call = user.run("tar");
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
        let mut call = user.run("rsync");
        call.arg("-av");
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
