//! synthexis.component: the Audio Unit v2 bundle for Logic Pro. Same
//! binary as the CLAP bundle; clap-wrapper's GetPluginFactoryAUV2 (lib.rs)
//! hosts the CLAP plugin inside it. Only the bundle layout, Info.plist and
//! the ad-hoc signature are made here.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

pub const NAME: &str = "synthexis";
/// Four-char codes for `auval -v aumu sgnl Snxs`. Apple reserves
/// all-lowercase manufacturer codes, so the manufacturer has a capital.
/// Changing any of them makes saved Logic projects lose the plugin.
pub const TYPE: &str = "aumu";
pub const SUBTYPE: &str = "sgnl";
pub const MANUFACTURER: &str = "Snxs";
const FACTORY: &str = "GetPluginFactoryAUV2";

/// Builds target/bundled/synthexis.component from the CLAP bundle that
/// nih_plug_xtask just made. Runs from the workspace root.
pub fn bundle() -> Result<()> {
    let bundled = target_dir().join("bundled");
    let binary = bundled.join(format!("{NAME}.clap/Contents/MacOS/{NAME}"));
    if !has_symbol(&binary, FACTORY)? {
        bail!("{} exports no {FACTORY}: clap_wrapper::export_auv2!() missing", binary.display());
    }
    let home = bundled.join(format!("{NAME}.component"));
    if home.exists() {
        fs::remove_dir_all(&home).context("could not remove the old component")?;
    }
    let macos = home.join("Contents/MacOS");
    fs::create_dir_all(&macos).context("could not create the component bundle")?;
    fs::copy(&binary, macos.join(NAME)).context("could not copy the plugin binary")?;
    let version = crate_version()?;
    fs::write(home.join("Contents/PkgInfo"), "BNDL????")?;
    fs::write(home.join("Contents/Info.plist"), info_plist(&version, au_version(&version)?))?;
    let signed = Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(&home)
        .status()
        .context("could not run codesign")?;
    if !signed.success() {
        bail!("codesign failed for {}", home.display());
    }
    eprintln!("Created an AUv2 bundle at '{}' ({TYPE} {SUBTYPE} {MANUFACTURER})", home.display());
    Ok(())
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| PathBuf::from("target"), PathBuf::from)
}

fn has_symbol(binary: &Path, name: &str) -> Result<bool> {
    let out = Command::new("nm")
        .args(["-gU"])
        .arg(binary)
        .output()
        .context("could not run nm")?;
    let want = format!("_{name}");
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.split_whitespace().last() == Some(want.as_str())))
}

/// The synthexis crate's version, so the AU and the other formats agree.
fn crate_version() -> Result<String> {
    let toml = fs::read_to_string("synthexis/Cargo.toml").context("could not read synthexis/Cargo.toml")?;
    toml.lines()
        .find_map(|l| l.strip_prefix("version = \"")?.strip_suffix('"'))
        .map(str::to_string)
        .context("no version in synthexis/Cargo.toml")
}

/// AU versions are 0xMMMMmmbb.
fn au_version(v: &str) -> Result<u32> {
    let parts: Vec<u32> = v.split('.').map(str::parse).collect::<Result<_, _>>()
        .with_context(|| format!("version {v} is not x.y.z"))?;
    match parts.as_slice() {
        [major, minor, patch] if *minor < 256 && *patch < 256 => Ok(major << 16 | minor << 8 | patch),
        _ => bail!("version {v} does not fit an AU version"),
    }
}

fn info_plist(version: &str, au_version: u32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>English</string>
  <key>CFBundleExecutable</key><string>{NAME}</string>
  <key>CFBundleIdentifier</key><string>com.synthexis.synthexis.component</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>{NAME}</string>
  <key>CFBundlePackageType</key><string>BNDL</string>
  <key>CFBundleSignature</key><string>????</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>{version}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>AudioComponents</key>
  <array>
    <dict>
      <key>type</key><string>{TYPE}</string>
      <key>subtype</key><string>{SUBTYPE}</string>
      <key>manufacturer</key><string>{MANUFACTURER}</string>
      <key>name</key><string>{NAME}: {NAME}</string>
      <key>description</key><string>camera-driven parameter macros</string>
      <key>version</key><integer>{au_version}</integer>
      <key>factoryFunction</key><string>{FACTORY}</string>
      <key>sandboxSafe</key><false/>
      <key>tags</key><array><string>Synthesizer</string></array>
    </dict>
  </array>
</dict>
</plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn au_version_packs_major_minor_patch() {
        assert_eq!(au_version("0.1.1").ok(), Some(0x0101));
        assert_eq!(au_version("1.2.3").ok(), Some(0x0001_0203));
        assert!(au_version("0.1").is_err());
        assert!(au_version("0.300.0").is_err());
    }

    #[test]
    fn codes_are_four_chars_and_manufacturer_is_not_all_lowercase() {
        for code in [TYPE, SUBTYPE, MANUFACTURER] {
            assert_eq!(code.len(), 4, "{code}");
        }
        assert!(MANUFACTURER.chars().any(|c| c.is_ascii_uppercase()));
    }
}
