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
//! The bwrap options of the bubblewrap flakes
//!
//! The bwrap options of a flake are read by bubblewrap-pilot. For
//! a system wide flake with mount points below its root filesystem
//! they are also read by the helper, which mounts the layers they
//! configure, see its bubblewrap-pilot command. Both read the
//! layers the same way, through this module.
//!
use lazy_static::lazy_static;
use regex::Regex;

use crate::defaults;

lazy_static! {
    /// Reference of an environment variable in a runtime option
    static ref VAR_PATTERN: Regex = Regex::new(r"%([A-Z]+)").unwrap();
}

pub fn split_options<F>(
    options: &[&str], lookup: F
) -> Result<(Vec<String>, Vec<String>), String>
where
    F: Fn(&str) -> Option<String>
{
    /*!
    Split the given bwrap options of a flake into the layers stacked
    on top of the rootfs and the other arguments of bwrap

    An option can be configured together with its value(s) in one
    entry, e.g "--ro-bind /etc /etc". bwrap expects them as separate
    arguments. Variable references are resolved through the given
    lookup, see expand_variables(). A source configured with
    --overlay-src is a layer, in the order the sources are given
    !*/
    let mut arguments: Vec<String> = Vec::new();
    for option in options.iter().flat_map(|x| x.split_whitespace()) {
        arguments.push(expand_variables(option, &lookup)?);
    }
    let mut layers: Vec<String> = Vec::new();
    let mut other_arguments: Vec<String> = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == defaults::BWRAP_OVERLAY_SRC_OPTION {
            if let Some(layer) = arguments.next() {
                layers.push(layer);
            }
        } else {
            other_arguments.push(argument);
        }
    }
    Ok((layers, other_arguments))
}

pub fn expand_variables<F>(option: &str, lookup: F) -> Result<String, String>
where
    F: Fn(&str) -> Option<String>
{
    /*!
    Resolve the variable references of the given bwrap argument

    A reference in the format %NAME is replaced by the value the
    given lookup provides for the variable of that name, usually
    the environment. A variable the lookup does not provide is
    provided as a shell style variable reference, $NAME
    !*/
    let mut option_value = option.to_string();
    // The value of a variable can contain another %VAR reference.
    // Expansion is therefore done repeatedly but only up to a
    // fixed limit. Without it a variable value referencing itself
    // would keep this loop running forever
    let mut expansion_count = 0;
    while VAR_PATTERN.captures(&option_value.clone()).is_some() {
        expansion_count += 1;
        if expansion_count > defaults::VAR_EXPANSION_LIMIT {
            return Err(format!("Too many variable expansions in: {option}"))
        }
        for capture in VAR_PATTERN.captures_iter(&option_value.clone()) {
            // replace %VAR placeholder(s) with the respective
            // variable value if possible.
            // If not possible replace by the variable name
            let var_name = capture.get(1).unwrap().as_str();
            let var_value = lookup(var_name)
                .unwrap_or(format!("${var_name}"));
            option_value = option_value.replace(
                &format!("%{var_name}"), &var_value
            );
        }
    }
    Ok(option_value)
}

#[cfg(test)]
mod tests {
    use super::{expand_variables, split_options};

    fn lookup(name: &str) -> Option<String> {
        match name {
            "HOME" => Some("/home/user".to_string()),
            "LOOP" => Some("%LOOP".to_string()),
            _ => None
        }
    }

    #[test]
    fn test_split_options() {
        let (layers, arguments) = split_options(
            &["--overlay-src /usr/share/a", "--ro-bind %HOME /home",
              "--overlay-src %HOME/b", "--overlay-src"],
            lookup
        ).unwrap();
        assert_eq!(vec!["/usr/share/a", "/home/user/b"], layers);
        assert_eq!(vec!["--ro-bind", "/home/user", "/home"], arguments);
    }

    #[test]
    fn test_expand_variables() {
        assert_eq!("/home/user/x", expand_variables("%HOME/x", lookup).unwrap());
        assert_eq!("$UNSET", expand_variables("%UNSET", lookup).unwrap());
        // a variable referencing itself stops at the limit
        assert!(expand_variables("%LOOP", lookup).is_err());
    }
}
