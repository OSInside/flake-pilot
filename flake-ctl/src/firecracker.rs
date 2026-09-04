//
// Copyright (c) 2022 Elektrobit Automotive GmbH
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
use flakes::config::get_flakes_dir;
use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use tempfile::{Builder, NamedTempFile, TempDir};
use std::path::Path;
use std::borrow::Cow;
use std::fs;
use std::fs::File;

use crate::defaults;
use crate::network;
use crate::{app, app_config};

use crate::fetch::{fetch_file, send_request};

const FLAKE_PILOT_NFS_EXPORT_MARKER: &str =
    "# flake-pilot firecracker volume";

pub fn get_registry_dir(usermode: bool) -> String {
    /*!
    Provide the toplevel firecracker registry directory

    In user mode the registry is a private directory of the
    calling user below its flakes directory. In all other
    cases the system wide registry is used
    !*/
    if usermode {
        format!(
            "{}/{}", get_flakes_dir(true), defaults::FIRECRACKER_REGISTRY_NAME
        )
    } else {
        defaults::FIRECRACKER_REGISTRY_DIR.to_string()
    }
}

pub fn get_images_dir(usermode: bool) -> String {
    /*!
    Provide the directory the firecracker images are stored in
    !*/
    format!(
        "{}/{}", get_registry_dir(usermode), defaults::FIRECRACKER_IMAGES_NAME
    )
}

pub fn get_image_dir(name: &str, usermode: bool) -> String {
    /*!
    Provide the directory of the image registered as name
    !*/
    format!("{}/{}", get_images_dir(usermode), name)
}

pub fn registry_user(usermode: bool) -> &'static str {
    /*!
    Provide the user owning the firecracker registry

    The system wide registry belongs to root whereas the user
    registry belongs to the calling user itself. The latter is
    expressed as an empty user which means no privilege change
    is needed to operate on the registry
    !*/
    if usermode { "" } else { "root" }
}

pub fn init_toplevel_image_dir(registry_dir: &str) -> bool {
    /*!
    Create firecracker registry directory layout
    !*/
    let mut ok = true;
    let mut real_registry_dir = String::new();
    match fs::read_link(registry_dir) {
        Ok(target) => {
            real_registry_dir.push_str(
                &target.into_os_string().into_string().unwrap()
            );
        },
        Err(_) => {
            real_registry_dir.push_str(registry_dir);
        }
    }
    let mut subdirs: Vec<String> = Vec::new();
    subdirs.push(format!(
        "{}/{}", real_registry_dir, defaults::FIRECRACKER_IMAGES_NAME
    ));
    subdirs.push(format!(
        "{}/{}", real_registry_dir, defaults::FIRECRACKER_STORAGE_NAME
    ));
    for subdir in subdirs {
        if Path::new(&subdir).exists() {
            continue
        }
        match fs::create_dir_all(&subdir) {
            Ok(_) => {
                match fs::metadata(&subdir) {
                    Ok(attr) => {
                        let mut permissions = attr.permissions();
                        // The images in this directory are the root
                        // filesystem and the kernel of a VM. They must
                        // not be modifiable by everybody
                        permissions.set_mode(0o755);
                        match fs::set_permissions(&subdir, permissions) {
                            Ok(_) => { },
                            Err(error) => {
                                error!(
                                    "Failed to set 0o755 bits: {} {}",
                                    subdir, error
                                );
                                ok = false
                            }
                        }
                    },
                    Err(error) => {
                        error!(
                            "Failed to fetch attributes: {} {}", subdir, error
                        );
                        ok = false
                    }
                }
            },
            Err(error) => {
                error!("Error creating directory {}: {}", subdir, error);
                ok = false
            }
        }
    }
    ok
}

fn tempdir() -> io::Result<TempDir> {
    /*!
    Create a temporary directory below defaults::TEMP_DIR

    Image data is downloaded to a temporary location before it is
    moved to its final destination in the registry. The system
    default temporary directory /tmp is in most cases backed by a
    memory filesystem which is too small to hold an image. Thus
    the temporary data is placed in defaults::TEMP_DIR which is
    expected to be persistent storage
    !*/
    Builder::new()
        .prefix(defaults::TEMP_PREFIX)
        .tempdir_in(defaults::TEMP_DIR)
}

