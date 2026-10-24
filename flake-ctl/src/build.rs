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
use flakes::config::{get_kiwi_container, get_user_home};
use flakes::defaults::PODMAN_PATH;
use serde_json::Value;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::BoxbuildOptions;
use crate::defaults;
use crate::firecracker::{checksum, mv, tempdir, verify_checksum};

pub fn build_kis_image(
    kiwi_image: &str, outfile: &str, boxbuild: &BoxbuildOptions,
    usermode: bool
) -> i32 {
    /*!
    Build the KIWI image description kiwi_image as KIS image and
    store the resulting archive as outfile

    The build runs 'kiwi system boxbuild' in an instance of the
    configured kiwi container, see get_kiwi_container. Along with
    the archive a checksum file named like outfile plus a '.sha256'
    suffix is created, which allows to pull the result via
    'firecracker pull --kis-image outfile'. The log of the build
    is kept as outfile plus a '.log' suffix, also if the build
    has failed
    !*/
    let result = 255;

    // Check the paths upfront, a build takes long and should not
    // fail at the end because the result cannot be stored
    let description = match real_dir(kiwi_image) {
        Some(description) => description,
        None => return result
    };
    let outfile = match outfile_path(outfile) {
        Some(outfile) => outfile,
        None => return result
    };
    let shared_path = match &boxbuild.shared_path {
        Some(shared_path) => match real_dir(shared_path) {
            Some(shared_path) => Some(shared_path),
            None => return result
        },
        None => None
    };
    let boxes_dir = PathBuf::from(
        format!("{}/{}", get_user_home(), defaults::KIWI_BOXES_NAME)
    );
    if let Err(error) = fs::create_dir_all(&boxes_dir) {
        error!("Failed to create {}: {error}", boxes_dir.display());
        return result
    }

    let tmp_dir = match tempdir() {
        Ok(tmp_dir) => tmp_dir,
        Err(error) => {
            error!(
                "Failed to create tempdir in {}: {}", defaults::TEMP_DIR, error
            );
            return result
        }
    };
    let target_dir = tmp_dir.path().to_path_buf();

    let mut build_result = match boxbuild_call(
        &description, &target_dir, &boxes_dir,
        shared_path.as_deref(), boxbuild
    ) {
        Some(mut call) => run_build(&mut call),
        None => result
    };
    if build_result == 0 && (
        ! box_build_succeeded(&target_dir)
        || ! store_kis_archive(&target_dir, &outfile)
    ) {
        build_result = result
    }
    keep_build_log(&target_dir, &outfile);

    // Data created by the build which belongs to a user that only
    // exists in the user namespace of the rootless container cannot
    // be deleted by the calling user. It is deleted from within
    // that user namespace
    if let Err(error) = tmp_dir.close() {
        if usermode {
            let mut call = Command::new(PODMAN_PATH);
            call.arg("unshare").arg("rm").arg("-rf").arg(&target_dir);
            if ! matches!(call.status(), Ok(status) if status.success()) {
                warn!(
                    "Failed to delete build directory {}",
                    target_dir.display()
                );
            }
        } else {
            warn!(
                "Failed to delete build directory {}: {error}",
                target_dir.display()
            );
        }
    }
    build_result
}

fn boxbuild_call(
    description: &Path, target_dir: &Path, boxes_dir: &Path,
    shared_path: Option<&Path>, boxbuild: &BoxbuildOptions
) -> Option<Command> {
    /*!
    Create the podman call which runs the kiwi boxbuild of the
    given description in the kiwi container

    The kiwi container runs privileged as the box is a KVM based
    virtual machine or another container started from within the
    kiwi container
    !*/
    let mut call = Command::new(PODMAN_PATH);
    call.arg("run")
        .arg("--rm")
        .arg("--privileged")
        .arg("--pull=newer");
    // The box console is passed through if there is a terminal,
    // e.g to work in the box kept open by --box-debug
    if std::io::stdin().is_terminal() {
        call.arg("--interactive").arg("--tty");
    }
    call.arg("--volume").arg(
        volume(boxes_dir, Path::new(defaults::KIWI_BOXES_DIR))?
    );
    call.arg("--volume").arg(
        volume(description, Path::new(defaults::KIWI_DESCRIPTION_DIR))?
    );
    call.arg("--volume").arg(
        volume(target_dir, Path::new(defaults::KIWI_TARGET_DIR))?
    );
    // The shared path is expected at the same location in the
    // box as on the host. kiwi shares it from the container
    // into the box and therefore it has to exist there too
    if let Some(shared_path) = shared_path {
        call.arg("--volume").arg(volume(shared_path, shared_path)?);
    }
    call.arg(get_kiwi_container())
        .arg("system").arg("boxbuild")
        .args(boxbuild_args(boxbuild, shared_path))
        .arg("kiwi")
        .arg("--description").arg(defaults::KIWI_DESCRIPTION_DIR)
        .arg("--target-dir").arg(defaults::KIWI_TARGET_DIR);
    Some(call)
}

