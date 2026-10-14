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
use std::env;
use std::path::PathBuf;

use uzers::get_user_by_uid;
use uzers::os::unix::UserExt;

// Caller is the user the helper acts for
#[derive(Debug, Clone, PartialEq)]
pub struct Caller {
    /// ID of the user
    pub uid: u32,
    /// ID of the primary group of the user
    pub gid: u32,
    /// Login name of the user
    pub name: String,
    /// Home directory of the user
    pub home: PathBuf
}

impl Caller {
    pub fn from_environment() -> Result<Caller, String> {
        /*!
        Identify the caller of the helper

        The helper has to run as root, through sudo or called by
        root directly. A helper installed setuid root runs with the
        real user ID of its caller and is refused, it would have to
        defend itself against everything a caller controls. sudo
        provides the ID of the user who called it in SUDO_UID, a
        variable a caller cannot set for a process running as root.
        Without it the caller is root itself. The other properties
        of the caller are taken from the user database
        !*/
        if unsafe { libc::getuid() } != 0 || unsafe { libc::geteuid() } != 0 {
            return Err(
                "must be called through sudo or by root, a setuid \
                installation is not supported".to_string()
            )
        }
        let uid = match env::var("SUDO_UID") {
            Ok(sudo_uid) => sudo_uid.parse::<u32>().map_err(
                |_| format!("invalid SUDO_UID {sudo_uid:?}")
            )?,
            Err(env::VarError::NotPresent) => 0,
            Err(error) => return Err(format!("invalid SUDO_UID: {error}"))
        };
        Caller::from_uid(uid)
    }

    pub fn from_uid(uid: u32) -> Result<Caller, String> {
        /*!
        Provide the caller of the given user ID
        !*/
        let user = get_user_by_uid(uid).ok_or(
            format!("user ID {uid} does not exist")
        )?;
        Ok(Caller {
            uid,
            gid: user.primary_group_id(),
            name: user.name().to_string_lossy().to_string(),
            home: user.home_dir().to_path_buf()
        })
    }

    pub fn is_root(&self) -> bool {
        self.uid == 0
    }

    pub fn may_act_for(&self, uid: u32) -> bool {
        /*!
        Check if the caller may act for the user with the given ID.
        Every user may act for itself, root for every user
        !*/
        self.is_root() || self.uid == uid
    }
}

#[cfg(test)]
mod tests {
    use super::Caller;
    use uzers::get_current_uid;

    #[test]
    fn test_caller_from_uid() {
        let caller = Caller::from_uid(get_current_uid()).unwrap();
        assert_eq!(get_current_uid(), caller.uid);
        assert!(! caller.name.is_empty());
        assert!(Caller::from_uid(u32::MAX - 1).is_err());
    }

    #[test]
    fn test_may_act_for() {
        let mut caller = Caller::from_uid(get_current_uid()).unwrap();
        caller.uid = 1000;
        assert!(caller.may_act_for(1000));
        assert!(! caller.may_act_for(1001));
        assert!(! caller.may_act_for(0));
        caller.uid = 0;
        assert!(caller.may_act_for(1001));
    }
}
