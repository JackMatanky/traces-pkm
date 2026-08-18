//! The resolved [`Schema`] domain type and its lookups.

use indexmap::{IndexMap, IndexSet};

use super::{SchemaName, fields::SchemaFieldDef};
use crate::field::{FieldName, closest_match};

/// A resolved Schema with its effective field definitions and position in
/// the `extends` DAG.
#[derive(Clone, Debug, PartialEq)]
pub struct Schema {
    name: SchemaName,
    fields: IndexMap<FieldName, SchemaFieldDef>,
    /// Transitive `extends` targets that resolved.
    ancestors: IndexSet<SchemaName>,
    /// Schemas that directly `extends` this one.
    children: IndexSet<SchemaName>,
    /// Every Schema that transitively `extends` this one.
    descendants: IndexSet<SchemaName>,
}

impl Schema {
    /// Build a resolved Schema from its merged fields and ancestors.
    ///
    /// Hierarchy links ([`children`](Self::children),
    /// [`descendants`](Self::descendants)) are empty until
    /// [`set_hierarchy`](Self::set_hierarchy) is called after full DAG
    /// resolution.
    pub(super) fn new(
        name: SchemaName,
        fields: IndexMap<FieldName, SchemaFieldDef>,
        ancestors: IndexSet<SchemaName>,
    ) -> Self {
        Self {
            name,
            fields,
            ancestors,
            children: IndexSet::new(),
            descendants: IndexSet::new(),
        }
    }

    /// Set this Schema's direct and transitive extenders.
    pub(super) fn set_hierarchy(
        &mut self,
        children: IndexSet<SchemaName>,
        descendants: IndexSet<SchemaName>,
    ) {
        self.children = children;
        self.descendants = descendants;
    }

    /// Return the Schema name from the source file stem.
    #[inline]
    #[must_use]
    pub(crate) fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Return this Schema's effective Field Definitions, keyed by name.
    #[inline]
    #[must_use]
    pub(crate) fn fields(&self) -> &IndexMap<FieldName, SchemaFieldDef> {
        &self.fields
    }

    /// Return the named Field Definition, or `None` if it does not resolve for
    /// this Schema.
    #[inline]
    #[must_use]
    pub(crate) fn field(&self, name: &str) -> Option<&SchemaFieldDef> {
        self.fields.get(name)
    }

    /// Return this Schema's transitive `extends` ancestors.
    #[inline]
    #[must_use]
    pub(super) fn ancestors(&self) -> &IndexSet<SchemaName> {
        &self.ancestors
    }

    /// Return the Schemas that directly `extends` this one.
    #[inline]
    #[must_use]
    pub(super) fn children(&self) -> &IndexSet<SchemaName> {
        &self.children
    }

    /// Return every Schema that transitively `extends` this one.
    #[inline]
    #[must_use]
    pub(super) fn descendants(&self) -> &IndexSet<SchemaName> {
        &self.descendants
    }

    /// Test whether this Schema is a `class` name.
    ///
    /// # Examples
    ///
    /// A `sci_fi` Schema that transitively extends `book`:
    ///
    /// - `sci_fi.is_a("sci_fi")` → `true` (itself)
    /// - `sci_fi.is_a("book")` → `true` (ancestor)
    /// - `sci_fi.is_a("movie")` → `false` (unrelated)
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "is-a matching for class queries now goes through the \
                      bulk SchemaService::matches()/descendants(), which read \
                      the precomputed descendants set instead of calling \
                      is_a() per Schema; kept as a per-instance domain check \
                      for API completeness and direct is-a assertions"
        )
    )]
    pub(crate) fn is_a(&self, class: &str) -> bool {
        self.name.as_str() == class || self.ancestors.contains(class)
    }

    /// Suggest the field name that best matches `field`, for a template
    /// adapter's "did you mean" hint on an unknown-field error.
    ///
    /// Prefers a canonical ([`FieldKey`]) match over edit distance. Returns
    /// `None` when no candidate matches or more than one field canonically
    /// matches.
    ///
    /// [`FieldKey`]: crate::field::FieldKey
    #[must_use]
    pub(crate) fn suggest_field(&self, field: &str) -> Option<&str> {
        let input_key = crate::field::FieldKey::try_from(field).ok()?;
        let mut canonical_matches =
            self.fields.keys().filter(|name| input_key.is_match(name.as_str()));
        match (canonical_matches.next(), canonical_matches.next()) {
            (Some(only), None) => Some(only.as_str()),
            (Some(_), Some(_)) => None,
            (None, _) => self.closest_field_name(field),
        }
    }

    /// Find the field name with the smallest edit distance to `field`.
    fn closest_field_name(&self, field: &str) -> Option<&str> {
        closest_match(
            self.fields.keys().map(|name| (name.as_str(), name.as_str())),
            field,
        )
    }
}

#[cfg(test)]
mod tests {
    mod schema {
        use indexmap::{IndexMap, IndexSet};
        use pretty_assertions::assert_eq;

