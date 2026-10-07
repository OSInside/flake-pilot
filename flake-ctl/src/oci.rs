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
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{lchown, MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use serde_json::Value;
use uzers::get_effective_uid;
use crate::defaults;

// Mode, owner and group of a directory provided by an image layer
type DirSetup = (u32, u32, u32);

pub fn unpack(oci: &str, directory: &str) -> bool {
    /*!
    Unpack the file system of the image in the given OCI tarball
    into directory

    The tarball is read without the use of the podman registry.
    Both the OCI image layout and the docker archive layout as
    written by 'podman save' are supported. The layers of the
    image are applied in order, including the OCI whiteout
    entries which delete data of the layers below
    !*/
    match unpack_archive(oci, Path::new(directory)) {
        Ok(()) => true,
        Err(error) => {
            error!("Failed to unpack {oci}: {error}");
            false
        }
    }
}

fn unpack_archive(oci: &str, directory: &Path) -> Result<(), String> {
    let directory = fs::canonicalize(directory).map_err(
        |error| format!("{}: {error}", directory.display())
    )?;
    // The workspace is created next to the target directory such
    // that the data of a layer can be moved into the target without
    // copying it
    let workspace_parent = directory.parent().unwrap_or(Path::new("/"));
    let workspace = tempfile::Builder::new()
        .prefix(defaults::OCI_UNPACK_NAME_PREFIX)
        .tempdir_in(workspace_parent)
        .map_err(|error| format!(
            "Failed to create workspace in {}: {error}",
            workspace_parent.display()
        ))?;
    let archive = workspace.path().join("archive");
    let staging = workspace.path().join("layer");
    fs::create_dir(&archive).map_err(|error| error.to_string())?;

    info!("{} -x -f {oci} -C {}", defaults::TAR_TOOL, archive.display());
    run_tar(&[
        "-x".into(), "-f".into(), oci.into(),
        "-C".into(), archive.clone().into()
    ])?;
    let layers = get_layers(&archive)?;

    let mut dir_setup: HashMap<PathBuf, DirSetup> = HashMap::new();
    for (count, layer) in layers.iter().enumerate() {
        info!("Unpacking layer {}/{}...", count + 1, layers.len());
        if let Err(error) = apply_layer(
            layer, &staging, &directory, &mut dir_setup
        ) {
            // A layer which failed can leave directories behind
            // which are not writable. They would prevent the
            // workspace from being deleted
            if staging.exists() {
                let _ = prepare_staging(&staging);
            }
            return Err(error)
        }
    }
    // Directories are kept writable while the layers are applied.
    // Their mode as provided by the image is set at the very end,
    // deepest directories first
    let mut directories: Vec<(&PathBuf, &DirSetup)> =
        dir_setup.iter().collect();
    directories.sort_by_key(
        |(path, _)| std::cmp::Reverse(path.components().count())
    );
    for (path, (mode, _, _)) in directories {
        if is_real_dir(path) {
            set_mode(path, *mode)?;
        }
    }
    Ok(())
}

pub fn get_layers(archive: &Path) -> Result<Vec<PathBuf>, String> {
    /*!
    Get the layer files of the image in the given unpacked archive,
    lowest layer first
    !*/
    if archive.join("index.json").exists() {
        get_oci_layers(archive)
    } else if archive.join("manifest.json").exists() {
        get_docker_layers(archive)
    } else {
        Err(
            "Not an OCI image archive, neither index.json \
            nor manifest.json found".to_string()
        )
    }
}

fn get_oci_layers(archive: &Path) -> Result<Vec<PathBuf>, String> {
    /*!
    Follow index.json of an OCI image layout to the manifest of
    the image. An index can point to further indexes, e.g for
    multi architecture images, the manifest matching the
    architecture of the host is used
    !*/
    let mut document = read_json(&get_archive_file(archive, "index.json")?)?;
    for _ in 0..defaults::OCI_MAX_INDEX_DEPTH {
        if let Some(manifests) =
            document.get("manifests").and_then(Value::as_array)
        {
            let manifest = select_manifest(manifests)?;
            document = read_json(&get_blob(archive, manifest)?)?;
        } else if let Some(layers) =
            document.get("layers").and_then(Value::as_array)
        {
            return layers.iter().map(
                |layer| get_blob(archive, layer)
            ).collect()
        } else {
            return Err("Invalid OCI index or manifest".to_string())
        }
    }
    Err("Too many nested OCI indexes".to_string())
}

fn get_docker_layers(archive: &Path) -> Result<Vec<PathBuf>, String> {
    /*!
    Read the layers from manifest.json of a docker archive
    !*/
    let document = read_json(&get_archive_file(archive, "manifest.json")?)?;
    let images = document.as_array().ok_or("Invalid manifest.json")?;
    if images.len() != 1 {
        return Err(format!(
            "Expected exactly one image in manifest.json, found {}",
            images.len()
        ))
    }
    images[0].get("Layers").and_then(Value::as_array)
        .ok_or("No Layers in manifest.json")?
        .iter().map(|layer| match layer.as_str() {
            Some(layer) => get_archive_file(archive, layer),
            None => Err("Invalid layer in manifest.json".to_string())
        }).collect()
}

pub fn select_manifest(manifests: &[Value]) -> Result<&Value, String> {
    /*!
    Select the manifest for the host from the manifests of an
    OCI index. A manifest for the architecture of the host wins
    over a manifest without platform information. The latter
    is only taken if it is the only one of its kind
    !*/
    let architecture = get_oci_architecture();
    let platform_of = |manifest: &Value, key: &str| -> Option<String> {
        manifest.get("platform")?.get(key)?.as_str().map(str::to_string)
    };
    if let Some(manifest) = manifests.iter().find(|manifest|
        platform_of(manifest, "os").as_deref() == Some("linux")
        && platform_of(manifest, "architecture").as_deref()
            == Some(architecture)
    ) {
        return Ok(manifest)
    }
    let unspecific: Vec<&Value> = manifests.iter().filter(
        |manifest| manifest.get("platform").is_none()
    ).collect();
    match unspecific.as_slice() {
        [manifest] => Ok(manifest),
        [] => Err(format!(
            "No image for linux/{architecture} found in archive"
        )),
        _ => Err("Archive provides more than one image".to_string())
    }
}

fn get_oci_architecture() -> &'static str {
    /*!
    Architecture name of the host as used by OCI
    !*/
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "x86" => "386",
        "aarch64" => "arm64",
        "powerpc64" if cfg!(target_endian = "little") => "ppc64le",
        "powerpc64" => "ppc64",
        architecture => architecture
    }
}