pub async fn pull_component_image(
    name: &str, rootfs_uri: Option<&String>, kernel_uri: Option<&String>,
    initrd_uri: Option<&String>, force: bool, usermode: bool
) -> i32 {
    /*!
    Fetch components image consisting out of rootfs, kernel and
    optional initrd.
    !*/
    let mut result = 255;
    let image_dir = get_image_dir(name, usermode);
    struct Component<'a> {
        uri: String,
        file: Cow<'a, str>
    }
    info!("Fetching Component image...");
    if ! pull_new(name, force, usermode) {
        return result
    }
    match tempdir() {
        Ok(tmp_dir) => {
            let mut download_files: Vec<Component> = Vec::new();
            let rootfs_file = tmp_dir.path().join("rootfs")
                .into_os_string().into_string().unwrap();
            let kernel_file = tmp_dir.path().join("kernel")
                .into_os_string().into_string().unwrap();
            let initrd_file = tmp_dir.path().join("initrd")
                .into_os_string().into_string().unwrap();
            download_files.push(
                Component {
                    uri: rootfs_uri.unwrap().to_string(),
                    file: Cow::Borrowed(&rootfs_file),
                }
            );
            download_files.push(
                Component {
                    uri: kernel_uri.unwrap().to_string(),
                    file: Cow::Borrowed(&kernel_file),
                }
            );
            if let Some(initrd_uri) = initrd_uri {
                download_files.push(
                    Component {
                        uri: initrd_uri.to_string(),
                        file: Cow::Borrowed(&initrd_file),
                    }
                );
            }
            // Download...
            for component in download_files {
                match send_request(&component.uri).await {
                    Ok(response) => {
                        result = response.status().as_u16().into();
                        match fetch_file(
                            response, &component.file.into_owned()).await
                        {
                            Ok(_) => { },
                            Err(error) => {
                                error!(
                                    "Download failed with: {error}"
                                );
                                return result
                            }
                        }
                    },
                    Err(error) => {
                        error!(
                            "Request to '{}' failed with: {}",
                            component.uri, error
                        );
                        return result
                    }
                }
            }
            // Check for sci and add it to rootfs image if not present
            let tmp_dir_path = tmp_dir.path().display().to_string();
            if mount_fs_image(&rootfs_file, &tmp_dir_path, "root") {
                let sci_in_image = format!(
                    "{}/{}", tmp_dir_path, "/usr/sbin/sci"
                );
                // required for overlay mount process
                let overlay_root_in_image = format!(
                    "{}/{}", tmp_dir_path, "/overlayroot"
                );
                // required for /proc/sys/kernel/sysrq based force_reboot
                let proc_in_image = format!(
                    "{}/{}", tmp_dir_path, "/proc"
                );
                // required for the switch root into the overlay
                let sys_in_image = format!(
                    "{}/{}", tmp_dir_path, "/sys"
                );
                let dev_in_image = format!(
                    "{}/{}", tmp_dir_path, "/dev"
                );
                // required for PTS allocation
                let dev_pts_in_image = format!(
                    "{}/{}", tmp_dir_path, "/dev/pts"
                );
                if ! Path::new(&sci_in_image).exists() {
                    info!("Copying sci to rootfs...");
                    if ! copy(
                        defaults::FIRECRACKER_SCI, &sci_in_image, "root"
                    ) {
                        umount(&tmp_dir_path, "root");
                        return result
                    }
                }
                if ! Path::new(&overlay_root_in_image).exists() && ! mkdir(&overlay_root_in_image, "root") {
                    umount(&tmp_dir_path, "root");
                    return result
                }
                if ! Path::new(&proc_in_image).exists() && ! mkdir(&proc_in_image, "root") {
                    umount(&tmp_dir_path, "root");
                    return result
                }
                if ! Path::new(&sys_in_image).exists() && ! mkdir(&sys_in_image, "root") {
                    umount(&tmp_dir_path, "root");
                    return result
                }
                if ! Path::new(&dev_in_image).exists() && ! mkdir(&dev_in_image, "root") {
                    umount(&tmp_dir_path, "root");
                    return result
                }
                if ! Path::new(&dev_pts_in_image).exists() && ! mkdir(&dev_pts_in_image, "root") {
                    umount(&tmp_dir_path, "root");
                    return result
                }
                umount(&tmp_dir_path, "root");
            }

            // Move to final firecracker image store
            if ! mv(&tmp_dir_path, &image_dir, registry_user(usermode)) {
                return result
            }
        },
        Err(error) => {
            error!(
                "Failed to create tempdir in {}: {}", defaults::TEMP_DIR, error
            );
            return result
        }
    }
    result
}

