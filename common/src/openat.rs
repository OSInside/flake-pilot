//
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
//! File operations which never follow a symbolic link
//!
//! Checking a path for symbolic links and acting on it afterwards
//! is racy. Between the check and the call the path can be replaced
//! by a link and the call then works on a target somebody else has
//! chosen (CWE-59). The functions in this module do not check, they
//! resolve the path with openat2(RESOLVE_NO_SYMLINKS) and do their
//! work on the resulting file descriptor. A descriptor stays bound
//! to the file it was opened on and cannot be redirected.
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{Error, ErrorKind, Result};
use std::mem;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::{Component, Path};
use std::sync::atomic::{AtomicBool, Ordering};

use libc::c_int;

// openat2() exists since Linux 5.6. On an older kernel the same
// guarantee is provided by O_NOFOLLOW, see open_at()
static NO_OPENAT2: AtomicBool = AtomicBool::new(false);

pub fn create_dir_all(dirname: &str, mode: u32) -> Result<()> {
    /*!
    Create dirname and the missing directories above it

    The mode is set on the directory dirname refers to, the
    directories created above it get the standard mode, like
    'mkdir -p' does it. An already existing directory keeps its
    mode. No component of the path is allowed to be a symbolic
    link
    !*/
    let (parent, name) = open_parent(dirname, true)?;
    match make_dir_at(parent.as_raw_fd(), &name, mode) {
        Ok(()) => {
            // The mode given to mkdir is masked by the umask of the
            // process, therefore it has to be set again. This is
            // done on the descriptor of the new directory. A chmod
            // by name could act on a link placed there in between
            let directory = open_directory(parent.as_raw_fd(), &name)?;
            set_mode_of(&directory, mode)
        },
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            // Opening the directory proves that it really is one
            // and not a symbolic link pointing to one
            open_directory(parent.as_raw_fd(), &name)?;
            Ok(())
        },
        Err(error) => Err(error)
    }
}

pub fn set_mode(filename: &str, mode: u32) -> Result<()> {
    /*!
    Set the permissions of filename

    The file itself is changed, also if it is a symbolic link. No
    component of the path is allowed to be a symbolic link
    !*/
    let (parent, name) = open_parent(filename, false)?;
    // O_PATH opens any type of file, also a socket or a device
    // which could not be opened for reading or writing
    let target = open_at(parent.as_raw_fd(), &name, libc::O_PATH, 0)?;
    // fchmod() does not work on a descriptor opened with O_PATH.
    // The proc file of the descriptor refers to the file it is
    // bound to, thus this path cannot be redirected either. It is
    // the way glibc implements fchmodat(AT_SYMLINK_NOFOLLOW)
    let proc_path = to_c_string(
        &format!("/proc/self/fd/{}", target.as_raw_fd())
    )?;
    if unsafe { libc::chmod(proc_path.as_ptr(), mode as libc::mode_t) } != 0 {
        return Err(Error::last_os_error())
    }
    Ok(())
}

pub fn copy(source: &str, target: &str) -> Result<()> {
    /*!
    Copy the contents of source to target

    A target which does not exist yet is created with the
    permissions of the source, an existing target keeps its own
    permissions. Neither of the two paths is allowed to contain a
    symbolic link, also not as its last component
    !*/
    let (source_dir, source_name) = open_parent(source, false)?;
    let source_file = open_at(
        source_dir.as_raw_fd(), &source_name, libc::O_RDONLY, 0
    )?;
    let source_mode = mode_of(&source_file)? & 0o7777;
    let (target_dir, target_name) = open_parent(target, false)?;
    let target_file = match open_at(
        target_dir.as_raw_fd(), &target_name,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600
    ) {
        Ok(created) => {
            set_mode_of(&created, source_mode)?;
            created
        },
        Err(error) if error.kind() == ErrorKind::AlreadyExists => open_at(
            target_dir.as_raw_fd(), &target_name,
            libc::O_WRONLY | libc::O_TRUNC, 0
        )?,
        Err(error) => return Err(error)
    };
    std::io::copy(
        &mut File::from(source_file), &mut File::from(target_file)
    )?;
    Ok(())
}

