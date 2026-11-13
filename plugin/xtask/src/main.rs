mod auv2;

fn main() -> nih_plug_xtask::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // nih_plug_xtask bundles CLAP and VST3 and leaves the current directory
    // at the workspace root.
    nih_plug_xtask::main_with_args("cargo xtask", args.clone())?;
    if args.first().map(String::as_str) == Some("bundle") && args.iter().any(|a| a == "synthexis") {
        auv2::bundle()?;
    }
    Ok(())
}
