//! Proves schema field inheritance works end-to-end through the
//! `test-utils`-gated public surface alone. No crate-internal imports.
//!
//! `src/schema/service.rs` unit-tests resolution internals. This is the only
//! test proving that field inheritance works when called only through
//! `SchemaService::new` + `get` + `field`.

use traces_pkm::{SchemaService, TestProject};

#[test]
fn child_schema_inherits_parent_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let project = TestProject::empty(temp.path());
    project.write_schema(
        "book",
        r#"
        [fields.status]
        type = "select"
        values = ["draft", "done"]
        "#,
    );
    project.write_schema("sci_fi", r#"extends = ["book"]"#);

    let service = SchemaService::new(&project.root().join(".traces/schemas"))?;
    let sci_fi = service
        .get("sci_fi")
        .ok_or_else(|| std::io::Error::other("sci_fi resolved"))?;
    if sci_fi.field("status").is_none() {
        return Err(std::io::Error::other(
            "sci_fi must inherit status from book",
        )
        .into());
    }
    Ok(())
}

#[test]
fn parent_fields_override_is_not_lost_when_child_adds_own_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let project = TestProject::empty(temp.path());
    project.write_schema(
        "book",
        r#"
        [fields.status]
        type = "select"
        values = ["draft", "done"]
        "#,
    );
    project.write_schema(
        "sci_fi",
        r#"
        extends = ["book"]

        [fields.setting]
        type = "input"
        "#,
    );

    let service = SchemaService::new(&project.root().join(".traces/schemas"))?;
    let sci_fi = service
        .get("sci_fi")
        .ok_or_else(|| std::io::Error::other("sci_fi resolved"))?;
    if sci_fi.field("status").is_none() {
        return Err(std::io::Error::other(
            "sci_fi must still inherit status after adding own field",
        )
        .into());
    }
    if sci_fi.field("setting").is_none() {
        return Err(std::io::Error::other(
            "sci_fi must have its own setting field",
        )
        .into());
    }
    Ok(())
}

#[test]
fn children_of_returns_direct_extenders()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let project = TestProject::empty(temp.path());
    project.write_schema("book", "");
    project.write_schema("sci_fi", r#"extends = ["book"]"#);
    project.write_schema("memoir", r#"extends = ["book"]"#);

    let service = SchemaService::new(&project.root().join(".traces/schemas"))?;

    let children = service.children_of("book");
    let names: Vec<&str> = children.iter().map(|s| s.name()).collect();
    if names != ["memoir", "sci_fi"] {
        return Err(std::io::Error::other(format!(
            "direct children mismatch: {names:?}"
        ))
        .into());
    }
    Ok(())
}

#[test]
fn descendants_of_returns_transitive_extenders()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let project = TestProject::empty(temp.path());
    project.write_schema("thing", "");
    project.write_schema("book", r#"extends = ["thing"]"#);
    project.write_schema("sci_fi", r#"extends = ["book"]"#);

    let service = SchemaService::new(&project.root().join(".traces/schemas"))?;

    let descendants = service.descendants_of("thing");
    let names: Vec<&str> = descendants.iter().map(|s| s.name()).collect();
    if names != ["book", "sci_fi"] {
        return Err(std::io::Error::other(format!(
            "descendants mismatch: {names:?}"
        ))
        .into());
    }
    Ok(())
}

#[test]
fn matches_includes_transitive_subclasses()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let project = TestProject::empty(temp.path());
    project.write_schema("book", "");
    project.write_schema("sci_fi", r#"extends = ["book"]"#);

    let service = SchemaService::new(&project.root().join(".traces/schemas"))?;

    let matches = service.matches(&["book".to_owned()]);
    if !matches.contains("book") || !matches.contains("sci_fi") {
        return Err(std::io::Error::other(format!(
            "transitive matches missing: {matches:?}"
        ))
        .into());
    }
    Ok(())
}
