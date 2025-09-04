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
use std::path::Path;
use std::{process::Command, ffi::OsStr};
use serde::{Serialize, Deserialize};
use crate::command::{CommandExtTrait, CommandError};
use uzers::{get_current_uid, get_current_groupname};
use crate::lookup::{Lookup};
use crate::error::FlakeError;

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

    pub fn get_name(&self) -> String {
        let mut user = String::new();
        if let Some(name) = self.name {
            user.push_str(name)
        }
        user
    }

    pub fn run<S: AsRef<OsStr>>(&self, command: S) -> Command {
        let mut c = Command::new("sudo");
        c.arg("--preserve-env");
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
        debug!("{:?}", call.get_args());
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

pub fn cp(source: &str, target: &str, user: User) -> Result<(), CommandError> {
    /*!
    copy filename via sudo
    !*/
    let mut call = user.run("cp");
    call.arg(source).arg(target);
    if Lookup::is_debug() {
        debug!("{:?}", call.get_args());
    }
    call.perform()?;
    Ok(())
}

pub fn chmod(filename: &str, mode: &str, user: User) -> Result<(), CommandError> {
    /*!
    Chmod filename via sudo
    !*/
    let mut call = user.run("chmod");
    call.arg(mode).arg(filename);
    if Lookup::is_debug() {
        debug!("{:?}", call.get_args());
    }
    call.perform()?;
    Ok(())
}

pub fn mkdir(dirname: &str, mode: &str, user: User) -> Result<(), CommandError> {
    /*!
    Make directory via sudo
    !*/
    let mut targetdir = dirname;

    let workdir;
    let origin = Path::new(&dirname);
    if origin.is_symlink() {
        workdir = origin.read_link().unwrap();
        targetdir = workdir.to_str().unwrap();
    }
    if ! Path::new(&targetdir).exists() {
        let mut mkdir_call = user.run("mkdir");
        mkdir_call.arg("-p").arg("-m").arg(mode).arg(targetdir);
        if Lookup::is_debug() {
            debug!("{:?}", mkdir_call.get_args());
        }
        mkdir_call.perform()?;
        let mut chmod_call = user.run("chmod");
        chmod_call.arg(mode).arg(targetdir);
        if Lookup::is_debug() {
            debug!("{:?}", chmod_call.get_args());
        }
        chmod_call.perform()?;
    }
    Ok(())
}