pub async fn pull_kis_image(
    name: &str, uri: Option<&String>, force: bool, usermode: bool
) -> i32 {
    /*!
    Fetch the data provided in uri and treat it as a KIWI
    built KIS image type. This means the file behind uri
    is expected to be a tarball containing the KIS
    components; rootfs-image, kernel and optional initrd

    The archive must be accompanied by a checksum file of the
    same name plus a '.sha256' suffix. That checksum verifies
    the download and it is kept with the image such that a
    later pull of the same name can tell whether the image in
    the registry is still up to date. An image which is up to
    date is not fetched again and does not fail the pull
    !*/
    let mut result = 255;
    let image_dir = get_image_dir(name, usermode);
    let uri = uri.unwrap();

    info!("Fetching KIS image...");

    let pull_state = match pull_init(name, force, usermode) {
        Some(pull_state) => pull_state,
        None => return result
    };

    // Fetch the checksum record that belongs to the archive.
    // Without it neither the download can be verified nor can
    // an existing image be checked for an update
    let source_checksum = match fetch_source_checksum(uri).await {
        Some(source_checksum) => source_checksum,
        None => return result
    };

    // An image of that name is already in the registry. Compare
    // it against the checksum of its origin to find out whether
    // there is anything to update
    if pull_state == PullState::Update {
        if image_is_current(&image_dir, &source_checksum) {
            info!("Image '{name}' is up to date");
            return 0
        }
        info!("Image '{name}' is out of date, pulling latest version...");
    }

    match tempdir() {
        Ok(tmp_dir) => {
            let work_dir = tmp_dir.path().join("work")
                .into_os_string().into_string().unwrap();
            let kis_tar = tmp_dir.path().join("kis_archive")
                .into_os_string().into_string().unwrap();

            // Download...
            match fs::create_dir_all(&work_dir) {
                Ok(_) => {
                    match send_request(uri).await {
                        Ok(response) => {
                            result = response.status().as_u16().into();
                            match fetch_file(response, &kis_tar).await {
                                Ok(_) => { },
                                Err(error) => {
                                    error!("Download failed with: {error}");
                                    return result
                                }
                            }
                        },
                        Err(error) => {
                            error!(
                                "Request to '{}' failed with: {}", uri, error
                            );
                            return result
                        }
                    }
                },
                Err(error) => {
                    error!(
                        "Error creating work directory {work_dir}: {error}"
                    );
                    return result
                }
            }

            // Verify the archive against the checksum record
            // fetched from the server
            info!("Verifying archive checksum...");
            if ! verify_checksum(&kis_tar, &source_checksum) {
                error!("Archive checksum verification failed");
                return result
            }

            // Unpack and Rename...
            info!("Unpacking...");
            let mut tar = Command::new("tar");
            tar.arg("-C").arg(&work_dir)
               .arg("-xf").arg(&kis_tar);
            match tar.status() {
                Ok(status) => {
                    result = status.code().unwrap();
                },
                Err(error) => {
                    error!("Failed to execute tar: {error:?}");
                    return result
                }
            }
            let mut kis_ok = 2;
            for path in fs::read_dir(&work_dir).unwrap() {
                let path = path.unwrap().path();
                let extension = path.extension().unwrap();
                if extension == OsStr::new("sha256") {
                    fs::remove_file(&path).unwrap();
                    // unused, the image is verified against the
                    // checksum record fetched from its origin
                } else if extension == OsStr::new("append") {
                    fs::remove_file(&path).unwrap();
                    // unused
                } else if extension == OsStr::new("initrd") {
                    fs::rename(&path, format!("{}/{}",
                        work_dir, defaults::FIRECRACKER_INITRD_NAME
                    )).unwrap();
                    // optional
                } else if extension == OsStr::new("kernel") {
                    fs::rename(&path, format!("{}/{}",
                        work_dir, defaults::FIRECRACKER_KERNEL_NAME
                    )).unwrap();
                    kis_ok -= 1;
                } else {
                    fs::rename(&path, format!("{}/{}",
                        work_dir, defaults::FIRECRACKER_ROOTFS_NAME
                    )).unwrap();
                    kis_ok -= 1;
                }
            }
            if kis_ok != 0 {
                error!("Not a KIWI kis type image");
                return result
            }

            // Keep the checksum of the origin with the image. It is
            // the reference for the update check of a later pull
            if ! write_source_checksum(&work_dir, &source_checksum) {
                return result
            }

            // Move to final firecracker image store. An outdated
            // image of the same name gets replaced
            if pull_state == PullState::Update
                && ! remove_image_dir(&image_dir)
            {
                return result
            }
            if ! mv(&work_dir, &image_dir, registry_user(usermode)) {
                return result
            }
        },
        Err(error) => {
            error!(
                "Failed to create tempdir in {}: {}", defaults::TEMP_DIR, error
            );
            return result
        }
    }
    result
}

