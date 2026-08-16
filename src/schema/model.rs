//! Store the resolved Schema domain type.
//!
//! [`Schema`] holds effective [`SchemaFieldDef`]s after inheritance,
//! `excludes`, and `$ref` are applied, plus its transitive `extends`
//! ancestors, direct extenders (`children`), and transitive extenders
//! (`descendants`). Field construction lives in [`super::fields`]; this module
//! is the plain data type and its own lookups.
//!
//! Construction stays `pub(super)`: only [`super::service`] builds these; the
//! rest of the crate reads them through `pub(crate)` accessors.

use std::collections::{BTreeMap, BTreeSet};

use super::{fields::SchemaFieldDef, name::SchemaName};
use crate::field::{FieldName, closest_match};

/// Store one Schema's effective [`SchemaFieldDef`]s and its position in the
/// `extends` DAG.
///
/// Fields are resolved after inheritance, `excludes`, and `$ref` application.
#[derive(Clone, Debug, PartialEq)]
pub struct Schema {
    name: SchemaName,
    fields: BTreeMap<FieldName, SchemaFieldDef>,
    /// Transitive `extends` targets, filtered to targets that resolved (a
    /// missing target never reaches here; see
    /// [`super::error::SchemaWarning::MissingExtendsTarget`]).
    ancestors: BTreeSet<SchemaName>,
    /// Schemas that directly `extends` this one. Populated once, during
    /// [`super::service::SchemaService::resolve`], from
    /// [`super::graph::SchemaGraph::children_by_name`] — not an O(n)
    /// per-lookup scan.
    children: BTreeSet<SchemaName>,
    /// Every Schema that transitively `extends` this one (its `children`,
    /// their `children`, and so on). Populated the same way as `children`, via
    /// [`super::graph::SchemaGraph::descendants_by_name`].
    descendants: BTreeSet<SchemaName>,
}

impl Schema {
    /// Build a resolved Schema from its merged fields and ancestors. `children`
    /// and `descendants` start empty; [`Self::set_hierarchy`] fills them in
    /// once the whole `extends` DAG is confirmed acyclic.
    pub(super) fn new(
        name: SchemaName,
        fields: BTreeMap<FieldName, SchemaFieldDef>,
        ancestors: BTreeSet<SchemaName>,
    ) -> Self {
        Self {
            name,
            fields,
            ancestors,
            children: BTreeSet::new(),
            descendants: BTreeSet::new(),
        }
    }

