//! Proves schema field inheritance works end-to-end through the
//! `test-utils`-gated public surface alone. No crate-internal imports.
//!
//! `src/schema/service.rs` unit-tests resolution internals. This is the only
//! test proving that field inheritance works when called only through
//! `SchemaService::resolve` + `schema_get` + `schema_field`.

use pretty_assertions::assert_eq;
use traces_pkm::SchemaService;

fn write_schema(dir: &std::path::Path, name: &str, toml: &str) {
    std::fs::write(dir.join(format!("{name}.toml")), toml)
        .expect("write schema fixture");
}

#[test]
fn child_schema_inherits_parent_fields() {
    let temp = tempfile::tempdir().expect("create temp dir");
    write_schema(
        temp.path(),
        "book",
        r#"
        [fields.status]
        type = "select"
        values = ["draft", "done"]
        "#,
    );
    write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

    let service = SchemaService::resolve(temp.path()).expect("registry loads");

    let sci_fi = service.schema_get("sci_fi").expect("sci_fi resolved");
    assert!(
        sci_fi.schema_field("status"),
        "sci_fi must inherit status from book"
    );
}

#[test]
fn parent_fields_override_is_not_lost_when_child_adds_own_fields() {
    let temp = tempfile::tempdir().expect("create temp dir");
    write_schema(
        temp.path(),
        "book",
        r#"
        [fields.status]
        type = "select"
        values = ["draft", "done"]
        "#,
    );
    write_schema(
        temp.path(),
        "sci_fi",
        r#"
        extends = ["book"]

        [fields.setting]
        type = "input"
        "#,
    );

    let service = SchemaService::resolve(temp.path()).expect("registry loads");

    let sci_fi = service.schema_get("sci_fi").expect("sci_fi resolved");
    assert!(
        sci_fi.schema_field("status"),
        "sci_fi must still inherit status after adding own field"
    );
    assert!(
        sci_fi.schema_field("setting"),
        "sci_fi must have its own setting field"
    );
}

#[test]
fn children_of_returns_direct_extenders() {
    let temp = tempfile::tempdir().expect("create temp dir");
    write_schema(temp.path(), "book", "");
    write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);
    write_schema(temp.path(), "memoir", r#"extends = ["book"]"#);

    let service = SchemaService::resolve(temp.path()).expect("registry loads");

    let children = service.schema_children_of("book");
    let names: Vec<&str> = children.iter().map(|s| s.schema_name()).collect();
    assert_eq!(names, vec!["sci_fi", "memoir"]);
}

#[test]
fn descendants_of_returns_transitive_extenders() {
    let temp = tempfile::tempdir().expect("create temp dir");
    write_schema(temp.path(), "thing", "");
    write_schema(temp.path(), "book", r#"extends = ["thing"]"#);
    write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

    let service = SchemaService::resolve(temp.path()).expect("registry loads");

    let descendants = service.schema_descendants_of("thing");
    let names: Vec<&str> =
        descendants.iter().map(|s| s.schema_name()).collect();
    assert_eq!(names, vec!["book", "sci_fi"]);
}

#[test]
fn matches_includes_transitive_subclasses() {
    let temp = tempfile::tempdir().expect("create temp dir");
    write_schema(temp.path(), "book", "");
    write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

    let service = SchemaService::resolve(temp.path()).expect("registry loads");

    let matches = service.schema_matches(&["book".to_owned()]);
    assert!(matches.contains("book"));
    assert!(matches.contains("sci_fi"));
}
