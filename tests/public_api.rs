use skillctl::cli::Cli;
use skillctl::install::{EntryState, Marker, PathChange};
use skillctl::lockfile::{LockFile, Mode, Resolved, SkillEntry, SourceSelector, SourceSpec};
use skillctl::source::{SkillMetadata, Snapshot};

#[test]
fn pre_refactor_public_module_paths_remain_available() {
    fn assert_sized<T: Sized>() {}

    assert_sized::<Cli>();
    assert_sized::<LockFile>();
    assert_sized::<Mode>();
    assert_sized::<SkillEntry>();
    assert_sized::<SourceSpec>();
    assert_sized::<Resolved>();
    assert_sized::<SourceSelector>();
    assert_sized::<Snapshot>();
    assert_sized::<SkillMetadata>();
    assert_sized::<Marker>();
    assert_sized::<EntryState>();
    assert_sized::<PathChange>();

    let _ = skillctl::run;
    let _ = skillctl::install::entry_state;
    let _ = skillctl::lockfile::safe_relative_path;
    let _ = skillctl::source::acquire;
}