async fn fetch_source_checksum(uri: &String) -> Option<String> {
    /*!
    Fetch the checksum record that belongs to the given image URI

    The record is expected at the same location as the image under
    the name of the image plus a '.sha256' suffix. An image which
    does not provide it cannot be verified and cannot take part in
    the update check and is therefore rejected
    !*/
    let checksum_uri = format!("{uri}.sha256");
    info!("Fetching checksum {checksum_uri}...");
    let response = match send_request(&checksum_uri).await {
        Ok(response) => response,
        Err(error) => {
            error!("Request to '{checksum_uri}' failed with: {error}");
            error!(
                "The image is expected to provide a checksum file named \
                like the image plus a '.sha256' suffix"
            );
            return None
        }
    };
    let checksum_record = match response.text().await {
        Ok(checksum_record) => checksum_record,
        Err(error) => {
            error!("Failed to read '{checksum_uri}': {error}");
            return None
        }
    };
    if checksum_value(&checksum_record).is_none() {
        error!("Checksum file '{checksum_uri}' provides no checksum");
        return None
    }
    Some(checksum_record)
}

pub fn image_is_current(image_dir: &str, source_checksum: &str) -> bool {
    /*!
    Check the image in the registry against the checksum of its origin

    An image without a stored record, e.g one that was pulled before
    the record was written, is never considered current. Such an image
    gets pulled again and by that receives the record needed for the
    next update check
    !*/
    let record_file = format!(
        "{}/{}", image_dir, defaults::FIRECRACKER_SOURCE_CHECKSUM_NAME
    );
    let stored_checksum = match fs::read_to_string(&record_file) {
        Ok(stored_checksum) => stored_checksum,
        Err(error) => {
            info!("No checksum record at '{record_file}': {error}");
            return false
        }
    };
    match (checksum_value(&stored_checksum), checksum_value(source_checksum)) {
        (Some(stored_sum), Some(source_sum)) => stored_sum == source_sum,
        _ => false
    }
}