fn open_parent(path: &str, create: bool) -> Result<(OwnedFd, CString)> {
    /*!
    Open the directory the last component of path lives in

    The path is walked component by component, each of them
    opened relative to the one before and none of them allowed to
    be a symbolic link. If create is set, missing directories on
    the way are created. Provides the descriptor of the directory
    and the name of the last component in it
    !*/
    let mut components = path_components(path)?;
    let name = match components.pop() {
        Some(name) => name,
        None => return Err(Error::new(
            ErrorKind::InvalidInput, format!("{path} has no name component")
        ))
    };
    let start = if Path::new(path).is_absolute() { "/" } else { "." };
    let mut directory = open_directory(
        libc::AT_FDCWD, &to_c_string(start)?
    )?;
    for component in components {
        directory = match open_directory(directory.as_raw_fd(), &component) {
            Ok(next) => next,
            Err(error) if create && error.kind() == ErrorKind::NotFound => {
                // Only the directory asked for gets the requested
                // mode, the ones above it are created the way
                // 'mkdir -p' does it
                match make_dir_at(directory.as_raw_fd(), &component, 0o755) {
                    Ok(()) => {},
                    Err(error)
                        if error.kind() == ErrorKind::AlreadyExists => {},
                    Err(error) => return Err(error)
                }
                open_directory(directory.as_raw_fd(), &component)?
            },
            Err(error) => return Err(error)
        }
    }
    Ok((directory, name))
}

fn path_components(path: &str) -> Result<Vec<CString>> {
    /*!
    Split path into its name components

    A '..' component is rejected. Resolving it would mean to walk
    back to a directory the process has passed already, which
    cannot be done without trusting the path
    !*/
    let mut components = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => components.push(
                CString::new(name.as_bytes()).map_err(
                    |_| Error::new(
                        ErrorKind::InvalidInput,
                        format!("{path} contains a null byte")
                    )
                )?
            ),
            _ => return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("{path} contains a '..' component")
            ))
        }
    }
    Ok(components)
}

fn open_directory(dirfd: RawFd, name: &CStr) -> Result<OwnedFd> {
    /*!
    Open the directory name below the directory dirfd refers to
    !*/
    open_at(dirfd, name, libc::O_DIRECTORY | libc::O_RDONLY, 0)
}

fn open_at(
    dirfd: RawFd, name: &CStr, flags: c_int, mode: u32
) -> Result<OwnedFd> {
    /*!
    Open name below the directory dirfd refers to

    The name must be a single path component. It is not followed
    if it is a symbolic link, the call fails in this case
    !*/
    if ! NO_OPENAT2.load(Ordering::Relaxed) {
        // The struct is not constructed as a literal because libc
        // marks it as non exhaustive, the kernel may add fields
        let mut how: libc::open_how = unsafe { mem::zeroed() };
        how.flags = (flags | libc::O_CLOEXEC) as u64;
        if flags & libc::O_CREAT != 0 {
            how.mode = mode as u64;
        }
        how.resolve = libc::RESOLVE_NO_SYMLINKS;
        let descriptor = unsafe {
            libc::syscall(
                libc::SYS_openat2, dirfd, name.as_ptr(),
                &how, mem::size_of::<libc::open_how>()
            )
        };
        if descriptor >= 0 {
            return Ok(unsafe { OwnedFd::from_raw_fd(descriptor as RawFd) })
        }
        let error = Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOSYS) {
            return Err(error)
        }
        NO_OPENAT2.store(true, Ordering::Relaxed);
    }
    // The kernel is too old for openat2(). As name is a single
    // component below an already resolved directory, O_NOFOLLOW
    // provides the same guarantee
    let descriptor = unsafe {
        libc::openat(
            dirfd, name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW, mode as libc::c_uint
        )
    };
    if descriptor < 0 {
        return Err(Error::last_os_error())
    }
    let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
    if flags & libc::O_PATH != 0
        && mode_of(&descriptor)? & libc::S_IFMT == libc::S_IFLNK
    {
        // O_NOFOLLOW does not reject a link if it is combined with
        // O_PATH, it opens the link itself
        return Err(Error::from_raw_os_error(libc::ELOOP))
    }
    Ok(descriptor)
}

