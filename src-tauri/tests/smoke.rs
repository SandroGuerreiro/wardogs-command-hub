#[test]
fn crate_exposes_version() {
    assert!(!wardogs_command_hub::VERSION.is_empty());
}
