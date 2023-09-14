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
use crate::flakelog::FlakeLog;
use crate::error::FlakeError;
use crate::user::User;
use crate::command::CommandExtTrait;

#[derive(Debug, Default, Clone, Copy)]
pub struct IO {
}

impl IO {
    pub fn sync_includes(
        target: &String, tar_includes: Vec<&str>, path_includes: Vec<&str>, user: User
    ) -> Result<(), FlakeError> {
        /*!
        Sync custom include data to target path
        !*/
        for tar in tar_includes {
            FlakeLog::debug(&format!("Provision tar archive: [{}]", tar));
            let mut call = user.run("tar");
            call.arg("-C").arg(target)
                .arg("-xf").arg(tar);
            FlakeLog::debug(&format!("{:?}", call.get_args()));
            let output = call.perform()?;
            FlakeLog::debug(
                &format!("{}", &String::from_utf8_lossy(&output.stdout))
            );
            FlakeLog::debug(
                &format!("{}", &String::from_utf8_lossy(&output.stderr))
            );
        }
        for path in path_includes {
            FlakeLog::debug(&format!("Provision path: [{}]", path));
            Self::sync_data(
                path, &format!("{}/{}", target, path),
                ["--mkpath"].to_vec(), user
            )?;
        }
        Ok(())
    }

    pub fn sync_data(
        source: &str, target: &str, options: Vec<&str>, user: User
    ) -> Result<(), FlakeError> {
        /*!
        Sync data from source path to target path
        !*/
        let mut call = user.run("rsync");
        call.arg("-av");
        for option in options {
            call.arg(option);
        }
        call.arg(source).arg(target);
        FlakeLog::debug(&format!("{:?}", call.get_args()));
        let output = call.output()?;
        FlakeLog::debug(
            &format!("{}", &String::from_utf8_lossy(&output.stdout))
        );
        FlakeLog::debug(
            &format!("{}", &String::from_utf8_lossy(&output.stderr))
        );
        if !output.status.success() {
            return Err(FlakeError::SyncFailed)
        }
        Ok(())
    }
}
