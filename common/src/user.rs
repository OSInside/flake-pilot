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
use std::fs;
use std::path::Path;
use std::{process::Command, ffi::OsStr};
use serde::{Serialize, Deserialize};
use crate::command::CommandExtTrait;
use uzers::{get_current_uid, get_current_username, get_current_groupname};
use crate::lookup::{Lookup};
use crate::error::FlakeError;
use crate::openat;

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

    pub fn get_user_id(&self) -> String {
        get_current_uid().to_string()
    }

    pub fn get_group_name(&self) -> String {
        get_current_groupname().unwrap().into_string().unwrap()
    }

    pub fn is_calling_user(&self) -> bool {
        /*!
        Check if this user is the one running this process

        Work for another user has to be handed to sudo, but work
        for the caller can be done by the process itself. This
        allows to do it in a way which cannot be tricked into
        following a symbolic link
        !*/
        match self.name {
            // Without a name sudo is called without the --user
            // option, which runs the command as root
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

    pub fn run<S: AsRef<OsStr>>(&self, command: S) -> Command {
        /*!
        Call command via sudo

        The environment of the caller is not passed along. Handing
        the environment of an unprivileged caller to a program
        running as root allows to influence that program in ways
        the sudo rule for it never intended
        !*/
        self.run_with_env(command, &[])
    }

    pub fn run_with_env<S: AsRef<OsStr>>(
        &self, command: S, keep_env: &[&str]
    ) -> Command {
        /*!
        Call command via sudo, preserving the given environment
        variables. Only variables the called program really needs
        should be listed here
        !*/
        let mut c = Command::new("sudo");
        if ! keep_env.is_empty() {
            c.arg(format!("--preserve-env={}", keep_env.join(",")));
        }
        if let Some(name) = self.name {
            c.arg("--user").arg(name);
        }
        c.arg(command);
        c
    }
}

pub fn exists(filename: &str, user: User) -> Result<bool, FlakeError> {
    /*!
    check file exists via sudo
    !*/
    let mut call = user.run("test");
    call.arg("-e").arg(filename);
    if Lookup::is_debug() {
        debug!("{:?} {:?}", call.get_program(), call.get_args());
    }
    let output = match call.output() {
        Ok(output) => {
            output
        }
        Err(error) => {
            return Err(
                FlakeError::IOError {
                    kind: "call failed".to_string(),
                    message: format!("{error:?}")
                }
            );
        }
    };
    if output.status.success() {
        return Ok(true)
    }
    Ok(false)
}

pub fn cp(source: &str, target: &str, user: User) -> Result<(), FlakeError> {
    /*!
    Copy source to target

    Reading through a symbolic link as root provides the contents
    of a file the caller is not allowed to read. Writing through
    one overwrites a file of the caller's choice. Therefore no
    link is followed, see mkdir() for the details
    !*/
    if user.is_calling_user() {
        return openat::copy(source, target).map_err(
            |error| FlakeError::IOError {
                kind: "Copy failed".to_string(),
                message: format!(
                    "Failed to copy {source} to {target}: {error}"
                )
            }
        )
    }
    no_symlink_in_path(source)?;
    no_symlink_in_path(target)?;
    let mut call = user.run("cp");
    call.arg(source).arg(target);
    if Lookup::is_debug() {
        debug!("{:?} {:?}", call.get_program(), call.get_args());
    }
    call.perform()?;
    Ok(())
}

pub fn chmod(filename: &str, mode: &str, user: User) -> Result<(), FlakeError> {
    /*!
    Set the permissions of filename

    A chmod as root through a symbolic link changes the
    permissions of the file the link points to, which is a file
    the caller picked. Therefore no link is followed, see mkdir()
    for the details
    !*/
    if user.is_calling_user() {
        return openat::set_mode(filename, octal_mode(mode)?).map_err(
            |error| FlakeError::IOError {
                kind: "Chmod failed".to_string(),
                message: format!("Failed to set mode of {filename}: {error}")
            }
        )
    }
    no_symlink_in_path(filename)?;
    let mut call = user.run("chmod");
    call.arg(mode).arg(filename);
    if Lookup::is_debug() {
        debug!("{:?} {:?}", call.get_program(), call.get_args());
    }
    call.perform()?;
    Ok(())
}

pub fn mkdir(dirname: &str, mode: &str, user: User) -> Result<(), FlakeError> {
    /*!
    Make directory

    A directory created with the privileges of another user,
    usually root, must not be created through a symbolic link. A
    link placed in the path by the caller would let the privileged
    call create a directory, and set its permissions, at a
    location that caller picked, e.g a system directory

    If the directory belongs to the user running this process, it
    is created by the process itself. Every component of the path
    is resolved with openat2(RESOLVE_NO_SYMLINKS) and the mode is
    set on the descriptor of the new directory. Thus there is not
    only no link followed, there is also no moment in which the
    path could be replaced by one

    For another user the work has to be done by sudo, which
    resolves the path on its own. In this case the path is checked
    to be free of links before and after the call. This closes the
    known attack but not the window between check and call
    !*/
    if user.is_calling_user() {
        return openat::create_dir_all(dirname, octal_mode(mode)?).map_err(
            |error| FlakeError::IOError {
                kind: "Mkdir failed".to_string(),
                message: format!("Failed to create {dirname}: {error}")
            }
        )
    }
    no_symlink_in_path(dirname)?;
    if Path::new(dirname).exists() {
        return Ok(())
    }
    let mut mkdir_call = user.run("mkdir");
    mkdir_call.arg("-p").arg("-m").arg(mode).arg(dirname);
    if Lookup::is_debug() {
        debug!("{:?} {:?}", mkdir_call.get_program(), mkdir_call.get_args());
    }
    mkdir_call.perform()?;
    // The mode is applied again because 'mkdir -p' only sets it on
    // the last component of the path and not on a directory which
    // exists already. As chmod resolves a link, the path is
    // checked again. The directory could have been replaced by a
    // link in the time between its creation and this call
    no_symlink_in_path(dirname)?;
    let mut chmod_call = user.run("chmod");
    chmod_call.arg(mode).arg(dirname);
    if Lookup::is_debug() {
        debug!("{:?} {:?}", chmod_call.get_program(), chmod_call.get_args());
    }
    chmod_call.perform()?;
    Ok(())
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

pub fn no_symlink_in_path(filename: &str) -> Result<(), FlakeError> {
    /*!
    Make sure no component of the given path is a symbolic link

    A component which does not exist is no link and a component
    which cannot be looked up is left to the privileged call. Only
    a component which is proven to be a link is rejected
    !*/
    for component in Path::new(filename).ancestors() {
        let attributes = match fs::symlink_metadata(component) {
            Ok(attributes) => attributes,
            Err(_) => continue
        };
        if attributes.file_type().is_symlink() {
            return Err(FlakeError::IOError {
                kind: "Insecure path".to_string(),
                message: format!(
                    "{} is a symbolic link, refusing to use {} through it",
                    component.display(), filename
                )
            })
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{User, chmod, cp, mkdir, no_symlink_in_path, octal_mode};
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::Path;
    use tempfile::tempdir;
    use uzers::get_current_username;

    fn calling_user() -> String {
        // Work done for this user is done by the process itself
        // and not handed over to sudo
        get_current_username().unwrap().into_string().unwrap()
    }

    #[test]
    fn test_path_without_symlink_is_accepted() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        fs::create_dir(format!("{base}/dir")).unwrap();
        assert!(no_symlink_in_path(&format!("{base}/dir")).is_ok());
        // a component which does not exist yet is created by the
        // call and is therefore no link
        assert!(no_symlink_in_path(&format!("{base}/dir/new/sub")).is_ok());
    }

    #[test]
    fn test_symlink_as_last_component_is_rejected() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        fs::create_dir(format!("{base}/target")).unwrap();
        symlink(format!("{base}/target"), format!("{base}/link")).unwrap();
        assert!(no_symlink_in_path(&format!("{base}/link")).is_err());
    }

    #[test]
    fn test_symlink_in_path_is_rejected() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        fs::create_dir(format!("{base}/target")).unwrap();
        symlink(format!("{base}/target"), format!("{base}/link")).unwrap();
        assert!(no_symlink_in_path(&format!("{base}/link/sub")).is_err());
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
        // the call is rejected before any privileged command runs
        assert!(mkdir(&dirname, "755", User::ROOT).is_err());
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
    fn test_work_for_another_user_is_left_to_sudo() {
        assert!(! User::from("a_user_which_does_not_exist").is_calling_user());
        assert!(User::from(calling_user().as_str()).is_calling_user());
        assert_eq!(
            uzers::get_current_uid() == 0, User::ROOT.is_calling_user()
        );
    }

    #[test]
    fn test_octal_mode() {
        assert_eq!(0o755, octal_mode("755").unwrap());
        assert_eq!(0o1777, octal_mode("1777").unwrap());
        assert!(octal_mode("u+x").is_err());
    }
}

