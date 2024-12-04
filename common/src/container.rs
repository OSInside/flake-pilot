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

use crate::defaults;
use crate::flakelog::FlakeLog;
use crate::error::FlakeError;
use crate::user::User;
use crate::command::CommandExtTrait;
use users::{get_current_uid, get_current_gid};

#[derive(Debug, Default, Clone, Copy)]
pub struct Container {
}

impl Container {
    pub fn podman_setup_permissions() -> Result<(), FlakeError> {
        let root = User::from("root");
        let user_id = get_current_uid();
        let user_gid = get_current_gid();
        let chown_param = format!("{}:{}", user_id, user_gid);

        // This is an expensive operation
        let mut fix_storage = root.run("chown");
        fix_storage.arg("-R")
            .arg(chown_param.clone())
            .arg(defaults::FLAKES_REGISTRY);
        FlakeLog::debug(&format!("{:?}", fix_storage.get_args()));
        fix_storage.perform()?;

        let _ = Self::podman_setup_run_permissions();

        Ok(())
    }

    pub fn podman_setup_run_permissions() -> Result<(), FlakeError> {
        let root = User::from("root");
        let user_id = get_current_uid();
        let user_gid = get_current_gid();
        let chown_param = format!("{}:{}", user_id, user_gid);

        let mut fix_run_storage = root.run("chown");
        fix_run_storage.arg("-R")
            .arg(chown_param)
            .arg("/run/libpod")
            .arg(defaults::FLAKES_REGISTRY_RUNROOT);
        FlakeLog::debug(&format!("{:?}", fix_run_storage.get_args()));
        fix_run_storage.perform()?;

        Ok(())
    }
}
