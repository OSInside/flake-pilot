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
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::net::Ipv4Addr;
use uzers::get_current_uid;
use std::process::{Command, Stdio};
use tempfile::{Builder, NamedTempFile, TempDir};
use std::path::Path;
use std::borrow::Cow;
use std::fs;
use std::fs::File;

use crate::defaults;
use crate::network;
use crate::app;
use crate::instance;

use crate::fetch::{fetch_file, send_request};

const FLAKE_PILOT_NFS_EXPORT_MARKER: &str =
    "# flake-pilot firecracker volume";

// The name of an image is a directory name in the registry
const IMAGE_NAME_MAX_LEN: usize = 255;

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

pub fn get_image_dir(name: &str, usermode: bool) -> io::Result<String> {
    /*!
    Provide the directory of the image registered as name

    The images directory has to exist, see init_toplevel_image_dir
    !*/
    image_dir_below(&get_images_dir(usermode), name)
}

pub fn validate_image_name(name: &str) -> io::Result<()> {
    /*!
    Check the name of an image in the firecracker registry

    The name is used as a directory name in the registry. It has
    to start with an alphanumeric character and may only contain
    alphanumeric characters, '.', '_' and '-'. This rules out
    path separators as well as '.' and '..'
    !*/
    let mut chars = name.chars();
    let valid = name.len() <= IMAGE_NAME_MAX_LEN
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if ! valid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Invalid image name '{name}': expected up to \
                {IMAGE_NAME_MAX_LEN} characters out of [A-Za-z0-9._-], \
                starting with [A-Za-z0-9]"
            )
        ))
    }
    Ok(())
}

fn image_dir_below(images_dir: &str, name: &str) -> io::Result<String> {
    /*!
    Provide the directory of the image registered as name below
    the given images directory

    The directory is joined to the real path of the images
    directory. An existing entry of that name has to resolve to
    itself, a symlink could lead out of the registry. The result
    is passed to remove_dir_all and mv, partly as root, and
    therefore must not point anywhere else
    !*/
    validate_image_name(name)?;
    let real_images_dir = fs::canonicalize(images_dir).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("Failed to resolve images directory {images_dir}: {error}")
        )
    })?;
    let image_dir = real_images_dir.join(name);
    match fs::symlink_metadata(&image_dir) {
        Ok(_) => {
            if fs::canonicalize(&image_dir).ok().as_ref() != Some(&image_dir) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "Image directory {} is a symlink, refusing to use it",
                        image_dir.display()
                    )
                ))
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => { },
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("Failed to access {}: {error}", image_dir.display())
            ))
        }
    }
    image_dir.into_os_string().into_string().map_err(|image_dir| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Image directory {image_dir:?} is not valid UTF-8")
        )
    })
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
    struct Component<'a> {
        uri: String,
        file: Cow<'a, str>
    }
    info!("Fetching Component image...");
    let image_dir = match pull_new(name, force, usermode) {
        Some(image_dir) => image_dir,
        None => return result
    };
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

            // Move to final firecracker image store
            let tmp_dir_path = tmp_dir.path().display().to_string();
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
    let uri = uri.unwrap();

    info!("Fetching KIS image...");

    let (pull_state, image_dir) = match pull_init(name, force, usermode) {
        Some(pull) => pull,
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

    The program has to be given by its absolute path, see the
    tool definitions in defaults
    !*/
    debug_assert!(
        Path::new(program).is_absolute(),
        "sudo target {} is not an absolute path", program
    );
    if user.is_empty() {
        return Command::new(program)
    }
    let mut call = Command::new("sudo");
    call.arg("--user").arg(user).arg(program);
    call
}