        use super::super::*;
        use crate::schema::fields::SchemaFieldType;

        fn field(kind: SchemaFieldType) -> SchemaFieldDef {
            SchemaFieldDef::for_test(kind, false, false)
        }

        #[test]
        fn new_stores_the_given_name_fields_and_ancestors() {
            let mut fields = IndexMap::new();
            fields.insert(
                FieldName::try_from("title").expect("valid test field name"),
                field(SchemaFieldType::Input),
            );
            let mut ancestors = IndexSet::new();
            ancestors.insert(SchemaName::from("thing"));

            let schema = Schema::new(
                SchemaName::from("book"),
                fields.clone(),
                ancestors.clone(),
            );

            assert_eq!(schema.name(), "book");
            assert_eq!(schema.fields(), &fields);
            assert_eq!(schema.ancestors(), &ancestors);
            assert!(schema.children().is_empty());
            assert!(schema.descendants().is_empty());
        }

        #[test]
        fn set_hierarchy_stores_the_given_children_and_descendants() {
            let mut schema = Schema::new(
                SchemaName::from("thing"),
                IndexMap::new(),
                IndexSet::new(),
            );
            let mut children = IndexSet::new();
            children.insert(SchemaName::from("book"));
            let mut descendants = IndexSet::new();
            descendants.insert(SchemaName::from("book"));
            descendants.insert(SchemaName::from("sci_fi"));

            schema.set_hierarchy(children.clone(), descendants.clone());

            assert_eq!(schema.children(), &children);
            assert_eq!(schema.descendants(), &descendants);
        }

        #[test]
        fn field_returns_the_named_definition_when_present() {
            let mut fields = IndexMap::new();
            fields.insert(
                FieldName::try_from("title").expect("valid test field name"),
                field(SchemaFieldType::Input),
            );
            let schema =
                Schema::new(SchemaName::from("book"), fields, IndexSet::new());

            assert_eq!(
                schema.field("title"),
                Some(&field(SchemaFieldType::Input))
            );
        }

        #[test]
        fn field_returns_none_when_the_name_is_absent() {
            let schema = Schema::new(
                SchemaName::from("book"),
                IndexMap::new(),
                IndexSet::new(),
            );

            assert_eq!(schema.field("missing"), None);
        }

        #[test]
        fn is_a_matches_when_queried_equals_its_own_name() {
            let schema = Schema::new(
                SchemaName::from("book"),
                IndexMap::new(),
                IndexSet::new(),
            );

            assert!(schema.is_a("book"));
        }

        #[test]
        fn is_a_matches_when_queried_is_a_transitive_ancestor() {
            let mut ancestors = IndexSet::new();
            ancestors.insert(SchemaName::from("thing"));
            let schema = Schema::new(
                SchemaName::from("book"),
                IndexMap::new(),
                ancestors,
            );

            assert!(schema.is_a("thing"));
        }

        #[test]
        fn is_a_does_not_match_an_unrelated_name() {
            let schema = Schema::new(
                SchemaName::from("book"),
                IndexMap::new(),
                IndexSet::new(),
            );

            assert!(!schema.is_a("movie"));
        }
    }

    mod suggest_field {
        use indexmap::{IndexMap, IndexSet};
        use pretty_assertions::assert_eq;

        use super::super::*;
        use crate::schema::fields::SchemaFieldType;

        fn schema_with_fields(names: &[&str]) -> Schema {
            let fields = names
                .iter()
                .map(|&name| {
                    (
                        FieldName::try_from(name)
                            .expect("valid test field name"),
                        SchemaFieldDef::for_test(
                            SchemaFieldType::Input,
                            false,
                            false,
                        ),
                    )
                })
                .collect::<IndexMap<_, _>>();
            Schema::new(SchemaName::from("book"), fields, IndexSet::new())
        }

        #[test]
        fn suggests_the_exact_canonical_match_for_a_casing_typo() {
            let schema = schema_with_fields(&["status"]);

            assert_eq!(schema.suggest_field("Status"), Some("status"));
        }

        #[test]
        fn suggests_the_closest_field_for_an_edit_distance_typo() {
            let schema = schema_with_fields(&["status"]);

            assert_eq!(schema.suggest_field("statu"), Some("status"));
        }

        #[test]
        fn suggests_nothing_for_a_field_with_no_close_candidate() {
            let schema = schema_with_fields(&["status"]);

            assert_eq!(schema.suggest_field("completely_unrelated"), None);
        }

        #[test]
        fn suggests_nothing_when_two_fields_canonically_match() {
            // Defensive: Schema resolution already rejects two fields sharing
            // a canonical form within one Schema (`SchemaError::
            // AmbiguousFieldName`), so this never happens for a real
            // resolved Schema — this test only proves `suggest_field` itself
            // degrades safely if it ever did.
            let schema = schema_with_fields(&["due date", "Due-Date"]);

            assert_eq!(schema.suggest_field("due-date"), None);
        }
    }
}
