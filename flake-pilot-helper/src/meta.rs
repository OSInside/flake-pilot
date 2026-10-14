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
//! The meta data directories of the pilots
//!
//! The pilots keep the meta data of their instances in a private
//! directory of each user below a base directory which is shared by
//! all users, /tmp/flakes by default. The base directory has to
//! belong to root, see flakes::io::IO::meta_dir_problem(). It is
//! created by systemd-tmpfiles at boot or by this command.
//!
use flakes::config::{
    get_bubblewrap_ids_dir, get_firecracker_ids_dir, get_podman_ids_dir
};
use flakes::defaults::META_DIR_MODE;
use flakes::io::IO;
use flakes::openat;
use flakes::user::octal_mode;

use crate::caller::Caller;
use crate::validate;

pub fn init_dirs(caller: &Caller) -> bool {
    /*!
    Create the meta data base directories of the pilots

    The directories are taken from the system wide configuration,
    which belongs to root, the command takes no argument of the
    caller. A directory is created without following a symbolic
    link: it is usually placed in a directory which every user can
    write to, like /tmp. An existing directory is left as it is and
    reported if it is not set up as the pilots expect
    !*/
    info!(
        "Meta data directories requested by {} (user ID {})",
        caller.name, caller.uid
    );
    let mode = match octal_mode(META_DIR_MODE) {
        Ok(mode) => mode,
        Err(error) => {
            error!("{error}");
            return false
        }
    };
    let mut ids_dirs = vec![
        get_podman_ids_dir(false),
        get_firecracker_ids_dir(false),
        get_bubblewrap_ids_dir(false)
    ];
    ids_dirs.sort();
    ids_dirs.dedup();
    let mut ok = true;
    for ids_dir in ids_dirs {
        if let Err(error) = validate::absolute_path(&ids_dir) {
            error!("{error}");
            ok = false;
            continue
        }
        if let Err(error) = openat::create_dir_all(&ids_dir, mode) {
            error!("Failed to create {ids_dir}: {error}");
            ok = false;
            continue
        }
        if let Some(problem) = IO::meta_dir_problem(&ids_dir) {
            error!("{problem}");
            ok = false
        }
    }
    ok
}