fn get_blob(archive: &Path, descriptor: &Value) -> Result<PathBuf, String> {
    /*!
    Path to the blob of the given OCI descriptor
    !*/
    let digest = descriptor.get("digest").and_then(Value::as_str)
        .ok_or("OCI descriptor without digest")?;
    let valid = |part: &str, extra: &str| ! part.is_empty() && part.chars()
        .all(|c| c.is_ascii_alphanumeric() || extra.contains(c));
    match digest.split_once(':') {
        Some((algorithm, encoded))
            if valid(algorithm, "+._-") && valid(encoded, "=_-") =>
        {
            get_archive_file(archive, &format!("blobs/{algorithm}/{encoded}"))
        },
        _ => Err(format!("Invalid digest: {digest}"))
    }
}

pub fn get_archive_file(archive: &Path, name: &str) -> Result<PathBuf, String> {
    /*!
    Path to the regular file of the given name in the unpacked
    archive. The archive is not trusted, a file reached through
    a symlink pointing outside of the archive is refused
    !*/
    let file = fs::canonicalize(archive.join(name))
        .map_err(|error| format!("{name}: {error}"))?;
    let archive = fs::canonicalize(archive)
        .map_err(|error| error.to_string())?;
    if ! file.starts_with(&archive) || ! file.is_file() {
        return Err(format!("{name}: not a file in the archive"))
    }
    Ok(file)
}

