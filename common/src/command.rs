//
// Copyright (c) 2023 Elektrobit Automotive GmbH
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
use std::fmt::{Display, Write};
use std::process::{Command, Output, CommandArgs};
use std::ffi::OsStr;
use thiserror::Error;

pub trait CommandExtTrait {
    /// Execute command via output() and return:
    /// 
    /// 1. An IO Error if the command could not be run
    /// 2. An ExecutionError if the Command was not successfull
    /// 3. The [Output] of the Command if the command was executed successfully
    /// 
    /// Attaches all args to the resulting error
    /// 
    /// If a termination with a non 0 exit status is considered succesful
    /// this method should not be used.
    fn perform(&mut self) -> Result<std::process::Output, CommandError>;
}

impl CommandExtTrait for Command {
    fn perform(&mut self) -> Result<std::process::Output, CommandError> {
        handle_output(self.output(), self.get_args())
    }
}

pub fn handle_output(
    maybe_output: Result<Output, std::io::Error>, args: CommandArgs
) -> Result<std::process::Output, CommandError> {
    let out = maybe_output.map_err(ProcessError::IO);

    let error: ProcessError = match out {
        Ok(output) => {
            if output.status.success() {
                return Ok(output);
            } else {
                output.into()
            }
        }
        Err(error) => error,
    };
    // Provide caller arguments in addition to the error
    Err(
        CommandError {
            base: error,
            args: args
                .flat_map(OsStr::to_str)
                .map(ToOwned::to_owned)
                .collect(),
        }
    )
}

#[derive(Debug, Error)]
pub enum ProcessError {
    // The command could not be called
    #[error(transparent)]
    IO(#[from] std::io::Error),

    // The Command could be called but has a non zero exit status
    #[error("The process failed with status {}", .0.status)]
    ExecutionError(std::process::Output),
}

impl From<std::process::Output> for ProcessError {
    fn from(value: std::process::Output) -> Self {
        Self::ExecutionError(value)
    }
}

#[derive(Debug, Error)]
pub struct CommandError {
    pub base: ProcessError,
    pub args: Vec<String>,
}

impl CommandError {
    pub fn new(base: ProcessError) -> Self {
        Self {
            args: Vec::new(),
            base,
        }
    }
}

impl Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_char('"')?;
        for arg in self.args.iter() {
            f.write_str(arg)?;
            f.write_char(' ')?;
        }
        f.write_char('"')?;
        f.write_char(':')?;
        f.write_char(' ')?;
        f.write_str(format!("{:?}", self.base).as_str())?;
        f.write_char(' ')?;
        std::fmt::Display::fmt(&self.base, f)
    }
}