fn make_dir_at(dirfd: RawFd, name: &CStr, mode: u32) -> Result<()> {
    /*!
    Create the directory name below the directory dirfd refers to
    !*/
    if unsafe {
        libc::mkdirat(dirfd, name.as_ptr(), mode as libc::mode_t)
    } != 0 {
        return Err(Error::last_os_error())
    }
    Ok(())
}

fn set_mode_of(descriptor: &OwnedFd, mode: u32) -> Result<()> {
    /*!
    Set the permissions of the file the descriptor is bound to
    !*/
    if unsafe {
        libc::fchmod(descriptor.as_raw_fd(), mode as libc::mode_t)
    } != 0 {
        return Err(Error::last_os_error())
    }
    Ok(())
}

fn mode_of(descriptor: &OwnedFd) -> Result<libc::mode_t> {
    /*!
    Provide the mode, file type and permissions, of the file the
    descriptor is bound to
    !*/
    let mut attributes: libc::stat = unsafe { mem::zeroed() };
    if unsafe {
        libc::fstat(descriptor.as_raw_fd(), &mut attributes)
    } != 0 {
        return Err(Error::last_os_error())
    }
    Ok(attributes.st_mode)
}

fn to_c_string(name: &str) -> Result<CString> {
    /*!
    Convert name to the representation the kernel expects
    !*/
    CString::new(name.as_bytes()).map_err(
        |_| Error::new(
            ErrorKind::InvalidInput,
            format!("{name} contains a null byte")
        )
    )
}

#[cfg(test)]
mod tests {
    use super::{copy, create_dir_all, set_mode, NO_OPENAT2};
    use std::fs;
    use std::sync::atomic::Ordering;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::Path;
    use tempfile::tempdir;

    fn mode_of(path: &str) -> u32 {
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
    }