pub fn write_source_checksum(image_dir: &str, source_checksum: &str) -> bool {
    /*!
    Store the checksum of the image origin along with the image data
    !*/
    let record_file = format!(
        "{}/{}", image_dir, defaults::FIRECRACKER_SOURCE_CHECKSUM_NAME
    );
    match fs::write(&record_file, source_checksum) {
        Ok(_) => true,
        Err(error) => {
            error!("Failed to write '{record_file}': {error}");
            false
        }
    }
}

fn checksum_value(checksum_record: &str) -> Option<&str> {
    /*!
    Provide the plain checksum of a checksum record

    Records come in the sha256sum format '<sum>  <file>' as well as
    in the kiwi format '<sum> <blocks> <blocksize>'. Only the
    checksum itself is of interest to compare two records
    !*/
    checksum_record.split_whitespace().next()
}

pub fn verify_checksum(image: &str, checksum_record: &str) -> bool {
    /*!
    Verify the given file against the sha256 checksum record
    fetched from the location the file was downloaded from

    A record in the sha256sum format has the layout:

        <sha256>  <file>

    A record created by kiwi has the layout:

        <sha256> <blocks> <blocksize>

    and covers only the first blocks * blocksize bytes of the file

    A record which cannot be read or a missing checksum program
    is reported but does not fail the operation. A checksum
    which does not match the image does
    !*/
    let mut record = checksum_record.split_whitespace();
    let expected_sum = match record.next() {
        Some(expected_sum) => expected_sum,
        None => {
            warn!("No checksum record found, skipping verification");
            return true
        }
    };
    // If the record provides a block count the checksum was
    // created over that amount of data and not over the
    // complete file
    let size = match (record.next(), record.next()) {
        (Some(blocks), Some(blocksize)) => {
            match (blocks.parse::<u64>(), blocksize.parse::<u64>()) {
                (Ok(blocks), Ok(blocksize)) => Some(blocks * blocksize),
                _ => None
            }
        },
        _ => None
    };
    match checksum(image, size) {
        Some(image_sum) => {
            if image_sum != expected_sum {
                error!(
                    "Checksum mismatch for {image}: \
                    expected {expected_sum}, got {image_sum}"
                );
                return false
            }
        },
        None => {
            warn!("Could not calculate checksum, skipping verification");
        }
    }
    true
}

fn checksum(image: &str, size: Option<u64>) -> Option<String> {
    /*!
    Calculate the sha256 sum of the first size bytes of image.
    If no size is given the complete file is read
    !*/
    let tool = defaults::SHA256_TOOL;
    let file = match File::open(image) {
        Ok(file) => file,
        Err(error) => {
            error!("Failed to open {image}: {error:?}");
            return None
        }
    };
    let mut reader: Box<dyn Read> = match size {
        Some(size) => Box::new(file.take(size)),
        None => Box::new(file)
    };
    let mut call = match Command::new(tool)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(call) => call,
        Err(error) => {
            error!("Failed to execute {tool}: {error:?}");
            return None
        }
    };

    {
        let mut stdin = call.stdin.take()?;
        if let Err(error) = io::copy(&mut reader, &mut stdin) {
            error!("Failed to send {image} to {tool}: {error:?}");
            // the pipe is closed by the drop of stdin below,
            // this lets the checksum tool terminate
        }
    }

    match call.wait_with_output() {
        Ok(output) => {
            if ! output.status.success() {
                error!("{tool} failed: {:?}", output.status);
                return None
            }
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace().next().map(ToOwned::to_owned)
        },
        Err(error) => {
            error!("Failed to read {tool} output: {error:?}");
            None
        }
    }
}

pub fn run_as(program: &str, user: &str) -> Command {
    /*!
    Create a call of the given program

    An empty user indicates that the program is called with the
    privileges of the caller. In all other cases the call is
    passed to sudo to run it as the specified user
    !*/
    if user.is_empty() {
        return Command::new(program)
    }
    let mut call = Command::new("sudo");
    call.arg("--user").arg(user).arg(program);
    call
}

