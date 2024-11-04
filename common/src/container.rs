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
extern crate ini;

use crate::flakelog::FlakeLog;
use crate::error::FlakeError;
use crate::user::User;
use crate::command::CommandExtTrait;
use tempfile::NamedTempFile;
use std::path::Path;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::env;
use ini::Ini;

#[derive(Debug, Default, Clone, Copy)]
pub struct Container {
}

impl Container {
    pub fn podman_write_custom_storage_config(
        mut storage_conf: &NamedTempFile
    ) -> Result<(), FlakeError> {
        /*!
        Create storage conf to point root to the user
        storage data such that mounting becomes possible
        !*/
        let mut storage = Ini::new();
        storage.with_section(Some("storage"))
            .set("driver", "\"overlay\"")
            .set(
                "graphroot",
                format!("\"{}/.local/share/containers/storage\"",
                    env::var("HOME").unwrap()
                )
            );
        storage.write_to(&mut storage_conf)?;
        Ok(())
    }

    pub fn podman_fix_storage_permissions(
        runas: &str
    ) -> Result<(), FlakeError> {
        /*!
        Fix user storage permissions
        !*/
        let user = User::from(runas);
        let user_name = user.get_name();
        let user_id = user.get_user_id();
        let user_group = user.get_group_name();
        let root = User::from("root");
        let mut fix_run = root.run("chown");
        fix_run.arg("-R")
            .arg(format!("{}:{}", user_name, user_group))
            .arg(format!("/run/user/{}/containers", user_id));
        FlakeLog::debug(&format!("{:?}", fix_run.get_args()));
        fix_run.perform()?;

        let paths;
        let storage_dir = format!(
            "/home/{}/.local/share/containers/storage/overlay", user_name
        );
        match fs::read_dir(storage_dir.clone()) {
            Ok(result) => { paths = result },
            Err(error) => {
                return Err(FlakeError::IOError {
                    kind: format!("{:?}", error.kind()),
                    message: format!("fs::read_dir failed on {}: {}",
                        storage_dir, error
                    )
                })
            }
        };
        for path in paths {
            let file_path = path.unwrap().path();
            let work_path = format!("{}/work/work", file_path.display());
            let meta = fs::metadata(&file_path)?;
            if Path::new(&work_path).exists() {
                let work_meta = fs::metadata(&work_path)?;
                if work_meta.uid() == 0 {
                    let mut fix_storage = root.run("chown");
                    fix_storage.arg("-R")
                        .arg(format!("{}:{}", user_name, user_group))
                        .arg(&file_path);
                    FlakeLog::debug(&format!("{:?}", fix_storage.get_args()));
                    fix_storage.perform()?;
                }
            } else if meta.uid() == 0 {
                let mut fix_storage = root.run("chown");
                fix_storage.arg(format!("{}:{}", user_name, user_group))
                    .arg(&file_path);
                FlakeLog::debug(&format!("{:?}", fix_storage.get_args()));
                fix_storage.perform()?;
            }
        }
        Ok(())
    }
}