    #[test]
    fn test_create_dir_all_creates_the_path() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let dirname = format!("{base}/some/deep/dir");
        create_dir_all(&dirname, 0o700).unwrap();
        assert!(Path::new(&dirname).is_dir());
        assert_eq!(0o700, mode_of(&dirname));
        // the directories above it are not created with the mode
        // of the directory which was asked for
        assert_eq!(0o755, mode_of(&format!("{base}/some")));
    }

    #[test]
    fn test_create_dir_all_sets_the_mode_despite_the_umask() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let dirname = format!("{base}/dir");
        create_dir_all(&dirname, 0o1777).unwrap();
        assert_eq!(0o1777, mode_of(&dirname));
    }

    #[test]
    fn test_create_dir_all_keeps_an_existing_directory() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let dirname = format!("{base}/dir");
        fs::create_dir(&dirname).unwrap();
        fs::set_permissions(&dirname, fs::Permissions::from_mode(0o750))
            .unwrap();
        create_dir_all(&dirname, 0o700).unwrap();
        assert_eq!(0o750, mode_of(&dirname));
    }

    #[test]
    fn test_create_dir_all_refuses_a_symlink_in_the_path() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        fs::create_dir(format!("{base}/target")).unwrap();
        symlink(format!("{base}/target"), format!("{base}/link")).unwrap();
        assert!(create_dir_all(&format!("{base}/link/sub"), 0o755).is_err());
        assert!(! Path::new(&format!("{base}/target/sub")).exists());
        // a link as the last component is rejected as well
        assert!(create_dir_all(&format!("{base}/link"), 0o755).is_err());
    }

    #[test]
    fn test_create_dir_all_refuses_a_relative_path_component() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        assert!(create_dir_all(&format!("{base}/../dir"), 0o755).is_err());
    }

    #[test]
    fn test_set_mode_changes_the_file() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let filename = format!("{base}/file");
        fs::write(&filename, "data").unwrap();
        set_mode(&filename, 0o600).unwrap();
        assert_eq!(0o600, mode_of(&filename));
    }

    #[test]
    fn test_set_mode_refuses_a_symlink() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let filename = format!("{base}/file");
        fs::write(&filename, "data").unwrap();
        fs::set_permissions(&filename, fs::Permissions::from_mode(0o644))
            .unwrap();
        symlink(&filename, format!("{base}/link")).unwrap();
        assert!(set_mode(&format!("{base}/link"), 0o600).is_err());
        assert!(set_mode(&format!("{base}/link/sub"), 0o600).is_err());
        // the file the link points to was not touched
        assert_eq!(0o644, mode_of(&filename));
    }

    #[test]
    fn test_copy_creates_the_target() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let source = format!("{base}/source");
        let target = format!("{base}/target");
        fs::write(&source, "data").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o640))
            .unwrap();
        copy(&source, &target).unwrap();
        assert_eq!("data", fs::read_to_string(&target).unwrap());
        assert_eq!(0o640, mode_of(&target));
    }

    #[test]
    fn test_copy_truncates_an_existing_target() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let source = format!("{base}/source");
        let target = format!("{base}/target");
        fs::write(&source, "data").unwrap();
        fs::write(&target, "some longer data").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
            .unwrap();
        copy(&source, &target).unwrap();
        assert_eq!("data", fs::read_to_string(&target).unwrap());
        // an existing target keeps its permissions
        assert_eq!(0o600, mode_of(&target));
    }

    #[test]
    fn test_symlink_is_refused_without_openat2() {
        /*!
        The fallback for a kernel older than 5.6 has to protect
        the same way. This is the only test switching the flag.
        Other tests running at the same time may take the
        fallback as well, they must pass either way
        !*/
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        create_dir_all(&format!("{base}/dir"), 0o755).unwrap();
        // The flag is only set if the syscall returned ENOSYS.
        // Its state proves which of the two ways was taken
        assert!(! NO_OPENAT2.load(Ordering::Relaxed));
        fs::create_dir(format!("{base}/target")).unwrap();
        fs::write(format!("{base}/target/file"), "data").unwrap();
        fs::set_permissions(
            format!("{base}/target/file"), fs::Permissions::from_mode(0o644)
        ).unwrap();
        symlink(format!("{base}/target"), format!("{base}/link")).unwrap();
        symlink(
            format!("{base}/target/file"), format!("{base}/file_link")
        ).unwrap();
        NO_OPENAT2.store(true, Ordering::Relaxed);
        let rejected = [
            create_dir_all(&format!("{base}/link/sub"), 0o755).is_err(),
            create_dir_all(&format!("{base}/link"), 0o755).is_err(),
            set_mode(&format!("{base}/file_link"), 0o600).is_err(),
            copy(&format!("{base}/file_link"), &format!("{base}/copy"))
                .is_err()
        ];
        NO_OPENAT2.store(false, Ordering::Relaxed);
        assert_eq!([true, true, true, true], rejected);
        assert!(! Path::new(&format!("{base}/target/sub")).exists());
        assert!(! Path::new(&format!("{base}/copy")).exists());
        // the file behind the link kept its permissions
        assert_eq!(0o644, mode_of(&format!("{base}/target/file")));
    }

    #[test]
    fn test_copy_refuses_a_symlink() {
        let workdir = tempdir().unwrap();
        let base = workdir.path().to_str().unwrap();
        let source = format!("{base}/source");
        let secret = format!("{base}/secret");
        fs::write(&source, "data").unwrap();
        fs::write(&secret, "secret").unwrap();
        symlink(&secret, format!("{base}/link")).unwrap();
        // neither reading through a link the caller placed...
        assert!(copy(&format!("{base}/link"), &format!("{base}/copy")).is_err());
        assert!(! Path::new(&format!("{base}/copy")).exists());
        // ...nor writing through it
        assert!(copy(&source, &format!("{base}/link")).is_err());
        assert_eq!("secret", fs::read_to_string(&secret).unwrap());
    }
}