    /// Set this Schema's direct extenders and transitive extenders, computed
    /// in bulk over the whole `extends` DAG once every Schema resolved.
    pub(super) fn set_hierarchy(
        &mut self,
        children: BTreeSet<SchemaName>,
        descendants: BTreeSet<SchemaName>,
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
    pub(crate) fn fields(&self) -> &BTreeMap<FieldName, SchemaFieldDef> {
        &self.fields
    }

    /// Return the named Field Definition, or `None` if it does not resolve for
    /// this Schema.
    #[inline]
    #[must_use]
    pub(crate) fn field(&self, name: &str) -> Option<&SchemaFieldDef> {
        self.fields.get(name)
    }

    /// Return this Schema's transitive `extends` ancestors, used by
    /// [`super::service`]'s per-schema build step to accumulate a child's own
    /// ancestor set from its parents'.
    #[inline]
    #[must_use]
    pub(super) fn ancestors(&self) -> &BTreeSet<SchemaName> {
        &self.ancestors
    }

    /// Return the Schemas that directly `extends` this one.
    #[inline]
    #[must_use]
    pub(super) fn children(&self) -> &BTreeSet<SchemaName> {
        &self.children
    }

    /// Return every Schema that transitively `extends` this one.
    #[inline]
    #[must_use]
    pub(super) fn descendants(&self) -> &BTreeSet<SchemaName> {
        &self.descendants
    }

    /// Test whether this Schema is-a class name.
    ///
    /// The `ancestors` set includes all transitive `extends` targets that
    /// resolved during [`super::service::SchemaService::resolve`], so this
    /// check covers indirect inheritance chains, such as `sci_fi` to `book` to
    /// `thing`.
    ///
    /// # Examples
    ///
    /// A `sci_fi` Schema that transitively extends `book`:
    ///
    /// - `sci_fi.is_a("sci_fi")` returns `true` for itself.
    /// - `sci_fi.is_a("book")` returns `true` for an ancestor.
    /// - `sci_fi.is_a("movie")` returns `false` for an unrelated class.
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

    /// Suggest the field name in this Schema that best matches `field`, for a
    /// template adapter's `did you mean` hint on an unknown-field render
    /// error. Never consulted on a successful field lookup, which stays
    /// exact.
    ///
    /// Prefers a canonical ([`crate::field::FieldKey`]) match over an
    /// edit-distance match. Suggests nothing when `field` itself fails
    /// [`crate::field::FieldKey`] validation, when no field canonically or
    /// approximately matches, or when more than one field canonically matches
    /// (Schema resolution already rejects two fields sharing a canonical form
    /// within one Schema, so this last case is defensive).
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

    /// Find the field name in this Schema with the smallest edit distance to
    /// `field`, for [`Self::suggest_field`]'s fallback when no field
    /// canonically matches.
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
        use std::collections::{BTreeMap, BTreeSet};

        use pretty_assertions::assert_eq;

        use super::super::*;
        use crate::schema::fields::SchemaFieldType;

        fn field(kind: SchemaFieldType) -> SchemaFieldDef {
            SchemaFieldDef::for_test(kind, false, false)
        }

        #[test]
        fn new_stores_the_given_name_fields_and_ancestors() {
            let mut fields = BTreeMap::new();
            fields.insert(
                FieldName::try_from("title").expect("valid test field name"),
                field(SchemaFieldType::Input),
            );
            let mut ancestors = BTreeSet::new();
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
                BTreeMap::new(),
                BTreeSet::new(),
            );
            let mut children = BTreeSet::new();
            children.insert(SchemaName::from("book"));
            let mut descendants = BTreeSet::new();
            descendants.insert(SchemaName::from("book"));
            descendants.insert(SchemaName::from("sci_fi"));

            schema.set_hierarchy(children.clone(), descendants.clone());

            assert_eq!(schema.children(), &children);
            assert_eq!(schema.descendants(), &descendants);
        }

        #[test]
        fn field_returns_the_named_definition_when_present() {
            let mut fields = BTreeMap::new();
            fields.insert(
                FieldName::try_from("title").expect("valid test field name"),
                field(SchemaFieldType::Input),
            );
            let schema =
                Schema::new(SchemaName::from("book"), fields, BTreeSet::new());

            assert_eq!(
                schema.field("title"),
                Some(&field(SchemaFieldType::Input))
            );
        }

        #[test]
        fn field_returns_none_when_the_name_is_absent() {
            let schema = Schema::new(
                SchemaName::from("book"),
                BTreeMap::new(),
                BTreeSet::new(),
            );

            assert_eq!(schema.field("missing"), None);
        }

        #[test]
        fn is_a_matches_when_queried_equals_its_own_name() {
            let schema = Schema::new(
                SchemaName::from("book"),
                BTreeMap::new(),
                BTreeSet::new(),
            );

            assert!(schema.is_a("book"));
        }

        #[test]
        fn is_a_matches_when_queried_is_a_transitive_ancestor() {
            let mut ancestors = BTreeSet::new();
            ancestors.insert(SchemaName::from("thing"));
            let schema = Schema::new(
                SchemaName::from("book"),
                BTreeMap::new(),
                ancestors,
            );

            assert!(schema.is_a("thing"));
        }

        #[test]
        fn is_a_does_not_match_an_unrelated_name() {
            let schema = Schema::new(
                SchemaName::from("book"),
                BTreeMap::new(),
                BTreeSet::new(),
            );

            assert!(!schema.is_a("movie"));
        }
    }

    mod suggest_field {
        use std::collections::{BTreeMap, BTreeSet};

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
                .collect::<BTreeMap<_, _>>();
            Schema::new(SchemaName::from("book"), fields, BTreeSet::new())
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