pub fn export_volume(
    path: &str, app: &str, instance: Option<&String>, usermode: bool
) -> bool {
    /*!
    Export the given host path via NFS to the VM of a flake

    The export is limited to the VM the given flake instance runs
    in, which is identified by its address in the private network
    between the host and the VMs. Exporting the same path for
    further flakes or instances adds their VMs to the export.

    Only a path owned by a user can be exported. Every access of
    a VM is mapped to that user, no matter which user, including
    root, performs it inside of the VM. A VM therefore gets the
    permissions of the owner of the path on it and nothing more
    !*/
    if ! validate_volume_export_path(path, true) {
        return false
    }
    let owner = match get_volume_owner(path) {
        Some(owner) => owner,
        None => return false
    };
    let guest = match network::get_guest_address(app, instance, usermode) {
        Some(guest) => guest,
        None => return false
    };
    let update = NfsExportUpdate::Add {
        guest, options: nfs_export_options(&owner)
    };
    if ! update_nfs_exports(path, update) {
        return false
    }
    if nfs_server_is_running() {
        reload_nfs_exports()
    } else {
        start_nfs_server()
    }
}

pub fn release_volume(
    path: &str, app: Option<&String>, instance: Option<&String>,
    usermode: bool
) -> bool {
    /*!
    Remove the given host path from the NFS exports

    Called with an application only the VM of that flake instance
    is removed from the export. The export itself is deleted once
    no VM is left. Without an application the export is deleted
    !*/
    if ! validate_volume_export_path(path, false) {
        return false
    }
    let update = match app {
        Some(app) => match network::get_guest_address(
            app, instance, usermode
        ) {
            Some(guest) => NfsExportUpdate::RemoveGuest(guest),
            None => {
                error!(
                    "To release the export for all VMs call \
                    'flake-ctl firecracker volume release --path {path}'"
                );
                return false
            }
        },
        None => NfsExportUpdate::RemoveAll
    };
    if ! update_nfs_exports(path, update) {
        return false
    }
    restart_nfs_server()
}

// VolumeOwner is the user every access of a VM to an exported
// volume is mapped to
struct VolumeOwner {
    uid: u32,
    gid: u32
}

fn get_volume_owner(path: &str) -> Option<VolumeOwner> {
    /*!
    Provide the owner of the given volume path

    The path is required to be owned by the calling user. Called
    by root, a path owned by any user other than root is accepted.
    A path owned by root is refused, the VMs would get the
    permissions of root on it. The path has to be given the way
    the NFS server resolves it, a path which leads through a
    symbolic link is refused
    !*/
    let resolved_path = match fs::canonicalize(path) {
        Ok(resolved_path) => resolved_path,
        Err(error) => {
            error!("Failed to resolve volume path {path:?}: {error}");
            return None
        }
    };
    if resolved_path != Path::new(path) {
        error!("Volume path {path:?} leads through a symbolic link");
        error!("Please use the resolved path {}", resolved_path.display());
        return None
    }
    let metadata = match fs::metadata(&resolved_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            error!("Failed to read volume path {path:?}: {error}");
            return None
        }
    };
    let owner = VolumeOwner { uid: metadata.uid(), gid: metadata.gid() };
    if owner.uid == 0 {
        error!("Volume path {path:?} is owned by root");
        error!("Only a path owned by a user other than root can be exported");
        return None
    }
    let calling_uid = get_current_uid();
    if calling_uid != 0 && owner.uid != calling_uid {
        error!("Volume path {path:?} is not owned by the calling user");
        return None
    }
    Some(owner)
}

