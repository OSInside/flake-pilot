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
//! The mount table of a mount namespace
//!
//! The kernel provides the mounts of the namespace of a process in
//! /proc/PID/mountinfo, one mount per line, see proc_pid_mountinfo(5):
//!
//! 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw
//! |  |  |    |     |
//! |  |  |    |     └── mount point, relative to the root of the reader
//! |  |  |    └── root of the mount within its filesystem
//! |  |  └── major:minor of the filesystem
//! |  └── ID of the parent mount
//! └── ID of the mount
//!
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub id: u64,
    pub parent_id: u64,
    pub device: String,
    pub root: String,
    pub mount_point: PathBuf,
}

pub fn parse(mountinfo: &str) -> Vec<Mount> {
    /*!
    Provide the mounts of the given mount table. Lines which do not
    follow the format are skipped
    !*/
    mountinfo.lines().filter_map(|line| {
        let mut fields = line.split(' ');
        Some(Mount {
            id: fields.next()?.parse().ok()?,
            parent_id: fields.next()?.parse().ok()?,
            device: fields.next()?.to_string(),
            root: unescape(fields.next()?),
            mount_point: PathBuf::from(unescape(fields.next()?)),
        })
    }).collect()
}

fn unescape(field: &str) -> String {
    /*!
    Resolve the octal escapes of a path in the mount table, e.g
    \040 for a space
    !*/
    let bytes = field.as_bytes();
    let mut unescaped = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let escape = bytes.get(index + 1..index + 4).filter(
            |digits| bytes[index] == b'\\'
                && digits.iter().all(|digit| (b'0'..=b'7').contains(digit))
        );
        match escape {
            Some(digits) => {
                unescaped.push(digits.iter().fold(
                    0_u8, |byte, digit| byte.wrapping_mul(8).wrapping_add(digit - b'0')
                ));
                index += 4
            },
            None => {
                unescaped.push(bytes[index]);
                index += 1
            }
        }
    }
    String::from_utf8_lossy(&unescaped).to_string()
}

#[cfg(test)]
mod tests {
    use super::{Mount, parse};
    use std::path::PathBuf;

    #[test]
    fn test_parse() {
        let mounts = parse("\
            22 1 0:21 / / rw,relatime shared:1 - btrfs /dev/vda2 rw\n\
            garbage\n\
            25 22 0:23 /my\\040dir /data/my\\040tree rw - ext4 /dev/vdb rw\n");
        assert_eq!(vec![
            Mount {
                id: 22, parent_id: 1, device: "0:21".to_string(),
                root: "/".to_string(), mount_point: PathBuf::from("/")
            },
            Mount {
                id: 25, parent_id: 22, device: "0:23".to_string(),
                root: "/my dir".to_string(),
                mount_point: PathBuf::from("/data/my tree")
            }
        ], mounts);
    }
}
