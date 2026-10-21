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
//! Size of the tmpfs of a bubblewrap sandbox
//!
//! The writable layer of a sandbox lives in a tmpfs. Its size can
//! be requested per flake and is limited by the sandbox_tmpfs_size
//! setting of the system wide flakes configuration. The tmpfs is
//! mounted by bubblewrap-pilot or, for a root filesystem with mount
//! points below it, by the helper. Both apply the same rules.
//!
use crate::defaults;

pub fn get_tmpfs_size(
    requested: Option<&str>, limit: Option<&str>, memory: u64
) -> Result<Option<String>, String> {
    /*!
    Provide the size of the tmpfs of a sandbox

    The size limit is configured by root in the system wide flakes
    configuration. It is the default size, and a flake may request
    a size up to it but not more. Without a limit the requested
    size is used, without either the tmpfs gets the default size of
    the kernel
    !*/
    let limit_bytes = match limit {
        Some(limit) => Some(parse_tmpfs_size(limit, memory).map_err(
            |error| format!(
                "{error}, see sandbox_tmpfs_size in {}", defaults::FLAKES_CONFIG
            )
        )?),
        None => None
    };
    let requested = match requested {
        Some(requested) => requested,
        None => return Ok(limit.map(ToOwned::to_owned))
    };
    let requested_bytes = parse_tmpfs_size(requested, memory)?;
    if let (Some(limit), Some(limit_bytes)) = (limit, limit_bytes) {
        if requested_bytes > limit_bytes {
            return Err(format!(
                "The requested tmpfs size {requested} exceeds the limit \
                of {limit} set in {}", defaults::FLAKES_CONFIG
            ))
        }
    }
    Ok(Some(requested.to_string()))
}

pub fn parse_tmpfs_size(size: &str, memory: u64) -> Result<u64, String> {
    /*!
    Check a size in the notation of the size option of tmpfs and
    provide it in bytes. The size is a number with an optional k, m,
    g or t suffix, or a percentage of the given memory of the host.
    A size of 0 is refused, tmpfs takes it as no limit at all
    !*/
    let invalid = || format!(
        "Invalid tmpfs size {size:?}: expected a number with an optional \
        k, m, g or t suffix, or a percentage of the memory, e.g 2G or 25%"
    );
    if let Some(percent) = size.strip_suffix('%') {
        let percent: u64 = percent.parse().map_err(|_| invalid())?;
        if percent == 0 || percent > 100 {
            return Err(invalid())
        }
        return Ok((u128::from(memory) * u128::from(percent) / 100) as u64)
    }
    let (number, factor): (&str, u64) = match size.chars().last() {
        Some('k' | 'K') => (&size[..size.len() - 1], 1 << 10),
        Some('m' | 'M') => (&size[..size.len() - 1], 1 << 20),
        Some('g' | 'G') => (&size[..size.len() - 1], 1 << 30),
        Some('t' | 'T') => (&size[..size.len() - 1], 1 << 40),
        _ => (size, 1)
    };
    if number.is_empty() || ! number.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid())
    }
    let value: u64 = number.parse().map_err(|_| invalid())?;
    if value == 0 {
        return Err(invalid())
    }
    value.checked_mul(factor).ok_or_else(invalid)
}

pub fn host_memory() -> Result<u64, String> {
    /*!
    Provide the memory of the host in bytes
    !*/
    let meminfo = std::fs::read_to_string(defaults::PROC_MEMINFO).map_err(
        |error| format!("Failed to read {}: {error}", defaults::PROC_MEMINFO)
    )?;
    meminfo.lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|value| value.trim().strip_suffix("kB"))
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|kib| kib * 1024)
        .ok_or_else(|| format!("No MemTotal in {}", defaults::PROC_MEMINFO))
}

#[cfg(test)]
mod tests {
    use super::{get_tmpfs_size, host_memory, parse_tmpfs_size};

    #[test]
    fn test_get_tmpfs_size() {
        let memory = 8 << 30;
        // neither a request nor a limit, the default of the kernel
        assert_eq!(Ok(None), get_tmpfs_size(None, None, memory));
        // the limit is the default size
        assert_eq!(
            Ok(Some("2G".to_string())), get_tmpfs_size(None, Some("2G"), memory)
        );
        // a request without a limit, and one up to the limit
        assert_eq!(
            Ok(Some("4G".to_string())), get_tmpfs_size(Some("4G"), None, memory)
        );
        assert_eq!(
            Ok(Some("512M".to_string())),
            get_tmpfs_size(Some("512M"), Some("25%"), memory)
        );
        // a request above the limit is refused
        assert!(get_tmpfs_size(Some("3G"), Some("2G"), memory).is_err());
        assert!(get_tmpfs_size(Some("50%"), Some("2G"), memory).is_err());
        // as are invalid sizes
        assert!(get_tmpfs_size(Some("0"), None, memory).is_err());
        assert!(get_tmpfs_size(None, Some("lots"), memory).is_err());
    }

    #[test]
    fn test_parse_tmpfs_size() {
        let memory = 8 << 30;
        assert_eq!(Ok(2 << 30), parse_tmpfs_size("2G", memory));
        assert_eq!(Ok(512 << 20), parse_tmpfs_size("512m", memory));
        assert_eq!(Ok(4096), parse_tmpfs_size("4k", memory));
        assert_eq!(Ok(1000), parse_tmpfs_size("1000", memory));
        assert_eq!(Ok(2 << 30), parse_tmpfs_size("25%", memory));
        // tmpfs takes 0 as no limit at all
        for size in ["0", "0G", "0%", "", "G", "101%", "2x", "-1G", "2 G", "1.5G"] {
            assert!(parse_tmpfs_size(size, memory).is_err(), "{:?}", size);
        }
        assert!(parse_tmpfs_size("99999999999T", memory).is_err());
    }

    #[test]
    fn test_host_memory() {
        assert!(host_memory().unwrap() > 0);
    }
}