pub fn export_volume(path: &str, usermode: bool) -> bool {
    /*!
    Export the given host path via NFS for firecracker guests

    The export is limited to the private network between the
    host and the VMs, which is the network of the host setup
    created by 'flake-ctl firecracker network init'
    !*/
    if ! validate_volume_export_path(path, true) {
        return false
    }
    let client_network = match network::get_client_network(usermode) {
        Some(client_network) => client_network,
        None => return false
    };
    if ! update_nfs_exports(path, Some(&client_network)) {
        return false
    }
    if nfs_server_is_running() {
        reload_nfs_exports()
    } else {
        start_nfs_server()
    }
}

pub fn release_volume(path: &str) -> bool {
    /*!
    Remove the given host path from the NFS exports
    !*/
    if ! validate_volume_export_path(path, false) {
        return false
    }
    if ! update_nfs_exports(path, None) {
        return false
    }
    restart_nfs_server()
}

fn validate_volume_export_path(path: &str, must_exist: bool) -> bool {
    /*!
    Validate the path used for an NFS volume export operation
    !*/
    if ! path.starts_with('/') {
        error!("Path {path:?} must be specified with an absolute path");
        return false
    }
    if path.contains('\n') || path.contains('\r') {
        error!("Path {path:?} contains unsupported control characters");
        return false
    }
    if must_exist {
        let volume_path = Path::new(path);
        if ! volume_path.exists() {
            error!("Volume path {path:?} does not exist");
            return false
        }
        if ! volume_path.is_dir() {
            error!("Volume path {path:?} is not a directory");
            return false
        }
    }
    true
}

fn update_nfs_exports(path: &str, client_network: Option<&str>) -> bool {
    /*!
    Add or remove the flake-pilot managed NFS export entry

    The entry is created for the given client network. Without
    a network the entry is deleted
    !*/
    let exports = match read_nfs_exports() {
        Some(exports) => exports,
        None => return false
    };
    let desired_entry = client_network.map(
        |client_network| nfs_export_entry(path, client_network)
    );
    let managed_entries: Vec<&str> = exports.lines().filter(
        |line| is_flake_pilot_nfs_export(line, path)
    ).collect();
    if let Some(ref desired_entry) = desired_entry {
        if managed_entries.len() == 1
            && managed_entries[0].trim() == *desired_entry
        {
            info!("Keeping existing NFS export for {path}");
            return true
        }
    } else if managed_entries.is_empty() {
        info!("No flake-pilot NFS export entry found for {path}");
        return true
    }

    let mut updated_lines: Vec<String> = exports.lines().filter(
        |line| ! is_flake_pilot_nfs_export(line, path)
    ).map(ToOwned::to_owned).collect();
    match desired_entry {
        Some(desired_entry) => {
            info!("Exporting {path} through NFS...");
            updated_lines.push(desired_entry);
        },
        None => info!("Releasing NFS export for {path}...")
    }

    let mut updated_exports = updated_lines.join("\n");
    if ! updated_exports.is_empty() {
        updated_exports.push('\n');
    }
    write_nfs_exports(&updated_exports)
}

fn read_nfs_exports() -> Option<String> {
    /*!
    Read the /etc/exports file
    !*/
    match fs::read_to_string(defaults::NFS_EXPORTS_FILE) {
        Ok(exports) => Some(exports),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Some(String::new()),
        Err(error) => {
            error!(
                "Failed to read {}: {error}",
                defaults::NFS_EXPORTS_FILE
            );
            None
        }
    }
}

fn write_nfs_exports(exports: &str) -> bool {
    /*!
    Write the /etc/exports file through a temporary copy
    !*/
    let mut temp = match NamedTempFile::new_in(defaults::TEMP_DIR) {
        Ok(temp) => temp,
        Err(error) => {
            error!(
                "Failed to create temp file in {}: {error}",
                defaults::TEMP_DIR
            );
            return false
        }
    };
    if let Err(error) = temp.write_all(exports.as_bytes()) {
        error!("Failed to write temp exports file: {error}");
        return false
    }
    if let Err(error) = temp.flush() {
        error!("Failed to flush temp exports file: {error}");
        return false
    }
    let mut call = run_as("install", "root");
    call.arg("-m")
        .arg("644")
        .arg(temp.path())
        .arg(defaults::NFS_EXPORTS_FILE);
    run_ok(&mut call, &format!("install {}", defaults::NFS_EXPORTS_FILE))
}

