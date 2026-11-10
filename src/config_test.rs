use std::fs;

use tempfile::tempdir;

use super::*;

#[test]
fn loads_role_rules_from_the_root_or_an_explicit_path() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(CONFIG_FILE),
        r#"{ "roles": { ".layer": { "require": ["exported `layer`"], "forbid": ["imports .fake"] }, ".utils": {} } }"#,
    )
    .unwrap();

    let config = Config::load(dir.path(), None).unwrap();
    assert_eq!(config.roles[".layer"].require, ["exported `layer`"]);
    assert_eq!(config.roles[".layer"].forbid, ["imports .fake"]);
    assert_eq!(config.roles[".utils"], RoleRule::default());

    let other = dir.path().join("other.json");
    fs::write(&other, "{}").unwrap();
    assert_eq!(Config::load(dir.path(), other.to_str()).unwrap(), Config::default());
}

#[test]
fn rejects_a_missing_file_bad_json_and_unknown_keys() {
    let dir = tempdir().unwrap();
    let missing = Config::load(dir.path(), None).unwrap_err().to_string();
    assert!(missing.contains("no config at") && missing.contains("scanr.json"), "{missing}");

    fs::write(dir.path().join(CONFIG_FILE), "{ not json").unwrap();
    assert!(Config::load(dir.path(), None).unwrap_err().to_string().contains("invalid config"));

    fs::write(dir.path().join(CONFIG_FILE), r#"{ "roles": { ".layer": { "requires": [] } } }"#)
        .unwrap();
    let unknown = format!("{:#}", Config::load(dir.path(), None).unwrap_err());
    assert!(unknown.contains("unknown field `requires`"), "{unknown}");
}
