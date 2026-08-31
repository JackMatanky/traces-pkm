//! Proves schema field inheritance works end-to-end through the
//! `test-utils`-gated public surface alone. No crate-internal imports.
//!
//! `src/schema/service.rs` unit-tests resolution internals. This is the only
//! test proving that field inheritance works when called only through
//! `SchemaService::new` + `get` + `field`.

use traces_pkm::SchemaService;

fn write_schema(
    dir: &std::path::Path,
    name: &str,
    toml: &str,
) -> std::io::Result<()> {
    std::fs::write(dir.join(format!("{name}.toml")), toml)
}

#[test]
fn child_schema_inherits_parent_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    write_schema(
        temp.path(),
        "book",
        r#"
        [fields.status]
        type = "select"
        values = ["draft", "done"]
        "#,
    )?;
    write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#)?;

    let service = SchemaService::new(temp.path())?;
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
    write_schema(
        temp.path(),
        "book",
        r#"
        [fields.status]
        type = "select"
        values = ["draft", "done"]
        "#,
    )?;
    write_schema(
        temp.path(),
        "sci_fi",
        r#"
        extends = ["book"]

        [fields.setting]
        type = "input"
        "#,
    )?;

    let service = SchemaService::new(temp.path())?;
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
    write_schema(temp.path(), "book", "")?;
    write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#)?;
    write_schema(temp.path(), "memoir", r#"extends = ["book"]"#)?;

    let service = SchemaService::new(temp.path())?;

    let children = service.children_of("book");
    let names: Vec<&str> = children.iter().map(|s| s.name()).collect();
    if names != ["sci_fi", "memoir"] {
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
    write_schema(temp.path(), "thing", "")?;
    write_schema(temp.path(), "book", r#"extends = ["thing"]"#)?;
    write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#)?;

    let service = SchemaService::new(temp.path())?;

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
    write_schema(temp.path(), "book", "")?;
    write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#)?;

    let service = SchemaService::new(temp.path())?;

    let matches = service.matches(&["book".to_owned()]);
    if !matches.contains("book") || !matches.contains("sci_fi") {
        return Err(std::io::Error::other(format!(
            "transitive matches missing: {matches:?}"
        ))
        .into());
    }
    Ok(())
}