fn nfs_export_entry(path: &str, client_network: &str) -> String {
    /*!
    Provide the managed NFS export line for the given path
    !*/
    format!(
        "{} {}({}) {}",
        escape_nfs_export_path(path),
        client_network,
        defaults::NFS_EXPORT_OPTIONS,
        FLAKE_PILOT_NFS_EXPORT_MARKER
    )
}

fn escape_nfs_export_path(path: &str) -> String {
    /*!
    Escape whitespace in the path for /etc/exports
    !*/
    path.replace('\\', "\\\\")
        .replace(' ', "\\040")
        .replace('\t', "\\011")
}

fn is_flake_pilot_nfs_export(line: &str, path: &str) -> bool {
    /*!
    Check if the line is the managed flake-pilot export for path
    !*/
    let trimmed = line.trim();
    trimmed.starts_with(&format!("{} ", escape_nfs_export_path(path)))
        && trimmed.ends_with(FLAKE_PILOT_NFS_EXPORT_MARKER)
}

fn nfs_server_is_running() -> bool {
    /*!
    Check if the NFS server systemd service is active
    !*/
    let mut call = run_as("systemctl", "root");
    call.arg("is-active")
        .arg("--quiet")
        .arg(defaults::NFS_SERVER_SERVICE);
    match call.status() {
        Ok(status) => status.success(),
        Err(error) => {
            error!(
                "Failed to query {}: {error:?}",
                defaults::NFS_SERVER_SERVICE
            );
            false
        }
    }
}

fn start_nfs_server() -> bool {
    /*!
    Start the NFS server service
    !*/
    info!("Starting {}...", defaults::NFS_SERVER_SERVICE);
    let mut call = run_as("systemctl", "root");
    call.arg("start").arg(defaults::NFS_SERVER_SERVICE);
    run_ok(&mut call, &format!("start {}", defaults::NFS_SERVER_SERVICE))
}

fn restart_nfs_server() -> bool {
    /*!
    Restart the NFS server service
    !*/
    info!("Restarting {}...", defaults::NFS_SERVER_SERVICE);
    let mut call = run_as("systemctl", "root");
    call.arg("restart").arg(defaults::NFS_SERVER_SERVICE);
    run_ok(&mut call, &format!("restart {}", defaults::NFS_SERVER_SERVICE))
}

fn reload_nfs_exports() -> bool {
    /*!
    Reload the NFS exports of the running server
    !*/
    info!("Reloading NFS exports...");
    let mut call = run_as(defaults::NFS_EXPORTFS_TOOL, "root");
    call.arg("-ra");
    run_ok(&mut call, "reload NFS exports")
}

fn run_ok(call: &mut Command, action: &str) -> bool {
    /*!
    Run the given call and tell whether it succeeded
    !*/
    match call.status() {
        Ok(status) => {
            if ! status.success() {
                error!("Failed to {action}: {status}");
                return false
            }
            true
        },
        Err(error) => {
            error!("Failed to {action}: {error:?}");
            false
        }
    }
}

pub fn mkdir(dirname: &String, user: &str) -> bool {
    /*!
    Make directory
    !*/
    let mut call = run_as("mkdir", user);
    call.arg("-p").arg(dirname);
    match call.status() {
        Ok(_) => { },
        Err(error) => {
            error!("Failed to mkdir: {dirname}: {error:?}");
            return false
        }
    }
    true
}

pub fn mv(source: &str, target: &String, user: &str) -> bool {
    /*!
    Move file
    !*/
    let mut call = run_as("mv", user);
    call.arg(source).arg(target);
    match call.status() {
        Ok(_) => { },
        Err(error) => {
            error!("Failed to mv: {source} -> {target}: {error:?}");
            return false
        }
    }
    true
}