fn boxbuild_args(
    boxbuild: &BoxbuildOptions, shared_path: Option<&Path>
) -> Vec<String> {
    /*!
    Translate the boxbuild options into kiwi boxbuild arguments
    !*/
    let mut args: Vec<String> = vec![
        "--box".to_string(), boxbuild.boxname.clone()
    ];
    let mut value = |option: &str, value: Option<String>| {
        if let Some(value) = value {
            args.push(option.to_string());
            args.push(value);
        }
    };
    value("--box-memory", boxbuild.box_memory.clone());
    value("--box-console", boxbuild.box_console.clone());
    value("--box-smp-cpus", boxbuild.box_smp_cpus.map(|n| n.to_string()));
    value("--kiwi-version", boxbuild.kiwi_version.clone());
    value("--shared-path", shared_path.map(|p| p.display().to_string()));
    value("--ssh-key", boxbuild.ssh_key.clone());
    value("--ssh-port", boxbuild.ssh_port.map(|n| n.to_string()));
    value("--machine", boxbuild.machine.clone());
    value("--cpu", boxbuild.cpu.clone());
    for (option, set) in [
        ("--box-debug", boxbuild.box_debug),
        ("--container", boxbuild.container),
        ("--no-update-check", boxbuild.no_update_check),
        ("--no-snapshot", boxbuild.no_snapshot),
        ("--no-accel", boxbuild.no_accel),
        ("--9p-sharing", boxbuild.qemu_9p_sharing),
        ("--virtiofs-sharing", boxbuild.virtiofs_sharing),
        ("--sshfs-sharing", boxbuild.sshfs_sharing),
        ("--x86_64", boxbuild.x86_64),
        ("--aarch64", boxbuild.aarch64),
    ] {
        if set {
            args.push(option.to_string());
        }
    }
    args
}

fn run_build(call: &mut Command) -> i32 {
    /*!
    Run the build and provide its exit code
    !*/
    info!("Building KIS image...");
    debug!("{call:?}");
    match call.status() {
        Ok(status) => {
            match status.code() {
                Some(0) => 0,
                Some(code) => {
                    error!("Build failed with exit code {code}");
                    code
                },
                None => {
                    error!("Build terminated by signal");
                    255
                }
            }
        },
        Err(error) => {
            error!("Failed to execute {PODMAN_PATH}: {error}");
            255
        }
    }
}

fn box_build_succeeded(target_dir: &Path) -> bool {
    /*!
    Check the exit code of the build in the box

    The box stores it in the target directory. kiwi boxbuild does
    not fail if the box did not store an exit code at all, e.g if
    the box could not be started. Such a build is a failure too
    !*/
    let exit_code_file = target_dir.join(defaults::KIWI_RESULT_CODE_NAME);
    match fs::read_to_string(&exit_code_file) {
        Ok(exit_code) if exit_code.trim() == "0" => true,
        Ok(exit_code) => {
            error!("Box build failed with exit code {}", exit_code.trim());
            false
        },
        Err(error) => {
            error!(
                "Box build stored no exit code in {}: {error}",
                exit_code_file.display()
            );
            false
        }
    }
}

fn store_kis_archive(target_dir: &Path, outfile: &Path) -> bool {
    /*!
    Move the KIS archive built in target_dir to outfile and
    create the checksum file next to it

    An archive which comes with a checksum file created by kiwi
    is verified against it first
    !*/
    let archive = match find_kis_archive(target_dir) {
        Some(archive) => archive,
        None => return false
    };
    let archive_name = archive.to_string_lossy().to_string();
    if let Ok(record) = fs::read_to_string(format!("{archive_name}.sha256")) {
        info!("Verifying archive checksum...");
        if ! verify_checksum(&archive_name, &record) {
            error!("Archive checksum verification failed");
            return false
        }
    }
    let outfile_name = outfile.to_string_lossy().to_string();
    if ! mv(&archive_name, &outfile_name) {
        return false
    }
    let checksum_file = format!("{outfile_name}.sha256");
    let sum = match checksum(&outfile_name, None) {
        Some(sum) => sum,
        None => return false
    };
    // sha256sum format, the file name is the one of the archive
    // next to the checksum file
    let file_name = outfile.file_name().unwrap_or_default().to_string_lossy();
    if let Err(error) = fs::write(&checksum_file, format!("{sum}  {file_name}\n")) {
        error!("Failed to write {checksum_file}: {error}");
        return false
    }
    info!("KIS image: {outfile_name}");
    info!("Checksum: {checksum_file}");
    true
}

