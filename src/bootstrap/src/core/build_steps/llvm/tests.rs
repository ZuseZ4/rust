use std::ffi::OsStr;
use std::path::PathBuf;

use super::{InTreeLld, subproject_linker_flags};
use crate::core::config::{Config, TargetSelection};
use crate::utils::tests::TestCtx;

fn config(toml: &str) -> Config {
    TestCtx::new().config("check").with_default_toml_config(toml).create_config()
}

fn linux() -> TargetSelection {
    TargetSelection::from_user("x86_64-unknown-linux-gnu")
}

fn darwin() -> TargetSelection {
    TargetSelection::from_user("aarch64-apple-darwin")
}

fn lld() -> InTreeLld {
    InTreeLld {
        bin_dir: PathBuf::from("/build/x86_64-unknown-linux-gnu/lld/bin"),
        dylib_dir: Some(PathBuf::from("/build/x86_64-unknown-linux-gnu/llvm/lib")),
    }
}

#[test]
fn defaults_to_the_system_linker() {
    assert_eq!(subproject_linker_flags(&config(""), linux(), None), None);
}

#[test]
fn honors_llvm_use_linker() {
    assert_eq!(
        subproject_linker_flags(&config(r#"llvm.use-linker = "mold""#), linux(), None).as_deref(),
        Some(OsStr::new("-fuse-ld=mold"))
    );
}

#[test]
fn uses_the_in_tree_lld() {
    assert_eq!(
        subproject_linker_flags(&config(""), linux(), Some(&lld())).as_deref(),
        Some(OsStr::new("-B/build/x86_64-unknown-linux-gnu/lld/bin -fuse-ld=lld"))
    );
}

#[test]
fn llvm_use_linker_wins_over_the_in_tree_lld() {
    assert_eq!(
        subproject_linker_flags(&config(r#"llvm.use-linker = "mold""#), linux(), Some(&lld()))
            .as_deref(),
        Some(OsStr::new("-fuse-ld=mold"))
    );
}

#[test]
fn thin_lto_still_falls_back_to_lld() {
    assert_eq!(
        subproject_linker_flags(&config("llvm.thin-lto = true"), linux(), None).as_deref(),
        Some(OsStr::new("-fuse-ld=lld"))
    );
}

#[test]
fn thin_lto_does_not_use_lld_on_darwin() {
    assert_eq!(subproject_linker_flags(&config("llvm.thin-lto = true"), darwin(), None), None);
}
