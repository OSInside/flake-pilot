//
// Copyright (c) 2023 Elektrobit Automotive GmbH
// Copyright (c) 2023 Marcus Schäfer
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
use std::env;
use std::path::Path;
use std::{process::Command, ffi::OsStr};
use serde::{Serialize, Deserialize};
use uzers::{get_current_uid, get_current_username, get_user_by_uid};
use crate::error::FlakeError;
use crate::openat;
use crate::defaults;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct User<'a> {
    name: Option<&'a str>
}

impl<'a> From<&'a str> for User<'a> {
    fn from(value: &'a str) -> Self {
        Self { name: Some(value) }
    }
}

impl User<'_> {
    pub const ROOT: User<'static> = User { name: Some("root")};

    pub fn is_calling_user(&self) -> bool {
        /*!
        Check if this user is the one running this process

        flake-pilot changes the privileges of a process only through
        its helper. Work can therefore only be done for the user who
        runs this process, which also covers root working for a user
        through the helper. The work is done by the process itself
        in a way which cannot be tricked into following a symbolic
        link
        !*/
        match self.name {
            None | Some("root") => get_current_uid() == 0,
            Some(name) => get_current_username()
                .map(|caller| caller == *name)
                .unwrap_or(false)
        }
    }

    pub fn get_name(&self) -> String {
        let mut user = String::new();
        if let Some(name) = self.name {
            user.push_str(name)
        }
        user
    }

    pub fn run<S: AsRef<OsStr>>(&self, command: S) -> Result<Command, FlakeError> {
        /*!
        Create a call of command with the privileges of this user

        Only the user running this process can be served, see
        is_calling_user(). The command has to be given by its
        absolute path, see the tool definitions in defaults
        !*/
        debug_assert!(
            Path::new(command.as_ref()).is_absolute(),
            "{:?} is not an absolute path", command.as_ref()
        );
        self.check_calling_user()?;
        Ok(Command::new(command))
    }

    fn check_calling_user(&self) -> Result<(), FlakeError> {
        /*!
        Refuse work for another user than the one running this
        process, see is_calling_user()
        !*/
        if ! self.is_calling_user() {
            return Err(FlakeError::IOError {
                kind: "Privileges required".to_string(),
                message: format!(
                    "Cannot act as user {:?}, flake-pilot changes \
                    privileges only through its helper", self.get_name()
                )
            })
        }
        Ok(())
    }
}

pub fn get_caller_uid() -> u32 {
    /*!
    Provide the ID of the user this process works for

    This is the user running the process, except for a pilot which
    runs as root for a user who called a system wide flake. Such a
    pilot is started by the helper, which hands over the ID of that
    user in defaults::CALLER_UID_VAR. The variable is only read by
    a process running as root, a user cannot set it for one
    !*/
    let uid = get_current_uid();
    if uid != 0 {
        return uid
    }
    env::var(defaults::CALLER_UID_VAR).ok()
        .and_then(|caller| caller.parse().ok())
        .unwrap_or(uid)
}

pub fn get_caller_name() -> String {
    /*!
    Provide the login name of the user this process works for, see
    get_caller_uid()
    !*/
    get_user_by_uid(get_caller_uid())
        .map(|user| user.name().to_string_lossy().to_string())
        .unwrap_or_else(|| get_caller_uid().to_string())
}

pub fn exists(filename: &str, user: User) -> Result<bool, FlakeError> {
    /*!
    Check if the given file exists for the given user
    !*/
    user.check_calling_user()?;
    Ok(Path::new(filename).exists())
}

pub fn cp(source: &str, target: &str, user: User) -> Result<(), FlakeError> {
    /*!
    Copy source to target

    Reading through a symbolic link as root provides the contents
    of a file the caller is not allowed to read. Writing through
    one overwrites a file of the caller's choice. Therefore no
    link is followed, see mkdir() for the details
    !*/
    user.check_calling_user()?;
    openat::copy(source, target).map_err(
        |error| FlakeError::IOError {
            kind: "Copy failed".to_string(),
            message: format!("Failed to copy {source} to {target}: {error}")
        }
    )
}

pub fn chmod(filename: &str, mode: &str, user: User) -> Result<(), FlakeError> {
    /*!
    Set the permissions of filename

    A chmod as root through a symbolic link changes the
    permissions of the file the link points to, which is a file
    the caller picked. Therefore no link is followed, see mkdir()
    for the details
    !*/
    user.check_calling_user()?;
    openat::set_mode(filename, octal_mode(mode)?).map_err(
        |error| FlakeError::IOError {
            kind: "Chmod failed".to_string(),
            message: format!("Failed to set mode of {filename}: {error}")
        }
    )
}

pub fn mkdir(dirname: &str, mode: &str, user: User) -> Result<(), FlakeError> {
    /*!
    Make directory

    A directory created with the privileges of root must not be
    created through a symbolic link. A link placed in the path by
    a user would let the call create a directory, and set its
    permissions, at a location that user picked, e.g a system
    directory

    Every component of the path is resolved with
    openat2(RESOLVE_NO_SYMLINKS) and the mode is set on the
    descriptor of the new directory. Thus there is not only no link
    followed, there is also no moment in which the path could be
    replaced by one
    !*/
    user.check_calling_user()?;
    openat::create_dir_all(dirname, octal_mode(mode)?).map_err(
        |error| FlakeError::IOError {
            kind: "Mkdir failed".to_string(),
            message: format!("Failed to create {dirname}: {error}")
        }
    )
}