fn read_json(file: &Path) -> Result<Value, String> {
    let content = fs::read_to_string(file)
        .map_err(|error| format!("{}: {error}", file.display()))?;
    serde_json::from_str(&content)
        .map_err(|error| format!("{}: {error}", file.display()))
}

fn apply_layer(
    layer: &Path, staging: &Path, target: &Path,
    dir_setup: &mut HashMap<PathBuf, DirSetup>
) -> Result<(), String> {
    /*!
    Apply the given layer on top of target

    The layer is unpacked into a staging directory first and its
    data is moved into target afterwards. Doing this in a second
    step allows to resolve symlinks of the layers below inside
    of target instead of following them on the host
    !*/
    fs::create_dir(staging).map_err(|error| error.to_string())?;
    let members = list_layer(layer)?;
    run_tar(&[
        "-x".into(), "--numeric-owner".into(), "-f".into(), layer.into(),
        "-C".into(), staging.into()
    ])?;
    let staging_dirs = prepare_staging(staging)?;

    let (whiteouts, members): (Vec<PathBuf>, Vec<PathBuf>) =
        members.into_iter().partition(|member| is_whiteout(member));

    // Whiteouts apply to the layers below only. They are therefore
    // handled before any data of this layer is moved to target
    for whiteout in whiteouts {
        let name = whiteout.file_name().unwrap().to_string_lossy();
        let parent = resolve_in_root(
            target, whiteout.parent().unwrap_or(Path::new(""))
        )?;
        if name == defaults::OCI_WHITEOUT_OPAQUE {
            if is_real_dir(&parent) {
                for entry in fs::read_dir(&parent).map_err(io_error(&parent))? {
                    let entry = entry.map_err(io_error(&parent))?;
                    remove_path(&entry.path(), dir_setup)?;
                }
            }
        } else {
            let deleted = &name[defaults::OCI_WHITEOUT_PREFIX.len()..];
            if ! deleted.is_empty() {
                remove_path(&parent.join(deleted), dir_setup)?;
            }
        }
    }

    let is_root = get_effective_uid() == 0;
    for member in members {
        let source = match get_staging_path(staging, &member) {
            Some(source) => source,
            None => {
                warn!("Skipping {}: not unpacked", member.display());
                continue
            }
        };
        let parent = resolve_in_root(
            target, member.parent().unwrap_or(Path::new(""))
        )?;
        fs::create_dir_all(&parent).map_err(io_error(&parent))?;
        let destination = parent.join(member.file_name().unwrap());
        if let Some(setup) = staging_dirs.get(&member) {
            if ! is_real_dir(&destination) {
                remove_path(&destination, dir_setup)?;
                fs::create_dir(&destination)
                    .map_err(io_error(&destination))?;
            }
            let (mode, uid, gid) = *setup;
            set_mode(&destination, mode | 0o700)?;
            if is_root {
                lchown(&destination, Some(uid), Some(gid))
                    .map_err(io_error(&destination))?;
            }
            dir_setup.insert(destination, *setup);
        } else {
            remove_path(&destination, dir_setup)?;
            fs::rename(&source, &destination)
                .map_err(io_error(&destination))?;
        }
    }
    fs::remove_dir_all(staging).map_err(io_error(staging))
}

fn list_layer(layer: &Path) -> Result<Vec<PathBuf>, String> {
    /*!
    List the members of the given layer in archive order. Each
    member is listed once, member names which point outside of
    the layer are skipped like tar does on extraction
    !*/
    let listing = run_tar(&[
        "-t".into(), "--quoting-style=escape".into(),
        "-f".into(), layer.into()
    ])?;
    let mut seen = HashSet::new();
    let mut members = Vec::new();
    for line in listing.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue
        }
        let name = PathBuf::from(OsString::from_vec(unescape_name(line)));
        if name.components().all(|component| matches!(
            component, Component::CurDir | Component::RootDir
        )) {
            // the root directory of the layer itself
            continue
        }
        if let Some(member) = normalize_member(&name) {
            if seen.insert(member.clone()) {
                members.push(member);
            }
        } else {
            warn!("Skipping member {}", name.display());
        }
    }
    Ok(members)
}