fn nfs_export_options(owner: &VolumeOwner) -> String {
    /*!
    Provide the NFS export options for a volume of the given owner
    !*/
    format!(
        "{},anonuid={},anongid={}",
        defaults::NFS_EXPORT_OPTIONS, owner.uid, owner.gid
    )
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

// NfsExportUpdate is a change of the managed NFS export of a path
enum NfsExportUpdate {
    /// Allow the VM of the given address with the given options
    Add { guest: Ipv4Addr, options: String },
    /// Remove the VM of the given address
    RemoveGuest(Ipv4Addr),
    /// Delete the export
    RemoveAll
}

// NfsExportClient is a VM the managed NFS export of a path is
// allowed for
#[derive(Debug, PartialEq)]
struct NfsExportClient {
    guest: Ipv4Addr,
    options: String
}

fn update_nfs_exports(path: &str, update: NfsExportUpdate) -> bool {
    /*!
    Change the flake-pilot managed NFS export entry of the path
    !*/
    let exports = match read_nfs_exports() {
        Some(exports) => exports,
        None => return false
    };
    match get_updated_nfs_exports(&exports, path, update) {
        Some(updated_exports) => write_nfs_exports(&updated_exports),
        None => true
    }
}

fn get_updated_nfs_exports(
    exports: &str, path: &str, update: NfsExportUpdate
) -> Option<String> {
    /*!
    Provide the content of the exports file with the given change
    of the managed NFS export entry of the path applied. None is
    returned if the content stays the same

    The entry lists every VM it is allowed for as a client of its
    own. A client which is not the address of a single VM, as
    written by former versions for the whole private network, is
    dropped
    !*/
    let managed_entries: Vec<&str> = exports.lines().filter(
        |line| is_flake_pilot_nfs_export(line, path)
    ).collect();
    let mut clients = get_nfs_export_clients(&managed_entries, path);
    match update {
        NfsExportUpdate::Add { guest, options } => {
            clients.retain(|client| client.guest != guest);
            clients.push(NfsExportClient { guest, options: options.clone() });
            // All VMs access the volume as its owner, an owner
            // which has changed applies to all of them
            for client in clients.iter_mut() {
                client.options = options.clone()
            }
        },
        NfsExportUpdate::RemoveGuest(guest) => {
            if ! clients.iter().any(|client| client.guest == guest) {
                info!("{guest} is not a client of the NFS export for {path}");
            }
            clients.retain(|client| client.guest != guest)
        },
        NfsExportUpdate::RemoveAll => clients.clear()
    }
    let desired_entry = if clients.is_empty() {
        None
    } else {
        Some(nfs_export_entry(path, &clients))
    };
    match desired_entry {
        Some(ref desired_entry) if managed_entries.len() == 1
            && managed_entries[0].trim() == *desired_entry =>
        {
            info!("Keeping existing NFS export for {path}");
            return None
        },
        None if managed_entries.is_empty() => {
            info!("No flake-pilot NFS export entry found for {path}");
            return None
        },
        _ => { }
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
    Some(updated_exports)
}

fn get_nfs_export_clients(
    managed_entries: &[&str], path: &str
) -> Vec<NfsExportClient> {
    /*!
    Read the VMs the given managed NFS export entries allow
    !*/
    let mut clients: Vec<NfsExportClient> = Vec::new();
    for entry in managed_entries {
        let entry = entry.trim()
            .trim_end_matches(FLAKE_PILOT_NFS_EXPORT_MARKER);
        for client_spec in entry.split_whitespace().skip(1) {
            let (client, options) = client_spec.split_once('(')
                .unwrap_or((client_spec, ""));
            match client.parse::<Ipv4Addr>() {
                Ok(guest) if ! clients.iter().any(
                    |known| known.guest == guest
                ) => clients.push(NfsExportClient {
                    guest, options: options.trim_end_matches(')').to_string()
                }),
                Ok(_) => { },
                Err(_) => warn!(
                    "Dropping NFS export of {path} for {client}, \
                    only the address of a single VM is allowed"
                )
            }
        }
    }
    clients
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
    let mut call = run_as(defaults::INSTALL_TOOL, "root");
    call.arg("-m")
        .arg("644")
        .arg(temp.path())
        .arg(defaults::NFS_EXPORTS_FILE);
    run_ok(&mut call, &format!("install {}", defaults::NFS_EXPORTS_FILE))
}

fn nfs_export_entry(path: &str, clients: &[NfsExportClient]) -> String {
    /*!
    Provide the managed NFS export line for the given path
    !*/
    let clients: Vec<String> = clients.iter().map(
        |client| format!("{}({})", client.guest, client.options)
    ).collect();
    format!(
        "{} {} {}",
        escape_nfs_export_path(path),
        clients.join(" "),
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
    let mut call = run_as(defaults::SYSTEMCTL_TOOL, "root");
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
    let mut call = run_as(defaults::SYSTEMCTL_TOOL, "root");
    call.arg("start").arg(defaults::NFS_SERVER_SERVICE);
    run_ok(&mut call, &format!("start {}", defaults::NFS_SERVER_SERVICE))
}

fn restart_nfs_server() -> bool {
    /*!
    Restart the NFS server service
    !*/
    info!("Restarting {}...", defaults::NFS_SERVER_SERVICE);
    let mut call = run_as(defaults::SYSTEMCTL_TOOL, "root");
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

pub fn mv(source: &str, target: &String, user: &str) -> bool {
    /*!
    Move file

    The target is always treated as the new name of the source,
    never as a directory to move the source into. A directory
    which appeared at the target in the meantime therefore fails
    the move instead of receiving the source. Only an empty
    directory gets replaced
    !*/
    let mut call = run_as(defaults::MV_TOOL, user);
    call.arg("--no-target-directory").arg(source).arg(target);
    run_ok(&mut call, &format!("mv: {source} -> {target}"))
}

#[derive(Debug, PartialEq)]
pub enum PullState {
    /// No image of that name in the registry, fetch and register it
    New,
    /// An image of that name is in the registry. Only fetch and
    /// register it again if its origin has changed
    Update
}

pub fn pull_init(
    name: &str, force: bool, usermode: bool
) -> Option<(PullState, String)> {
    /*!
    Initialize pull and tell whether it registers a new image or
    updates an existing one, along with the image directory

    With force an existing image is deleted upfront which turns
    the pull into a pull from scratch. None is returned if the
    name is invalid or the registry could not be prepared for
    the pull
    !*/
    if let Err(error) = validate_image_name(name) {
        error!("{error}");
        return None
    }
    if ! init_toplevel_image_dir(&get_registry_dir(usermode)) {
        return None
    }
    let image_dir = match get_image_dir(name, usermode) {
        Ok(image_dir) => image_dir,
        Err(error) => {
            error!("{error}");
            return None
        }
    };
    if force && Path::new(&image_dir).exists() {
        if ! remove_image_dir(&image_dir) {
            return None
        }
        return Some((PullState::New, image_dir))
    }
    if Path::new(&image_dir).exists() {
        return Some((PullState::Update, image_dir))
    }
    Some((PullState::New, image_dir))
}

pub fn pull_new(name: &str, force: bool, usermode: bool) -> Option<String> {
    /*!
    Initialize new pull and provide the image directory

    Used for pulls that provide no way to tell an existing image
    apart from its origin. For those an already existing image
    is an error
    !*/
    match pull_init(name, force, usermode) {
        Some((PullState::New, image_dir)) => Some(image_dir),
        Some((PullState::Update, image_dir)) => {
            error!("Image directory '{image_dir}' already exists");
            None
        },
        None => None
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

pub fn purge_vm(vm: &str, usermode: bool) -> bool {
    /*!
    Iterate over all yaml config files and find those connected
    to the VM. Delete all app registrations for this VM along
    with the meta data of their instances and also delete the
    VM from the local registry
    !*/
    if let Err(error) = validate_image_name(vm) {
        error!("{error}");
        return false
    }
    for registration in app::image_flakes(
        vm, defaults::FIRECRACKER_ENGINE, usermode
    ) {
        if app::remove(
            &registration.host_app_path,
            defaults::FIRECRACKER_PILOT, usermode, false, false
        ) {
            instance::remove_vm_meta_data(&registration.name, usermode);
        }
    }
    match get_image_dir(vm, usermode) {
        Ok(image_dir) => remove_image_dir(&image_dir),
        Err(error) => {
            error!("{error}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    const OPTIONS: &str =
        "rw,sync,no_subtree_check,all_squash,anonuid=1000,anongid=100";

    fn add(guest: &str) -> NfsExportUpdate {
        NfsExportUpdate::Add {
            guest: guest.parse().unwrap(), options: OPTIONS.to_string()
        }
    }

    fn entry(path: &str, guests: &[&str]) -> String {
        let clients: Vec<String> = guests.iter().map(
            |guest| format!("{guest}({OPTIONS})")
        ).collect();
        format!(
            "{path} {} {FLAKE_PILOT_NFS_EXPORT_MARKER}", clients.join(" ")
        )
    }

    #[test]
    fn test_nfs_export_options() {
        assert_eq!(
            nfs_export_options(&VolumeOwner { uid: 1000, gid: 100 }),
            OPTIONS
        );
        assert!(! defaults::NFS_EXPORT_OPTIONS.contains("no_root_squash"));
        assert!(! defaults::NFS_EXPORT_OPTIONS.contains("insecure"));
    }

    #[test]
    fn test_export_is_created_per_guest() {
        let exports = "/other host(ro)\n";
        let updated = get_updated_nfs_exports(
            exports, "/srv/vol", add("172.16.0.2")
        ).unwrap();
        assert_eq!(
            updated,
            format!("/other host(ro)\n{}\n", entry("/srv/vol", &["172.16.0.2"]))
        );
        // A further VM is added to the same entry
        let updated = get_updated_nfs_exports(
            &updated, "/srv/vol", add("172.16.0.3")
        ).unwrap();
        assert_eq!(
            updated,
            format!(
                "/other host(ro)\n{}\n",
                entry("/srv/vol", &["172.16.0.2", "172.16.0.3"])
            )
        );
        // Exporting again for a known VM changes nothing
        assert_eq!(
            get_updated_nfs_exports(&updated, "/srv/vol", add("172.16.0.3")),
            None
        );
    }

    #[test]
    fn test_export_replaces_network_wide_entry() {
        let exports = format!(
            "/srv/vol 172.16.0.0/24(rw,sync,no_subtree_check,\
            no_root_squash,insecure) {FLAKE_PILOT_NFS_EXPORT_MARKER}\n\
            /srv/vol 10.0.0.1(ro)\n"
        );
        let updated = get_updated_nfs_exports(
            &exports, "/srv/vol", add("172.16.0.2")
        ).unwrap();
        // The export not managed by flake-pilot is kept as it is
        assert_eq!(
            updated,
            format!(
                "/srv/vol 10.0.0.1(ro)\n{}\n",
                entry("/srv/vol", &["172.16.0.2"])
            )
        );
        assert!(! updated.contains("no_root_squash"));
        assert!(! updated.contains("insecure"));
    }

    #[test]
    fn test_export_release() {
        let exports = format!(
            "{}\n", entry("/srv/vol", &["172.16.0.2", "172.16.0.3"])
        );
        let updated = get_updated_nfs_exports(
            &exports, "/srv/vol",
            NfsExportUpdate::RemoveGuest("172.16.0.2".parse().unwrap())
        ).unwrap();
        assert_eq!(
            updated, format!("{}\n", entry("/srv/vol", &["172.16.0.3"]))
        );
        // The entry is deleted with its last VM
        assert_eq!(
            get_updated_nfs_exports(
                &updated, "/srv/vol",
                NfsExportUpdate::RemoveGuest("172.16.0.3".parse().unwrap())
            ),
            Some(String::new())
        );
        assert_eq!(
            get_updated_nfs_exports(
                &exports, "/srv/vol", NfsExportUpdate::RemoveAll
            ),
            Some(String::new())
        );
        // Nothing to release
        assert_eq!(
            get_updated_nfs_exports(
                "/srv/vol 10.0.0.1(ro)\n", "/srv/vol",
                NfsExportUpdate::RemoveAll
            ),
            None
        );
    }

    #[test]
    fn test_validate_image_name() {
        for name in ["claude", "leap-15.6", "my_VM.1", "0"] {
            assert!(validate_image_name(name).is_ok(), "{}", name);
        }
        let too_long = "a".repeat(IMAGE_NAME_MAX_LEN + 1);
        for name in [
            "", ".", "..", "../etc", "a/b", "/abs", ".hidden", "-opt",
            "a b", "a*", "näme", "a\n", too_long.as_str()
        ] {
            assert!(validate_image_name(name).is_err(), "{:?}", name);
        }
    }

    #[test]
    fn test_image_dir_below() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let images_dir = tmp_dir.path().join("images");
        fs::create_dir(&images_dir).unwrap();
        let real_images_dir = fs::canonicalize(&images_dir).unwrap();
        let images_dir = images_dir.to_str().unwrap();

        // a new and an existing image resolve below the real path
        // of the images directory
        assert_eq!(
            image_dir_below(images_dir, "new").unwrap(),
            real_images_dir.join("new").to_str().unwrap()
        );
        fs::create_dir(real_images_dir.join("present")).unwrap();
        assert_eq!(
            image_dir_below(images_dir, "present").unwrap(),
            real_images_dir.join("present").to_str().unwrap()
        );

        // names which leave the registry are refused
        assert!(image_dir_below(images_dir, "../outside").is_err());
        assert!(image_dir_below(images_dir, "..").is_err());

        // a symlink is refused wherever it points to, dangling
        // or not
        symlink(tmp_dir.path(), real_images_dir.join("escape")).unwrap();
        assert!(image_dir_below(images_dir, "escape").is_err());
        symlink("/nonexistent", real_images_dir.join("dangling")).unwrap();
        assert!(image_dir_below(images_dir, "dangling").is_err());

        // the images directory has to exist
        let missing = tmp_dir.path().join("missing");
        assert!(image_dir_below(missing.to_str().unwrap(), "new").is_err());
    }

    #[test]
    fn test_mv_never_moves_into_target() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let source = tmp_dir.path().join("source");
        let target = tmp_dir.path().join("target");
        let new_source = || {
            fs::create_dir(&source).unwrap();
            fs::write(source.join("rootfs"), "data").unwrap();
        };
        let source_str = source.to_str().unwrap();
        let target_str = target.to_str().unwrap().to_string();

        // a target which does not exist is created
        new_source();
        assert!(mv(source_str, &target_str, ""));
        assert!(target.join("rootfs").is_file());

        // a target directory which is not empty fails the move,
        // the source is not placed inside of it
        new_source();
        assert!(! mv(source_str, &target_str, ""));
        assert!(source.join("rootfs").is_file());
        assert!(! target.join("source").exists());

        // an empty target directory is replaced
        fs::remove_dir_all(&target).unwrap();
        fs::create_dir(&target).unwrap();
        assert!(mv(source_str, &target_str, ""));
        assert!(target.join("rootfs").is_file());
        assert!(! target.join("source").exists());
    }

    #[test]
    fn test_get_volume_owner() {
        // A path owned by root is never exported
        assert!(get_volume_owner("/").is_none());

        let tmp_dir = tempfile::tempdir().unwrap();
        let volume = tmp_dir.path().join("volume");
        fs::create_dir(&volume).unwrap();
        let link = tmp_dir.path().join("link");
        symlink(&volume, &link).unwrap();

        // A path leading through a symbolic link is refused
        assert!(get_volume_owner(link.to_str().unwrap()).is_none());

        // The path is owned by the calling user, which is only
        // accepted if that user is not root
        let resolved = fs::canonicalize(&volume).unwrap();
        assert_eq!(
            get_volume_owner(resolved.to_str().unwrap()).is_some(),
            get_current_uid() != 0
        );
    }
}