pub fn octal_mode(mode: &str) -> Result<u32, FlakeError> {
    /*!
    Convert a mode as it is given to chmod(1), e.g "755", into
    the number the system calls expect
    !*/
    u32::from_str_radix(mode, 8).map_err(
        |_| FlakeError::IOError {
            kind: "Invalid mode".to_string(),
            message: format!("{mode} is no octal permission mode")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::{User, chmod, cp, get_caller_uid, mkdir, octal_mode};
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use tempfile::tempdir;
    use uzers::get_current_username;

    fn calling_user() -> String {
        // Work can only be done for the user running the process
        get_current_username().unwrap().into_string().unwrap()
    }

    #[test]
    fn test_mkdir_refuses_to_create_through_a_symlink() {
        // The link and its target stay inside the temporary
        // directory. Thus a regression cannot create anything
        // outside of it
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        fs::create_dir(format!("{base}/target")).unwrap();
        symlink(format!("{base}/target"), format!("{base}/link")).unwrap();
        let dirname = format!("{base}/link/sub");
        assert!(mkdir(&dirname, "755", User::ROOT).is_err());
        let user = calling_user();
        assert!(mkdir(&dirname, "755", User::from(user.as_str())).is_err());
        assert!(! Path::new(&format!("{base}/target/sub")).exists());
    }

    #[test]
    fn test_mkdir_for_the_calling_user() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let dirname = format!("{base}/some/dir");
        let user = calling_user();
        mkdir(&dirname, "700", User::from(user.as_str())).unwrap();
        let attributes = fs::symlink_metadata(&dirname).unwrap();
        assert!(attributes.is_dir());
        assert_eq!(0o700, attributes.permissions().mode() & 0o7777);
    }

    #[test]
    fn test_chmod_of_a_socket_for_the_calling_user() {
        // firecracker-pilot restricts the vsock socket of a VM this
        // way, the socket belongs to the user who started the VM
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let socket = format!("{base}/vm.sock");
        let _listener = UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o777))
            .unwrap();
        let user = calling_user();
        chmod(&socket, "600", User::from(user.as_str())).unwrap();
        assert_eq!(
            0o600,
            fs::symlink_metadata(&socket).unwrap()
                .permissions().mode() & 0o7777
        );
    }

    #[test]
    fn test_chmod_refuses_a_symlink() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let filename = format!("{base}/file");
        fs::write(&filename, "data").unwrap();
        fs::set_permissions(&filename, fs::Permissions::from_mode(0o644))
            .unwrap();
        symlink(&filename, format!("{base}/link")).unwrap();
        assert!(chmod(&format!("{base}/link"), "600", User::ROOT).is_err());
        assert_eq!(
            0o644,
            fs::symlink_metadata(&filename).unwrap()
                .permissions().mode() & 0o7777
        );
    }

    #[test]
    fn test_cp_refuses_a_symlink() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let source = format!("{base}/source");
        let secret = format!("{base}/secret");
        fs::write(&source, "data").unwrap();
        fs::write(&secret, "secret").unwrap();
        symlink(&secret, format!("{base}/link")).unwrap();
        // reading through a link placed by the caller...
        assert!(
            cp(&format!("{base}/link"), &format!("{base}/copy"), User::ROOT)
                .is_err()
        );
        assert!(! Path::new(&format!("{base}/copy")).exists());
        // ...and writing through it are both refused
        assert!(cp(&source, &format!("{base}/link"), User::ROOT).is_err());
        assert_eq!("secret", fs::read_to_string(&secret).unwrap());
    }

    #[test]
    fn test_cp_for_the_calling_user() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let source = format!("{base}/source");
        let target = format!("{base}/target");
        fs::write(&source, "data").unwrap();
        let user = calling_user();
        cp(&source, &target, User::from(user.as_str())).unwrap();
        assert_eq!("data", fs::read_to_string(&target).unwrap());
    }

    #[test]
    fn test_work_for_another_user_is_refused() {
        let other = User::from("a_user_which_does_not_exist");
        assert!(! other.is_calling_user());
        assert!(User::from(calling_user().as_str()).is_calling_user());
        assert_eq!(
            uzers::get_current_uid() == 0, User::ROOT.is_calling_user()
        );
        // there is no way to act as another user
        let workdir = tempdir().unwrap();
        let dirname = format!("{}/dir", workdir.path().to_str().unwrap());
        assert!(mkdir(&dirname, "755", other).is_err());
        assert!(other.run("/usr/bin/true").is_err());
        assert!(! Path::new(&dirname).exists());
    }

    #[test]
    fn test_get_caller_uid() {
        // a process which does not run as root works for its user,
        // no matter what the environment says
        if uzers::get_current_uid() != 0 {
            std::env::set_var(crate::defaults::CALLER_UID_VAR, "0");
            assert_eq!(uzers::get_current_uid(), get_caller_uid());
            std::env::remove_var(crate::defaults::CALLER_UID_VAR);
        }
        assert_eq!(uzers::get_current_uid(), get_caller_uid());
    }

    #[test]
    fn test_octal_mode() {
        assert_eq!(0o755, octal_mode("755").unwrap());
        assert_eq!(0o1777, octal_mode("1777").unwrap());
        assert!(octal_mode("u+x").is_err());
    }
}

