// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

fn main() {
    // Tell Cargo to rerun this build script if the grammar file changes
    println!("cargo:rerun-if-changed=grammer.pest");
}