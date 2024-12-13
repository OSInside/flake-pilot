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
pub const GC_THRESHOLD: i32 = 20;
pub const HOST_DEPENDENCIES: &str = "removed";
pub const SYSTEM_HOST_DEPENDENCIES: &str = "systemfiles";
pub const SYSTEM_HOST_DEPENDENCIES_LIBS: &str = "systemfiles.libs";
pub const PODMAN_PATH: &str = "/usr/bin/podman";
pub const FLAKES_STORAGE: &str = "/etc/flakes/storage.conf";
pub const FLAKES_REGISTRY: &str = "/usr/share/flakes/storage";
pub const FLAKES_REGISTRY_RUNROOT: &str = "/run/flakes";
