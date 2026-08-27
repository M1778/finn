use crate::FinnContext;
use crate::commands::build;
use crate::finc::SUPPORTED_CONTRACT;
use anyhow::{Result, anyhow};
use colored::*;

/// `finn run` cannot run anything yet, and says so instead of pretending.
///
/// Contract 1 is explicit: `finc` has no code generation, `-o` is parsed and then
/// ignored, and a `0` means "accepted the source" rather than "wrote a binary". The
/// previous implementation pushed a `-r` flag at the compiler in the hope of a JIT; under
/// contract 1 an unknown flag is exit 2, so that hope now produces a confusing failure
/// rather than a run. This checks the source -- which is the part that does work -- and
/// then reports the gap honestly.
pub fn run(args: Vec<String>, ctx: &FinnContext) -> Result<()> {
    if !ctx.quiet {
        println!(
            "{} finc contract {} produces no executable; checking the source instead.",
            "[WARN]".yellow(),
            SUPPORTED_CONTRACT
        );
    }

    build::run(args, ctx)?;

    Err(anyhow!(
        "Nothing to run: finc does not generate code yet, so `finn build` produced no \
         executable. The source above was accepted. `finn run` will work as soon as finc \
         emits a binary; until then this is a gap in the compiler, not in your project."
    ))
}
