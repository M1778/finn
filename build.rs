// =============================================================================
// Fin Programming Language
//
// Made with ❤️ for someone special.
//
// This project is genuinely built on love and dedication, a way to express your love can be done through Fin.
// Fin is a semi-object-orientated programming language that specialized in control, giving so much control to the user while asking for the bare minimum.
// Fin is the name of the Angel from BlackNightmare-Stories not yet published.
// “Love is the reason this project exists”
// -----------------------------------------------------------------------------
// Author: M1778
// Repository: https://github.com/M1778/finn
// Profile: https://github.com/M1778M/
// -----------------------------------------------------------------------------
// Copyright (C) 2027 M1778
//
// This file is part of finn, a project for the Fin Programming Language.
//
// Licensed under the MIT License. See LICENSE in the repository root.
//
// SPDX-License-Identifier: MIT
// =============================================================================

//! finn's build script exists for exactly one value: the target triple.
//!
//! `finc` release archives are named `finc-<semver>-<rust-target-triple>` and the
//! version index is keyed `versions[<version>].targets[<triple>]`, so the name finn
//! looks for has to be the triple finn was *built* for -- not a keyword inferred from
//! `cfg!(target_os)` at runtime.  Matching on the OS alone is what handed an arm64
//! user an x86_64 build (the old `download.rs:62-65`), and the cure is to stop
//! maintaining a mapping by hand: cargo already knows the answer and passes it here.
fn main() {
    let target = std::env::var("TARGET").expect("cargo always sets TARGET for a build script");
    println!("cargo:rustc-env=FINN_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