fn keep_build_log(target_dir: &Path, outfile: &Path) {
    /*!
    Keep the log the box has stored in the target directory as
    outfile plus a '.log' suffix

    The target directory is deleted after the build, the log is
    kept to look into the build, in particular a failed one. A log
    of a former build of the same outfile is deleted if the box
    has stored no log, e.g because the box could not be started
    !*/
    let build_log = target_dir.join(defaults::KIWI_RESULT_LOG_NAME);
    let mut outfile_log = outfile.as_os_str().to_owned();
    outfile_log.push(".log");
    let outfile_log = PathBuf::from(outfile_log);
    if build_log.exists() {
        match fs::copy(&build_log, &outfile_log) {
            Ok(_) => info!("Build log: {}", outfile_log.display()),
            Err(error) => {
                warn!("Failed to keep build log {}: {error}", outfile_log.display());
            }
        }
    } else {
        warn!("Box build stored no log");
        if let Err(error) = fs::remove_file(&outfile_log) {
            if error.kind() != io::ErrorKind::NotFound {
                warn!(
                    "Failed to delete former build log {}: {error}",
                    outfile_log.display()
                );
            }
        }
    }
}

fn find_kis_archive(target_dir: &Path) -> Option<PathBuf> {
    /*!
    Find the KIS archive in the target directory of the build

    A build in a box VM keeps the build result in the VM. Only the
    result bundle created from it is stored in the target directory,
    there is no kiwi result file. The bundle consists of the KIS
    archive, its checksum file and the package lists. The name of
    the archive carries the bundle id. If the bundle renames the
    archive, a symlink of the original name points to the archive
    at its location in the VM, which does not exist on the host.

    A build in a box container stores the build result itself in
    the target directory, including the kiwi result file which
    names the archive. The name stored there is the one of the
    build environment, thus only its base name is used
    !*/
    let result_file = target_dir.join(defaults::KIWI_RESULT_NAME);
    if result_file.exists() {
        let kiwi_result: Value = match fs::read_to_string(&result_file)
            .map_err(|error| error.to_string())
            .and_then(|data| {
                serde_json::from_str(&data).map_err(|error| error.to_string())
            })
        {
            Ok(kiwi_result) => kiwi_result,
            Err(error) => {
                error!("Failed to read {}: {error}", result_file.display());
                return None
            }
        };
        return match kiwi_result[defaults::KIWI_RESULT_KIS_ARCHIVE]
            ["filename"].as_str()
            .and_then(|filename| Path::new(filename).file_name())
        {
            Some(archive_name) => Some(target_dir.join(archive_name)),
            None => {
                error!(
                    "No KIS archive in build result, the image description \
                    is expected to build the kis image type"
                );
                None
            }
        }
    }
    let entries = match fs::read_dir(target_dir) {
        Ok(entries) => entries,
        Err(error) => {
            error!("Failed to read {}: {error}", target_dir.display());
            return None
        }
    };
    let mut archives: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_type().is_ok_and(|file_type| file_type.is_file())
        })
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.to_string_lossy();
            name.ends_with(".tar.xz") || name.ends_with(".tar")
        })
        .collect();
    match archives.len() {
        1 => archives.pop(),
        0 => {
            error!(
                "No KIS archive in build result, the image description \
                is expected to build the kis image type"
            );
            None
        },
        _ => {
            error!("Build result is ambiguous, found archives: {archives:?}");
            None
        }
    }
}

fn real_dir(path: &str) -> Option<PathBuf> {
    /*!
    Provide the real path of the given directory
    !*/
    match fs::canonicalize(path) {
        Ok(real_path) if real_path.is_dir() => Some(real_path),
        Ok(_) => {
            error!("{path} is not a directory");
            None
        },
        Err(error) => {
            error!("Failed to access {path}: {error}");
            None
        }
    }
}

fn outfile_path(outfile: &str) -> Option<PathBuf> {
    /*!
    Provide the absolute path of the outfile

    The directory to store the outfile in has to exist and the
    outfile itself must not be a directory. An existing file
    gets replaced
    !*/
    let path = Path::new(outfile);
    let file_name = match path.file_name() {
        Some(file_name) => file_name,
        None => {
            error!("Invalid outfile {outfile}: no file name");
            return None
        }
    };
    let parent = match path.parent() {
        Some(parent) if ! parent.as_os_str().is_empty() => parent,
        _ => Path::new(".")
    };
    let outfile = real_dir(&parent.to_string_lossy())?.join(file_name);
    if outfile.is_dir() {
        error!("Outfile {} is a directory", outfile.display());
        return None
    }
    Some(outfile)
}

