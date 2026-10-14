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
use flakes::helper::helper_call;
use std::ffi::OsStr;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use tempfile::{Builder, TempDir};
use std::path::Path;
use std::borrow::Cow;
use std::fs;
use std::fs::File;

use crate::defaults;
use crate::network;
use crate::app;
use crate::instance;

use crate::fetch::{fetch_file, send_request};

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
            if ! mv(&tmp_dir_path, &image_dir) {
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
            if ! mv(&work_dir, &image_dir) {
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

pub fn export_volume(
    path: &str, app: &str, instance: Option<&String>, usermode: bool
) -> bool {
    /*!
    Export the given host path via NFS to the VM of a flake

    The export is limited to the VM the given flake instance runs
    in, it is created by the helper, see its volume-export command.
    Only a path owned by the caller can be exported, root can
    export the path of any user other than root. Every access of
    the VM is mapped to the owner of the path
    !*/
    let config_file = match network::get_flake_config_file(app, usermode) {
        Some(config_file) => config_file,
        None => return false
    };
    let mut call = helper_call("volume-export");
    call.arg("--path").arg(path).arg("--config-file").arg(&config_file);
    if let Some(instance) = instance {
        call.arg("--instance").arg(instance);
    }
    run_ok(&mut call, &format!("export {path}"))
}

pub fn release_volume(
    path: &str, app: Option<&String>, instance: Option<&String>,
    usermode: bool
) -> bool {
    /*!
    Remove the given host path from the NFS exports

    Called with an application only the VM of that flake instance
    is removed from the export. The export itself is deleted once
    no VM is left. Without an application the export is deleted.
    The export is released by the helper, see its volume-release
    command
    !*/
    let mut call = helper_call("volume-release");
    call.arg("--path").arg(path);
    if let Some(app) = app {
        let config_file = match network::get_flake_config_file(app, usermode) {
            Some(config_file) => config_file,
            None => return false
        };
        call.arg("--config-file").arg(&config_file);
        if let Some(instance) = instance {
            call.arg("--instance").arg(instance);
        }
    }
    if ! run_ok(&mut call, &format!("release {path}")) {
        if app.is_some() {
            error!(
                "To release the export for all VMs call \
                'flake-ctl firecracker volume release --path {path}'"
            );
        }
        return false
    }
    true
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

pub fn mv(source: &str, target: &String) -> bool {
    /*!
    Move file

    The target is always treated as the new name of the source,
    never as a directory to move the source into. A directory
    which appeared at the target in the meantime therefore fails
    the move instead of receiving the source. Only an empty
    directory gets replaced

    The registry belongs to the caller: a user operates on its own
    registry, the system wide registry is only operated on by root
    !*/
    let mut call = Command::new(defaults::MV_TOOL);
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
        assert!(mv(source_str, &target_str));
        assert!(target.join("rootfs").is_file());

        // a target directory which is not empty fails the move,
        // the source is not placed inside of it
        new_source();
        assert!(! mv(source_str, &target_str));
        assert!(source.join("rootfs").is_file());
        assert!(! target.join("source").exists());

        // an empty target directory is replaced
        fs::remove_dir_all(&target).unwrap();
        fs::create_dir(&target).unwrap();
        assert!(mv(source_str, &target_str));
        assert!(target.join("rootfs").is_file());
        assert!(! target.join("source").exists());
    }

}