pub fn normalize_member(name: &Path) -> Option<PathBuf> {
    /*!
    Make the given member name relative to the root of the layer
    !*/
    let mut member = PathBuf::new();
    for component in name.components() {
        match component {
            Component::Normal(part) => member.push(part),
            Component::CurDir | Component::RootDir => {},
            Component::ParentDir | Component::Prefix(_) => return None
        }
    }
    if member.as_os_str().is_empty() {
        return None
    }
    Some(member)
}

pub fn unescape_name(name: &[u8]) -> Vec<u8> {
    /*!
    Revert the escape quoting style of the tar listing
    !*/
    let mut result = Vec::with_capacity(name.len());
    let mut index = 0;
    while index < name.len() {
        if name[index] != b'\\' || index + 1 == name.len() {
            result.push(name[index]);
            index += 1;
            continue
        }
        let next = name[index + 1];
        let octal = name[index + 1..].iter().take(3)
            .take_while(|c| c.is_ascii_digit() && **c < b'8').count();
        if octal > 0 {
            let value = name[index + 1..index + 1 + octal].iter()
                .fold(0u32, |value, c| value * 8 + (c - b'0') as u32);
            result.push(value as u8);
            index += 1 + octal;
            continue
        }
        let unescaped = match next {
            b'a' => 0x07,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            b'\\' => b'\\',
            _ => {
                result.push(b'\\');
                index += 1;
                continue
            }
        };
        result.push(unescaped);
        index += 2;
    }
    result
}

fn prepare_staging(staging: &Path) -> Result<HashMap<PathBuf, DirSetup>, String> {
    /*!
    Record the mode, owner and group of the directories of the
    unpacked layer and make them writable for the caller. Without
    write permission data could not be moved out of a directory
    !*/
    let mut staging_dirs = HashMap::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let path = staging.join(&relative);
        let metadata = fs::symlink_metadata(&path).map_err(io_error(&path))?;
        if ! relative.as_os_str().is_empty() {
            let mode = metadata.permissions().mode() & 0o7777;
            staging_dirs.insert(
                relative.clone(), (mode, metadata.uid(), metadata.gid())
            );
        }
        set_mode(&path, metadata.permissions().mode() | 0o700)?;
        for entry in fs::read_dir(&path).map_err(io_error(&path))? {
            let entry = entry.map_err(io_error(&path))?;
            if entry.file_type().map_err(io_error(&path))?.is_dir() {
                pending.push(relative.join(entry.file_name()));
            }
        }
    }
    Ok(staging_dirs)
}

fn get_staging_path(staging: &Path, member: &Path) -> Option<PathBuf> {
    /*!
    Path of the given member in the staging directory. A member
    reached through a symlink of the layer is refused, the
    symlink could point anywhere on the host
    !*/
    let mut path = staging.to_path_buf();
    let mut components = member.components().peekable();
    while let Some(component) = components.next() {
        path.push(component);
        let metadata = fs::symlink_metadata(&path).ok()?;
        if components.peek().is_some() && ! metadata.is_dir() {
            return None
        }
    }
    Some(path)
}

pub fn resolve_in_root(root: &Path, path: &Path) -> Result<PathBuf, String> {
    /*!
    Resolve the given relative path inside of root

    Symlinks are followed as if root were the root of the file
    system. Thus absolute symlinks and '..' never lead outside
    of root. All existing components of the result are no
    symlinks
    !*/
    let mut resolved = PathBuf::new();
    let mut pending: Vec<OsString> = path.components().rev()
        .map(|component| component.as_os_str().to_os_string()).collect();
    let mut symlinks = 0;
    while let Some(component) = pending.pop() {
        match Path::new(&component).components().next() {
            Some(Component::Normal(name)) => {
                let candidate = root.join(&resolved).join(name);
                match fs::symlink_metadata(&candidate) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        symlinks += 1;
                        if symlinks > defaults::OCI_MAX_SYMLINKS {
                            return Err(format!(
                                "Too many levels of symlinks: {}",
                                path.display()
                            ))
                        }
                        let link = fs::read_link(&candidate)
                            .map_err(io_error(&candidate))?;
                        if link.is_absolute() {
                            resolved.clear();
                        }
                        pending.extend(link.components().rev().map(
                            |component| component.as_os_str().to_os_string()
                        ));
                    },
                    _ => resolved.push(name)
                }
            },
            Some(Component::ParentDir) => {
                resolved.pop();
            },
            _ => {}
        }
    }
    Ok(root.join(resolved))
}