fn volume(source: &Path, target: &Path) -> Option<String> {
    /*!
    Provide the podman volume specification which mounts the
    host path source to target in the container

    The ':' separates the fields of the specification and can
    therefore not be part of a path
    !*/
    let source = source.to_string_lossy();
    let target = target.to_string_lossy();
    if source.contains(':') || target.contains(':') {
        error!("Path {source} must not contain ':' to be mounted");
        return None
    }
    Some(format!("{source}:{target}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Commands, Firecracker};
    use clap::Parser;
    use std::os::unix::fs::symlink;

    fn parse_boxbuild(args: &[&str]) -> BoxbuildOptions {
        let mut call = vec![
            "flake-ctl", "firecracker", "build",
            "--kiwi-image", "desc", "--outfile", "out.kis"
        ];
        call.extend_from_slice(args);
        match Cli::parse_from(call).command {
            Commands::Firecracker {
                command: Firecracker::Build { boxbuild, .. }
            } => boxbuild,
            _ => panic!("not a build command")
        }
    }

    #[test]
    fn test_boxbuild_args() {
        // tumbleweed is the default box
        assert_eq!(boxbuild_args(&parse_boxbuild(&[]), None), [
            "--box", "tumbleweed"
        ]);
        let boxbuild = parse_boxbuild(&[
            "--box", "universal", "--box-memory", "16", "--box-smp-cpus", "8",
            "--box-debug", "--no-snapshot", "--9p-sharing", "--x86_64",
            "--ssh-port", "2222", "--cpu", "max", "--shared-path", "/var/tmp"
        ]);
        assert_eq!(boxbuild_args(&boxbuild, Some(Path::new("/var/tmp"))), [
            "--box", "universal", "--box-memory", "16", "--box-smp-cpus", "8",
            "--shared-path", "/var/tmp", "--ssh-port", "2222", "--cpu", "max",
            "--box-debug", "--no-snapshot", "--9p-sharing", "--x86_64"
        ]);
    }

    #[test]
    fn test_boxbuild_call() {
        let call = boxbuild_call(
            Path::new("/desc"), Path::new("/target"), Path::new("/boxes"),
            Some(Path::new("/shared")), &parse_boxbuild(&[])
        ).unwrap();
        let args: Vec<String> = call.get_args()
            .map(|arg| arg.to_string_lossy().to_string()).collect();
        assert_eq!(call.get_program(), PODMAN_PATH);
        for volume in [
            "/boxes:/root/.kiwi_boxes", "/desc:/description",
            "/target:/result", "/shared:/shared"
        ] {
            assert!(args.contains(&volume.to_string()), "{}", volume);
        }
        assert!(args.ends_with(&[
            get_kiwi_container(), "system".to_string(),
            "boxbuild".to_string(), "--box".to_string(),
            "tumbleweed".to_string(), "--shared-path".to_string(),
            "/shared".to_string(), "kiwi".to_string(),
            "--description".to_string(), "/description".to_string(),
            "--target-dir".to_string(), "/result".to_string()
        ]));

        // a path which cannot be expressed as podman volume
        assert!(boxbuild_call(
            Path::new("/de:sc"), Path::new("/target"), Path::new("/boxes"),
            None, &parse_boxbuild(&[])
        ).is_none());
    }

    #[test]
    fn test_box_build_succeeded() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let exit_code_file = tmp_dir.path().join("result.code");
        assert!(! box_build_succeeded(tmp_dir.path()));
        fs::write(&exit_code_file, "1\n").unwrap();
        assert!(! box_build_succeeded(tmp_dir.path()));
        fs::write(&exit_code_file, "0\n").unwrap();
        assert!(box_build_succeeded(tmp_dir.path()));
    }

    fn sha256_record(file: &Path) -> String {
        format!(
            "{}  {}\n", checksum(&file.to_string_lossy(), None).unwrap(),
            file.file_name().unwrap().to_string_lossy()
        )
    }

    fn assert_stored(tmp_dir: &Path, outfile: &Path, data: &str) {
        assert_eq!(fs::read_to_string(outfile).unwrap(), data);
        let record = fs::read_to_string(
            tmp_dir.join("result.kis.sha256")
        ).unwrap();
        assert!(record.ends_with("  result.kis\n"));
        assert!(crate::firecracker::verify_checksum(
            &outfile.to_string_lossy(), &record
        ));
    }

    #[test]
    fn test_store_kis_archive_from_box_vm() {
        // A build in a box VM provides the result bundle only
        let tmp_dir = tempfile::tempdir().unwrap();
        let target_dir = tmp_dir.path().join("target");
        fs::create_dir(&target_dir).unwrap();
        let outfile = tmp_dir.path().join("result.kis");

        // no archive
        fs::write(target_dir.join("result.code"), "0\n").unwrap();
        fs::write(target_dir.join("result.log"), "log").unwrap();
        fs::write(target_dir.join("foo.x86_64-1.0-0.packages"), "").unwrap();
        assert!(! store_kis_archive(&target_dir, &outfile));

        // the bundled archive with the checksum file of kiwi and
        // the symlink of the original name which only resolves in
        // the box
        let archive = target_dir.join("foo.x86_64-1.0-0.tar.xz");
        fs::write(&archive, "kis").unwrap();
        symlink(
            "/bundle/foo.x86_64-1.0-0.tar.xz",
            target_dir.join("foo.x86_64-1.0.tar.xz")
        ).unwrap();
        let kiwi_checksum_file = target_dir.join("foo.x86_64-1.0-0.tar.xz.sha256");

        // a checksum mismatch fails
        fs::write(&kiwi_checksum_file, "0000  foo.x86_64-1.0-0.tar.xz\n").unwrap();
        assert!(! store_kis_archive(&target_dir, &outfile));
        assert!(archive.exists());

        fs::write(&kiwi_checksum_file, sha256_record(&archive)).unwrap();
        assert!(store_kis_archive(&target_dir, &outfile));
        assert!(! archive.exists());
        assert_stored(tmp_dir.path(), &outfile, "kis");

        // more than one archive is ambiguous
        fs::write(target_dir.join("a.tar.xz"), "a").unwrap();
        fs::write(target_dir.join("b.tar.xz"), "b").unwrap();
        assert!(! store_kis_archive(&target_dir, &outfile));
    }

    #[test]
    fn test_store_kis_archive_from_box_container() {
        // A build in a box container provides the kiwi result file
        let tmp_dir = tempfile::tempdir().unwrap();
        let target_dir = tmp_dir.path().join("target");
        fs::create_dir(&target_dir).unwrap();
        let outfile = tmp_dir.path().join("result.kis");

        // a result without a KIS archive
        let result_file = target_dir.join(defaults::KIWI_RESULT_NAME);
        fs::write(&result_file, r#"{"image_packages": {}}"#).unwrap();
        assert!(! store_kis_archive(&target_dir, &outfile));

        // the archive is found by the base name of the file name
        // the build has recorded
        fs::write(&result_file, r#"{"kis_archive": {
            "filename": "/bundle/foo.x86_64-1.0.tar.xz"
        }}"#).unwrap();
        fs::write(target_dir.join("foo.x86_64-1.0.tar.xz"), "kis").unwrap();
        fs::write(target_dir.join("other.tar.xz"), "other").unwrap();
        assert!(store_kis_archive(&target_dir, &outfile));
        assert!(! target_dir.join("foo.x86_64-1.0.tar.xz").exists());
        assert_stored(tmp_dir.path(), &outfile, "kis");
    }

    #[test]
    fn test_keep_build_log() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let target_dir = tmp_dir.path().join("target");
        fs::create_dir(&target_dir).unwrap();
        let outfile = tmp_dir.path().join("result.kis");
        let outfile_log = tmp_dir.path().join("result.kis.log");

        // the log of the box is kept next to the outfile
        fs::write(target_dir.join("result.log"), "build log").unwrap();
        keep_build_log(&target_dir, &outfile);
        assert_eq!(fs::read_to_string(&outfile_log).unwrap(), "build log");

        // a box which stored no log leaves no former log behind
        fs::remove_file(target_dir.join("result.log")).unwrap();
        keep_build_log(&target_dir, &outfile);
        assert!(! outfile_log.exists());
    }

    #[test]
    fn test_outfile_path() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let real_dir = fs::canonicalize(tmp_dir.path()).unwrap();
        let outfile = real_dir.join("out.kis");
        assert_eq!(outfile_path(&outfile.to_string_lossy()), Some(outfile));
        assert!(outfile_path(&real_dir.to_string_lossy()).is_none());
        assert!(outfile_path("/nonexistent/out.kis").is_none());
        assert!(outfile_path("out.kis").unwrap().is_absolute());
    }
}
