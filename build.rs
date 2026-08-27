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