fn remove_path(
    path: &Path, dir_setup: &mut HashMap<PathBuf, DirSetup>
) -> Result<(), String> {
    /*!
    Delete the given path, a symlink is deleted, not its target
    !*/
    let result = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => Err(error)
    };
    dir_setup.retain(|directory, _| ! directory.starts_with(path));
    result.map_err(io_error(path))
}

fn is_whiteout(member: &Path) -> bool {
    member.file_name().is_some_and(|name| {
        name.to_string_lossy().starts_with(defaults::OCI_WHITEOUT_PREFIX)
    })
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(io_error(path))
}

fn io_error(path: &Path) -> impl Fn(io::Error) -> String + '_ {
    move |error| format!("{}: {error}", path.display())
}

fn run_tar(args: &[OsString]) -> Result<Vec<u8>, String> {
    let tool = defaults::TAR_TOOL;
    let output = Command::new(tool).args(args).output()
        .map_err(|error| format!("Failed to execute {tool}: {error}"))?;
    if ! output.status.success() {
        return Err(format!(
            "{tool} failed: {}", String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    fn tar(directory: &Path, archive: &Path, members: &[&str]) {
        let status = Command::new("tar")
            .arg("-c").arg("-f").arg(archive)
            .arg("-C").arg(directory)
            .args(members)
            .status().unwrap();
        assert!(status.success());
    }

    fn write_blob(layout: &Path, content: &[u8]) -> String {
        // Blobs are addressed by name only in these tests, the
        // digest is not verified on unpack
        let name = format!("{:064x}", fs::read_dir(
            layout.join("blobs/sha256")
        ).unwrap().count() + 1);
        fs::write(layout.join("blobs/sha256").join(&name), content).unwrap();
        format!("sha256:{name}")
    }

    fn create_oci_archive(work: &Path, layers: &[&Path]) -> PathBuf {
        let layout = work.join("layout");
        fs::create_dir_all(layout.join("blobs/sha256")).unwrap();
        fs::write(
            layout.join("oci-layout"), r#"{"imageLayoutVersion":"1.0.0"}"#
        ).unwrap();
        let layer_descriptors: Vec<Value> = layers.iter().map(|layer| json!({
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": write_blob(&layout, &fs::read(layer).unwrap())
        })).collect();
        let manifest = json!({
            "schemaVersion": 2,
            "layers": layer_descriptors
        });
        let manifest_digest = write_blob(
            &layout, manifest.to_string().as_bytes()
        );
        let index = json!({
            "schemaVersion": 2,
            "manifests": [{
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": manifest_digest
            }]
        });
        fs::write(layout.join("index.json"), index.to_string()).unwrap();
        let archive = work.join("image.tar");
        tar(&layout, &archive, &["."]);
        archive
    }

    #[test]
    fn test_unpack_layers_with_whiteouts() {
        let work = tempdir().unwrap();
        let work = work.path();

        // lower layer
        let lower = work.join("lower");
        fs::create_dir_all(lower.join("etc/conf.d")).unwrap();
        fs::create_dir_all(lower.join("usr/lib")).unwrap();
        fs::write(lower.join("etc/hostname"), "lower").unwrap();
        fs::write(lower.join("etc/deleted"), "lower").unwrap();
        fs::write(lower.join("etc/conf.d/old"), "lower").unwrap();
        symlink("usr/lib", lower.join("lib")).unwrap();
        symlink("/etc", lower.join("escape")).unwrap();
        tar(&lower, &work.join("lower.tar"), &["."]);

        // upper layer: replaces a file, deletes a file, makes a
        // directory opaque and writes through symlinks of the
        // lower layer
        let upper = work.join("upper");
        fs::create_dir_all(upper.join("etc/conf.d")).unwrap();
        fs::create_dir_all(upper.join("lib")).unwrap();
        fs::create_dir_all(upper.join("escape")).unwrap();
        fs::write(upper.join("etc/hostname"), "upper").unwrap();
        fs::write(upper.join("etc/.wh.deleted"), "").unwrap();
        fs::write(upper.join("etc/conf.d/.wh..wh..opq"), "").unwrap();
        fs::write(upper.join("etc/conf.d/new"), "upper").unwrap();
        fs::write(upper.join("lib/libfoo.so"), "upper").unwrap();
        fs::write(upper.join("escape/passwd"), "upper").unwrap();
        tar(&upper, &work.join("upper.tar"), &[
            "etc/hostname", "etc/.wh.deleted", "etc/conf.d/.wh..wh..opq",
            "etc/conf.d/new", "lib/libfoo.so", "escape/passwd"
        ]);

        let archive = create_oci_archive(
            work, &[&work.join("lower.tar"), &work.join("upper.tar")]
        );
        let target = work.join("target");
        fs::create_dir(&target).unwrap();
        assert!(unpack(archive.to_str().unwrap(), target.to_str().unwrap()));

        let read = |name: &str| fs::read_to_string(target.join(name)).unwrap();
        assert_eq!(read("etc/hostname"), "upper");
        assert!(! target.join("etc/deleted").exists());
        assert!(! target.join("etc/.wh.deleted").exists());
        assert!(! target.join("etc/conf.d/old").exists());
        assert_eq!(read("etc/conf.d/new"), "upper");
        // the file written through the lib symlink ends up in the
        // symlink target and the symlink is kept
        assert!(fs::symlink_metadata(target.join("lib"))
            .unwrap().file_type().is_symlink());
        assert_eq!(read("usr/lib/libfoo.so"), "upper");
        // the absolute symlink is resolved inside of target
        assert_eq!(read("etc/passwd"), "upper");
        // nothing of the workspace stays behind
        assert_eq!(fs::read_dir(work).unwrap()
            .filter(|entry| entry.as_ref().unwrap().file_name()
                .to_string_lossy()
                .starts_with(defaults::OCI_UNPACK_NAME_PREFIX)
            ).count(), 0);
    }

    #[test]
    fn test_unpack_restores_directory_mode() {
        let work = tempdir().unwrap();
        let work = work.path();
        let layer = work.join("layer");
        fs::create_dir_all(layer.join("readonly")).unwrap();
        fs::write(layer.join("readonly/file"), "data").unwrap();
        set_mode(&layer.join("readonly"), 0o555).unwrap();
        tar(&layer, &work.join("layer.tar"), &["."]);
        set_mode(&layer.join("readonly"), 0o755).unwrap();

        let archive = create_oci_archive(work, &[&work.join("layer.tar")]);
        let target = work.join("target");
        fs::create_dir(&target).unwrap();
        assert!(unpack(archive.to_str().unwrap(), target.to_str().unwrap()));
        let mode = fs::metadata(target.join("readonly"))
            .unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o555);
        assert_eq!(
            fs::read_to_string(target.join("readonly/file")).unwrap(), "data"
        );
        set_mode(&target.join("readonly"), 0o755).unwrap();
    }

    #[test]
    fn test_unpack_docker_archive() {
        let work = tempdir().unwrap();
        let work = work.path();
        let layer = work.join("layer");
        fs::create_dir_all(&layer).unwrap();
        fs::write(layer.join("file"), "data").unwrap();
        let archive_dir = work.join("archive");
        fs::create_dir_all(archive_dir.join("abc")).unwrap();
        tar(&layer, &archive_dir.join("abc/layer.tar"), &["."]);
        fs::write(
            archive_dir.join("manifest.json"),
            r#"[{"Config":"c.json","Layers":["abc/layer.tar"]}]"#
        ).unwrap();
        let archive = work.join("image.tar");
        tar(&archive_dir, &archive, &["."]);

        let target = work.join("target");
        fs::create_dir(&target).unwrap();
        assert!(unpack(archive.to_str().unwrap(), target.to_str().unwrap()));
        assert_eq!(fs::read_to_string(target.join("file")).unwrap(), "data");
    }

    #[test]
    fn test_unpack_invalid_archive() {
        let work = tempdir().unwrap();
        let work = work.path();
        let content = work.join("content");
        fs::create_dir_all(&content).unwrap();
        fs::write(content.join("file"), "data").unwrap();
        let archive = work.join("plain.tar");
        tar(&content, &archive, &["."]);
        let target = work.join("target");
        fs::create_dir(&target).unwrap();
        assert!(! unpack(archive.to_str().unwrap(), target.to_str().unwrap()));
    }

    #[test]
    fn test_get_archive_file_refuses_symlink_out_of_archive() {
        let archive = tempdir().unwrap();
        symlink("/etc/hostname", archive.path().join("index.json")).unwrap();
        assert!(get_archive_file(archive.path(), "index.json").is_err());
        assert!(get_archive_file(archive.path(), "../index.json").is_err());
    }

    #[test]
    fn test_get_blob_refuses_invalid_digest() {
        let archive = tempdir().unwrap();
        assert!(get_blob(
            archive.path(), &json!({"digest": "sha256:../../etc/passwd"})
        ).is_err());
        assert!(get_blob(archive.path(), &json!({"digest": "nodigest"}))
            .is_err());
    }

    #[test]
    fn test_resolve_in_root() {
        let root = tempdir().unwrap();
        let root = root.path();
        fs::create_dir_all(root.join("usr/lib")).unwrap();
        symlink("usr/lib", root.join("lib")).unwrap();
        symlink("/usr", root.join("abs")).unwrap();
        symlink("../../..", root.join("usr/up")).unwrap();
        symlink("loop", root.join("loop")).unwrap();
        assert_eq!(
            resolve_in_root(root, Path::new("lib/x")).unwrap(),
            root.join("usr/lib/x")
        );
        assert_eq!(
            resolve_in_root(root, Path::new("abs/lib")).unwrap(),
            root.join("usr/lib")
        );
        assert_eq!(
            resolve_in_root(root, Path::new("usr/up/etc")).unwrap(),
            root.join("etc")
        );
        assert_eq!(
            resolve_in_root(root, Path::new("../../etc")).unwrap(),
            root.join("etc")
        );
        assert!(resolve_in_root(root, Path::new("loop/x")).is_err());
    }

    #[test]
    fn test_select_manifest() {
        let architecture = get_oci_architecture();
        let manifests = vec![
            json!({"digest": "a", "platform": {
                "os": "linux", "architecture": "foreign"
            }}),
            json!({"digest": "b", "platform": {
                "os": "linux", "architecture": architecture
            }})
        ];
        assert_eq!(select_manifest(&manifests).unwrap()["digest"], "b");
        let manifests = vec![json!({"digest": "c"})];
        assert_eq!(select_manifest(&manifests).unwrap()["digest"], "c");
        let manifests = vec![json!({"digest": "c"}), json!({"digest": "d"})];
        assert!(select_manifest(&manifests).is_err());
        let manifests = vec![json!({"digest": "a", "platform": {
            "os": "linux", "architecture": "foreign"
        }})];
        assert!(select_manifest(&manifests).is_err());
    }

    #[test]
    fn test_unescape_name() {
        assert_eq!(unescape_name(b"etc/hostname"), b"etc/hostname");
        assert_eq!(unescape_name(b"a\\\\b"), b"a\\b");
        assert_eq!(unescape_name(b"a\\nb"), b"a\nb");
        assert_eq!(unescape_name(b"\\303\\244"), "ä".as_bytes());
    }

    #[test]
    fn test_normalize_member() {
        assert_eq!(
            normalize_member(Path::new("./etc/")),
            Some(PathBuf::from("etc"))
        );
        assert_eq!(
            normalize_member(Path::new("/usr/bin")),
            Some(PathBuf::from("usr/bin"))
        );
        assert_eq!(normalize_member(Path::new("./")), None);
        assert_eq!(normalize_member(Path::new("a/../../b")), None);
    }
}