pub fn copy(source: &str, target: &String, user: &str) -> bool {
    /*!
    Copy file
    !*/
    let mut call = run_as("cp", user);
    call.arg(source).arg(target);
    match call.status() {
        Ok(_) => { },
        Err(error) => {
            error!("Failed to cp: {source} -> {target}: {error:?}");
            return false
        }
    }
    true
}

pub fn mount_fs_image(
    fs_name: &str, mount_point: &String, user: &str
) -> bool {
    /*!
    Mount filesystem image
    !*/
    let mut call = run_as("mount", user);
    call.arg(fs_name).arg(mount_point);
    match call.status() {
        Ok(_) => { },
        Err(error) => {
            error!("Failed to execute mount: {error:?}");
            return false
        }
    }
    true
}

pub fn umount(mount_point: &str, user: &str) -> bool {
    /*!
    Umount given mount_point
    !*/
    let mut call = run_as("umount", user);
    call.arg(mount_point);
    match call.status() {
        Ok(_) => { },
        Err(error) => {
            error!("Failed to execute mount: {error:?}");
            return false
        }
    }
    true
}


#[derive(Debug, PartialEq)]
pub enum PullState {
    /// No image of that name in the registry, fetch and register it
    New,
    /// An image of that name is in the registry. Only fetch and
    /// register it again if its origin has changed
    Update
}

pub fn pull_init(name: &str, force: bool, usermode: bool) -> Option<PullState> {
    /*!
    Initialize pull and tell whether it registers a new image or
    updates an existing one

    With force an existing image is deleted upfront which turns
    the pull into a pull from scratch. None is returned if the
    registry could not be prepared for the pull
    !*/
    if ! init_toplevel_image_dir(&get_registry_dir(usermode)) {
        return None
    }
    let image_dir = get_image_dir(name, usermode);
    if force && Path::new(&image_dir).exists() {
        if ! remove_image_dir(&image_dir) {
            return None
        }
        return Some(PullState::New)
    }
    if Path::new(&image_dir).exists() {
        return Some(PullState::Update)
    }
    Some(PullState::New)
}

pub fn pull_new(name: &str, force: bool, usermode: bool) -> bool {
    /*!
    Initialize new pull

    Used for pulls that provide no way to tell an existing image
    apart from its origin. For those an already existing image
    is an error
    !*/
    match pull_init(name, force, usermode) {
        Some(PullState::New) => true,
        Some(PullState::Update) => {
            error!(
                "Image directory '{}' already exists",
                get_image_dir(name, usermode)
            );
            false
        },
        None => false
    }
}

pub fn remove_image_dir(image_dir: &str) -> bool {
    /*!
    Delete the given image directory from the registry
    !*/
    match fs::remove_dir_all(image_dir) {
        Ok(_) => true,
        Err(error) => {
            error!("Error removing directory {image_dir}: {error}");
            false
        }
    }
}

pub fn purge_vm(vm: &str, usermode: bool) {
    /*!
    Iterate over all yaml config files and find those connected
    to the VM. Delete all app registrations for this
    VM and also delete the VM from the local registry
    !*/
    for app_name in app::app_names(usermode) {
        let config_file = format!(
            "{}/{}.yaml", get_flakes_dir(usermode), app_name
        );
        match app_config::AppConfig::init_from_file(Path::new(&config_file)) {
            Ok(app_conf) => {
                if let Some(ref vm_conf) = app_conf.vm {
                    if vm == vm_conf.name {
                        app::remove(
                            &vm_conf.host_app_path,
                            defaults::FIRECRACKER_PILOT, usermode, false, false
                        );
                    }
                }
            },
            Err(error) => {
                error!(
                    "Ignoring error on load or parse flake config {config_file}: {error:?}"
                );
            }
        };
    }
    let image_dir = get_image_dir(vm, usermode);
    match fs::remove_dir_all(&image_dir) {
        Ok(_) => { },
        Err(error) => {
            error!("Error removing directory {image_dir}: {error}");
        }
    }
}
