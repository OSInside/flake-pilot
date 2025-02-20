//
// Copyright (c) 2022 Elektrobit Automotive GmbH
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
use crate::app_path::program_abs_path;
use crate::app_path::basename;
use crate::config::config_file;
use crate::config::config_from_str;

#[test]
fn test_program_abs_path() {
    let program_path = program_abs_path();
    assert!(program_path.starts_with('/'));
}

#[test]
fn test_basename() {
    let base_name = basename(&"/some/name".to_string());
    assert_eq!("name", base_name);
}

#[test]
fn simple_config() {
    let cfg = config_from_str(
r#"container:
 name: JoJo
 host_app_path: /myapp
 check_host_dependencies: false
include:
 tar: ~
"#);
    assert_eq!(cfg.container.name, "JoJo");
}

#[test]
fn combine_configs() {
    let cfg = config_from_str(
r#"container:
 name: JoJo
 host_app_path: /myapp
 check_host_dependencies: false
include:
 tar: ~
container:
 name: Dio
 host_app_path: /other
 check_host_dependencies: false
"#);
    assert_eq!(cfg.container.name, "Dio");
}

#[test]
fn test_program_config_file() {
    let config_file = config_file(&"app".to_string());
    assert_eq!("/usr/share/flakes/app.yaml", config_file);
}
