//! `select` field type definition, entry type, and parsing.
//!
//! Parses the `values` attribute for `select` fields into
//! [`SchemaSelectFieldEntry`]s.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::Arc,
};

use indexmap::IndexMap;
use thiserror::Error;

use super::{
    SchemaFieldType, error::SchemaFieldParserError, parser::SchemaFieldParser,
};
use crate::{
    field::FieldValue,
    path::{PathError, RootConfinedPath},
};

type CachedSelectValues = Arc<Vec<FieldValue>>;
type ValuesFileCacheMap = IndexMap<PathBuf, CachedSelectValues>;

/// Resolved `select` field options.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SchemaSelectField {
    values: Vec<SchemaSelectFieldEntry>,
}

impl SchemaSelectField {
    /// Return the selectable entries.
    #[inline]
    #[must_use]
    pub(crate) fn values(&self) -> &[SchemaSelectFieldEntry] {
        &self.values
    }

    /// Build an instance for tests.
    #[cfg(test)]
    pub(crate) const fn for_test(values: Vec<SchemaSelectFieldEntry>) -> Self {
        Self {
            values,
        }
    }

    /// Parse `options` against `select`'s `values` attribute, merging with
    /// `base` when present.
    ///
    /// # Arguments
    ///
    /// * `parser`: pre-constructed parser for this field.
    /// * `options`: raw key-value pairs from the TOML definition.
    /// * `base`: inherited field type to fall back to for unset keys.
    pub(super) fn parse(
        values_cache: &SelectValuesFileCache,
        parser: &mut SchemaFieldParser<'_>,
        options: &IndexMap<String, FieldValue>,
        base: Option<&Self>,
    ) -> SchemaFieldType {
        let err_count_before = parser.error_count();
        let raw = parser.raw_value(options, "values");
        let values = match raw {
            None => base.map_or_else(Vec::new, |b| b.values.clone()),
            Some(FieldValue::List(items)) => {
                if items.is_empty() {
                    base.map_or_else(Vec::new, |b| b.values.clone())
                } else if items
                    .iter()
                    .all(|item| matches!(item, FieldValue::String(_)))
                {
                    items
                        .iter()
                        .filter_map(|item| match item {
                            FieldValue::String(s) => {
                                Some(SchemaSelectFieldEntry::literal(s.clone()))
                            }
                            _ => None,
                        })
                        .collect()
                } else if items
                    .iter()
                    .all(|item| matches!(item, FieldValue::Object(_)))
                {
                    parse_inline_value_objects(parser, items)
                } else {
                    parser.push_error(SchemaFieldParserError::TypeMismatch {
                        address: parser.address_owned(),
                        kind: parser.kind(),
                        key: "values".to_owned(),
                        value: format!("{items:?}"),
                        expected: "a list of strings, a list of value \
                                   objects, or a file subtable",
                    });
                    base.map_or_else(Vec::new, |b| b.values.clone())
                }
            }
            Some(FieldValue::Object(subtable)) => {
                parse_file_subtable(values_cache, parser, subtable, base)
            }
            Some(other) => {
                parser.push_error(SchemaFieldParserError::TypeMismatch {
                    address: parser.address_owned(),
                    kind: parser.kind(),
                    key: "values".to_owned(),
                    value: format!("{other:?}"),
                    expected: "a list of strings, a list of value objects, or \
                               a file subtable",
                });
                base.map_or_else(Vec::new, |b| b.values.clone())
            }
        };

        let values = if parser.error_count() > err_count_before {
            base.map_or_else(Vec::new, |b| b.values.clone())
        } else {
            values
        };

        SchemaFieldType::Select(Arc::new(Self {
            values,
        }))
    }
}

/// Rendered as a plain string when `label` equals `value` and `extra` is
/// empty, otherwise as `{value, label, ...extra}`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SchemaSelectFieldEntry {
    value: FieldValue,
    label: FieldValue,
    extra: IndexMap<String, FieldValue>,
}

impl SchemaSelectFieldEntry {
    /// Build a new entry with explicit value, label, and extra passthrough
    /// keys.
    pub(crate) const fn new(
        value: FieldValue,
        label: FieldValue,
        extra: IndexMap<String, FieldValue>,
    ) -> Self {
        Self {
            value,
            label,
            extra,
        }
    }

    /// Build a literal entry where `label` equals `value` and `extra` is empty.
    pub(crate) fn literal(value: String) -> Self {
        Self {
            value: FieldValue::String(value.clone()),
            label: FieldValue::String(value),
            extra: IndexMap::new(),
        }
    }

    /// Return this entry's value.
    #[inline]
    #[must_use]
    pub(crate) const fn value(&self) -> &FieldValue {
        &self.value
    }

    /// Return this entry's display label.
    #[inline]
    #[must_use]
    pub(crate) const fn label(&self) -> &FieldValue {
        &self.label
    }

    /// Return this entry's passthrough keys beyond `value`/`label`.
    #[inline]
    #[must_use]
    pub(crate) const fn extra(&self) -> &IndexMap<String, FieldValue> {
        &self.extra
    }

    #[cfg(test)]
    /// Build an entry with a distinct label and extra passthrough keys.
    pub(crate) fn with_label(
        value: String,
        label: String,
        extra: IndexMap<String, FieldValue>,
    ) -> Self {
        Self {
            value: FieldValue::String(value),
            label: FieldValue::String(label),
            extra,
        }
    }
}

/// Confined memoizing cache for external `select`/`multi` field values files.
///
/// Transient to `SchemaService::new` construction: loaded statically once per
/// distinct confined path during schema building, then dropped.
#[derive(Debug)]
pub(crate) struct SelectValuesFileCache {
    dir: PathBuf,
    cache: RefCell<ValuesFileCacheMap>,
}

impl SelectValuesFileCache {
    /// Creates a new cache rooted at `dir`.
    #[must_use]
    pub(crate) fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            cache: RefCell::new(IndexMap::new()),
        }
    }

    /// Creates an empty cache for unit tests.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn for_test() -> Self {
        Self {
            dir: PathBuf::new(),
            cache: RefCell::new(IndexMap::new()),
        }
    }

    /// Confines `relative_path` to the schema directory, parses
    /// `.toml`/`.json`, and returns the memoized list of `entries`.
    ///
    /// # Errors
    ///
    /// Returns [`SelectValuesFileError`] if confinement fails, I/O fails,
    /// parsing fails, or `entries` is missing.
    pub(crate) fn load(
        &self,
        relative_path: &str,
    ) -> Result<Arc<Vec<FieldValue>>, SelectValuesFileError> {
        let path = Path::new(relative_path);
        let confined = RootConfinedPath::parse(&self.dir, path)?;
        let confined_path = confined.into_path_buf();

        let ext =
            confined_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "toml" && ext != "json" {
            return Err(SelectValuesFileError::BadExtension(
                relative_path.to_owned(),
            ));
        }

        if let Some(entries) = self.cache.borrow().get(&confined_path) {
            return Ok(Arc::clone(entries));
        }

        let content = std::fs::read_to_string(&confined_path)?;
        let val: crate::schema::raw::RawSchemaSelectFieldValues = if ext
            == "toml"
        {
            toml::from_str(&content)
                .map_err(|e| SelectValuesFileError::ParseToml(e.to_string()))?
        } else if ext == "json" {
            serde_json::from_str(&content)
                .map_err(|e| SelectValuesFileError::ParseJson(e.to_string()))?
        } else {
            return Err(SelectValuesFileError::BadExtension(
                relative_path.to_owned(),
            ));
        };

        let entries =
            val.entries.ok_or(SelectValuesFileError::MissingEntries)?;
        let domain_entries =
            entries.into_iter().map(FieldValue::from).collect::<Vec<_>>();

        let arc_entries = Arc::new(domain_entries);
        self.cache.borrow_mut().insert(confined_path, Arc::clone(&arc_entries));
        Ok(arc_entries)
    }
}

/// Errors encountered while loading or parsing an external values file.
#[derive(Debug, Error)]
pub(crate) enum SelectValuesFileError {
    /// The values file path escaped the schema directory or contained unsafe
    /// components.
    #[error(transparent)]
    Confinement(#[from] PathError),

    /// An I/O error occurred while reading the values file.
    #[error("failed to read values file: {0}")]
    Io(#[from] std::io::Error),

    /// A TOML values file failed to parse.
    #[error("failed to parse TOML values file: {0}")]
    ParseToml(String),

    /// A JSON values file failed to parse.
    #[error("failed to parse JSON values file: {0}")]
    ParseJson(String),

    /// The values file path has an unsupported extension.
    #[error("unsupported values file extension {0:?} (must be .toml or .json)")]
    BadExtension(String),

    /// The values file is missing the top-level `entries` list.
    #[error("values file missing top-level 'entries' list")]
    MissingEntries,
}

fn extract_selector<'a>(
    subtable: &'a IndexMap<String, FieldValue>,
    key: &str,
    default: &'a str,
    parser: &mut SchemaFieldParser<'_>,
) -> &'a str {
    match subtable.get(key) {
        Some(FieldValue::String(s)) => s.as_str(),
        Some(other) => {
            parser.push_error(SchemaFieldParserError::TypeMismatch {
                address: parser.address_owned(),
                kind: parser.kind(),
                key: key.to_owned(),
                value: format!("{other:?}"),
                expected: "a string",
            });
            default
        }
        None => default,
    }
}

fn extract_optional_selector<'a>(
    subtable: &'a IndexMap<String, FieldValue>,
    key: &str,
    parser: &mut SchemaFieldParser<'_>,
) -> Option<&'a str> {
    match subtable.get(key) {
        Some(FieldValue::String(s)) => Some(s.as_str()),
        Some(other) => {
            parser.push_error(SchemaFieldParserError::TypeMismatch {
                address: parser.address_owned(),
                kind: parser.kind(),
                key: key.to_owned(),
                value: format!("{other:?}"),
                expected: "a string",
            });
            None
        }
        None => None,
    }
}

fn entry_string_field(
    map: &IndexMap<String, FieldValue>,
    key: &str,
    selector_name: &'static str,
    parser: &mut SchemaFieldParser<'_>,
) -> Option<String> {
    match map.get(key) {
        Some(FieldValue::String(s)) => Some(s.clone()),
        Some(other) => {
            parser.push_error(SchemaFieldParserError::TypeMismatch {
                address: parser.address_owned(),
                kind: parser.kind(),
                key: key.to_owned(),
                value: format!("{other:?}"),
                expected: "a string",
            });
            None
        }
        None => {
            parser.push_error(SchemaFieldParserError::SelectorMissingKey {
                address: parser.address_owned(),
                selector: selector_name,
                key: key.to_owned(),
            });
            None
        }
    }
}

fn parse_object_entries(
    parser: &mut SchemaFieldParser<'_>,
    items: &[FieldValue],
    value_selector: &str,
    label_selector: Option<&str>,
    order_selector: Option<&str>,
) -> Vec<SchemaSelectFieldEntry> {
    let mut entries_with_order: Vec<(SchemaSelectFieldEntry, Option<f64>)> =
        Vec::with_capacity(items.len());

    for item in items {
        let FieldValue::Object(map) = item else {
            continue;
        };

        let value_str =
            entry_string_field(map, value_selector, "value", parser)
                .unwrap_or_default();
        let value = FieldValue::String(value_str);

        let label = if let Some(lbl_sel) = label_selector {
            entry_string_field(map, lbl_sel, "label", parser)
                .map_or_else(|| value.clone(), FieldValue::String)
        } else {
            value.clone()
        };

        let order = order_selector.and_then(|ord_sel| {
            if let Some(val) = map.get(ord_sel) {
                val.as_f64().or_else(|| {
                    parser.push_error(SchemaFieldParserError::TypeMismatch {
                        address: parser.address_owned(),
                        kind: parser.kind(),
                        key: ord_sel.to_owned(),
                        value: format!("{val:?}"),
                        expected: "a number",
                    });
                    None
                })
            } else {
                parser.push_error(SchemaFieldParserError::SelectorMissingKey {
                    address: parser.address_owned(),
                    selector: "order",
                    key: ord_sel.to_owned(),
                });
                None
            }
        });

        let mut extra = IndexMap::new();
        for (k, v) in map {
            if k != value_selector
                && (label_selector.is_none()
                    || label_selector != Some(k.as_str()))
            {
                extra.insert(k.clone(), v.clone());
            }
        }

        entries_with_order
            .push((SchemaSelectFieldEntry::new(value, label, extra), order));
    }

    if order_selector.is_some()
        && entries_with_order.iter().all(|(_, o)| o.is_some())
    {
        entries_with_order.sort_by(|(_, a_ord), (_, b_ord)| {
            match (a_ord, b_ord) {
                (Some(a), Some(b)) => a.total_cmp(b),
                _ => std::cmp::Ordering::Equal,
            }
        });
    }

    entries_with_order.into_iter().map(|(entry, _)| entry).collect()
}

fn parse_inline_value_objects(
    parser: &mut SchemaFieldParser<'_>,
    items: &[FieldValue],
) -> Vec<SchemaSelectFieldEntry> {
    let has_label_on_any = items.iter().any(|item| {
        matches!(item, FieldValue::Object(map) if map.contains_key("label"))
    });
    let has_label_on_all = items.iter().all(|item| {
        matches!(item, FieldValue::Object(map) if map.contains_key("label"))
    });
    if has_label_on_any && !has_label_on_all {
        parser.push_error(SchemaFieldParserError::SelectorMissingKey {
            address: parser.address_owned(),
            selector: "label",
            key: "label".to_owned(),
        });
    }

    let has_order_on_any = items.iter().any(|item| {
        matches!(item, FieldValue::Object(map) if map.contains_key("order"))
    });
    let has_order_on_all = items.iter().all(|item| {
        matches!(item, FieldValue::Object(map) if map.contains_key("order"))
    });
    if has_order_on_any && !has_order_on_all {
        parser.push_error(SchemaFieldParserError::SelectorMissingKey {
            address: parser.address_owned(),
            selector: "order",
            key: "order".to_owned(),
        });
    }

    let label_sel = if has_label_on_all {
        Some("label")
    } else {
        None
    };
    let order_sel = if has_order_on_all {
        Some("order")
    } else {
        None
    };

    parse_object_entries(parser, items, "value", label_sel, order_sel)
}

fn parse_file_bare_entries(
    parser: &mut SchemaFieldParser<'_>,
    subtable: &IndexMap<String, FieldValue>,
    loaded_entries: &[FieldValue],
) -> Vec<SchemaSelectFieldEntry> {
    if subtable.contains_key("value") {
        parser.push_error(SchemaFieldParserError::SelectorOnBareEntries {
            address: parser.address_owned(),
            selector: "value",
        });
    }
    if subtable.contains_key("label") {
        parser.push_error(SchemaFieldParserError::SelectorOnBareEntries {
            address: parser.address_owned(),
            selector: "label",
        });
    }
    if subtable.contains_key("order") {
        parser.push_error(SchemaFieldParserError::SelectorOnBareEntries {
            address: parser.address_owned(),
            selector: "order",
        });
    }
    loaded_entries
        .iter()
        .filter_map(|item| match item {
            FieldValue::String(s) => {
                Some(SchemaSelectFieldEntry::literal(s.clone()))
            }
            _ => None,
        })
        .collect()
}

fn parse_file_object_entries(
    parser: &mut SchemaFieldParser<'_>,
    subtable: &IndexMap<String, FieldValue>,
    loaded_entries: &[FieldValue],
) -> Vec<SchemaSelectFieldEntry> {
    let value_selector = extract_selector(subtable, "value", "value", parser);
    let label_selector = extract_optional_selector(subtable, "label", parser);
    let order_selector = extract_optional_selector(subtable, "order", parser);

    parse_object_entries(
        parser,
        loaded_entries,
        value_selector,
        label_selector,
        order_selector,
    )
}

fn parse_file_subtable(
    values_cache: &SelectValuesFileCache,
    parser: &mut SchemaFieldParser<'_>,
    subtable: &IndexMap<String, FieldValue>,
    base: Option<&SchemaSelectField>,
) -> Vec<SchemaSelectFieldEntry> {
    for key in subtable.keys() {
        if key != "path" && key != "value" && key != "label" && key != "order" {
            parser.push_error(SchemaFieldParserError::UnknownKey {
                address: parser.address_owned(),
                kind: parser.kind(),
                key: key.to_owned(),
            });
        }
    }

    let path_str = if let Some(FieldValue::String(p)) = subtable.get("path") {
        p.as_str()
    } else {
        parser.push_error(SchemaFieldParserError::TypeMismatch {
            address: parser.address_owned(),
            kind: parser.kind(),
            key: "path".to_owned(),
            value: format!("{:?}", subtable.get("path")),
            expected: "a string file path",
        });
        return base.map_or_else(Vec::new, |b| b.values.clone());
    };

    let loaded_entries = match values_cache.load(path_str) {
        Ok(entries) => entries,
        Err(SelectValuesFileError::BadExtension(_)) => {
            parser.push_error(SchemaFieldParserError::BadValueFileExtension {
                address: parser.address_owned(),
                path: path_str.to_owned(),
            });
            return base.map_or_else(Vec::new, |b| b.values.clone());
        }
        Err(SelectValuesFileError::MissingEntries) => {
            parser.push_error(
                SchemaFieldParserError::ValueFileMissingEntries {
                    address: parser.address_owned(),
                    path: path_str.to_owned(),
                },
            );
            return base.map_or_else(Vec::new, |b| b.values.clone());
        }
        Err(err) => {
            parser.push_error(SchemaFieldParserError::ValueFileLoad {
                address: parser.address_owned(),
                path: path_str.to_owned(),
                error: err.to_string(),
            });
            return base.map_or_else(Vec::new, |b| b.values.clone());
        }
    };
    if loaded_entries.is_empty() {
        return Vec::new();
    }

    if loaded_entries.iter().all(|item| matches!(item, FieldValue::String(_))) {
        parse_file_bare_entries(parser, subtable, &loaded_entries)
    } else if loaded_entries
        .iter()
        .all(|item| matches!(item, FieldValue::Object(_)))
    {
        parse_file_object_entries(parser, subtable, &loaded_entries)
    } else {
        parser.push_error(SchemaFieldParserError::TypeMismatch {
            address: parser.address_owned(),
            kind: parser.kind(),
            key: "entries".to_owned(),
            value: format!("{loaded_entries:?}"),
            expected: "a list of strings or a list of value objects",
        });
        base.map_or_else(Vec::new, |b| b.values.clone())
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::*;
    use crate::schema::fields::{
        SchemaFieldTypeTag, address::FieldAddress,
        error::SchemaFieldParserError, parser::SchemaFieldParser,
    };

    fn address() -> FieldAddress {
        FieldAddress::try_from("#book/field").expect("valid ref")
    }

    fn options(pairs: &[(&str, FieldValue)]) -> IndexMap<String, FieldValue> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    fn select_field(
        field_type: &SchemaFieldType,
    ) -> Option<&SchemaSelectField> {
        match field_type {
            SchemaFieldType::Select(def) => Some(def.as_ref()),
            _ => None,
        }
    }

    mod literal_lists {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn parses_flat_string_list_into_literal_entries() {
            let opts = options(&[(
                "values",
                FieldValue::List(vec![
                    FieldValue::String("draft".to_owned()),
                    FieldValue::String("done".to_owned()),
                ]),
            )]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let cache = SelectValuesFileCache::for_test();

            let field_type =
                SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert!(errors.is_empty());
            let def =
                select_field(&field_type).expect("expected select field type");
            let first = def.values().first().expect("first entry");
            assert_eq!(def.values().len(), 2);
            assert_eq!(first.value(), &FieldValue::String("draft".to_owned()));
            assert_eq!(first.label(), &FieldValue::String("draft".to_owned()));
            assert!(first.extra().is_empty());
        }

        #[test]
        fn defaults_to_empty_list_when_values_omitted() {
            let opts = options(&[]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let cache = SelectValuesFileCache::for_test();

            let field_type =
                SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert!(errors.is_empty());
            assert_eq!(
                field_type,
                SchemaFieldType::Select(Arc::new(SchemaSelectField::default()))
            );
        }

        #[test]
        fn rejects_non_list_or_non_object_value_types() {
            let opts =
                options(&[("values", FieldValue::String("draft".to_owned()))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let cache = SelectValuesFileCache::for_test();

            let _ = SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::TypeMismatch { .. }
            ));
        }

        #[test]
        fn rejects_lists_containing_non_strings() {
            let opts = options(&[(
                "values",
                FieldValue::List(vec![FieldValue::Int(1), FieldValue::Int(2)]),
            )]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let cache = SelectValuesFileCache::for_test();

            let _ = SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::TypeMismatch { .. }
            ));
        }
    }

    mod inline_value_objects {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn parses_object_list_and_sorts_by_order() {
            let mut obj1 = IndexMap::new();
            obj1.insert(
                "value".to_owned(),
                FieldValue::String("ca".to_owned()),
            );
            obj1.insert(
                "label".to_owned(),
                FieldValue::String("Canada".to_owned()),
            );
            obj1.insert("order".to_owned(), FieldValue::Float(2.0));
            obj1.insert(
                "currency".to_owned(),
                FieldValue::String("CAD".to_owned()),
            );

            let mut obj2 = IndexMap::new();
            obj2.insert(
                "value".to_owned(),
                FieldValue::String("us".to_owned()),
            );
            obj2.insert(
                "label".to_owned(),
                FieldValue::String("United States".to_owned()),
            );
            obj2.insert("order".to_owned(), FieldValue::Float(1.0));
            obj2.insert(
                "currency".to_owned(),
                FieldValue::String("USD".to_owned()),
            );

            let opts = options(&[(
                "values",
                FieldValue::List(vec![
                    FieldValue::Object(obj1),
                    FieldValue::Object(obj2),
                ]),
            )]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let cache = SelectValuesFileCache::for_test();

            let field_type =
                SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert!(errors.is_empty());

            let def = select_field(&field_type).expect("expected select field");
            let values = def.values();
            let first = values.first().expect("first entry");
            let second = values.get(1).expect("second entry");

            assert_eq!(values.len(), 2);
            assert_eq!(first.value(), &FieldValue::String("us".to_owned()));
            assert_eq!(
                first.label(),
                &FieldValue::String("United States".to_owned())
            );
            assert_eq!(
                first.extra().get("currency"),
                Some(&FieldValue::String("USD".to_owned()))
            );
            assert_eq!(
                first.extra().get("order"),
                Some(&FieldValue::Float(1.0))
            );

            assert_eq!(second.value(), &FieldValue::String("ca".to_owned()));
            assert_eq!(
                second.label(),
                &FieldValue::String("Canada".to_owned())
            );
            assert_eq!(
                second.extra().get("currency"),
                Some(&FieldValue::String("CAD".to_owned()))
            );
            assert_eq!(
                second.extra().get("order"),
                Some(&FieldValue::Float(2.0))
            );
        }

        #[test]
        fn rejects_lists_with_inconsistent_keys() {
            let mut obj1 = IndexMap::new();
            obj1.insert(
                "value".to_owned(),
                FieldValue::String("ca".to_owned()),
            );
            obj1.insert(
                "label".to_owned(),
                FieldValue::String("Canada".to_owned()),
            );

            let mut obj2 = IndexMap::new();
            obj2.insert(
                "value".to_owned(),
                FieldValue::String("us".to_owned()),
            );

            let opts = options(&[(
                "values",
                FieldValue::List(vec![
                    FieldValue::Object(obj1),
                    FieldValue::Object(obj2),
                ]),
            )]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let cache = SelectValuesFileCache::for_test();

            let _ = SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            let error = errors.first().expect("expected error");
            assert!(matches!(
                error,
                SchemaFieldParserError::SelectorMissingKey { selector, key, .. }
                    if *selector == "label" && key == "label"
            ));
        }

        #[test]
        fn rejects_fields_with_invalid_types() {
            let mut obj = IndexMap::new();
            obj.insert("value".to_owned(), FieldValue::Int(123));
            obj.insert(
                "order".to_owned(),
                FieldValue::String("first".to_owned()),
            );

            let opts = options(&[(
                "values",
                FieldValue::List(vec![FieldValue::Object(obj)]),
            )]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let cache = SelectValuesFileCache::for_test();

            let _ = SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert_eq!(errors.len(), 2);
            let value_error = errors.first().expect("value error");
            let order_error = errors.get(1).expect("order error");
            assert!(matches!(
                value_error,
                SchemaFieldParserError::TypeMismatch { key, expected, .. }
                    if key == "value" && *expected == "a string"
            ));
            assert!(matches!(
                order_error,
                SchemaFieldParserError::TypeMismatch { key, expected, .. }
                    if key == "order" && *expected == "a number"
            ));
        }

        #[test]
        fn sorts_order_with_total_float_ordering() {
            let mut nan = IndexMap::new();
            nan.insert(
                "value".to_owned(),
                FieldValue::String("nan".to_owned()),
            );
            nan.insert("order".to_owned(), FieldValue::Float(f64::NAN));

            let mut zero = IndexMap::new();
            zero.insert(
                "value".to_owned(),
                FieldValue::String("zero".to_owned()),
            );
            zero.insert("order".to_owned(), FieldValue::Float(0.0));

            let mut negative_zero = IndexMap::new();
            negative_zero.insert(
                "value".to_owned(),
                FieldValue::String("negative_zero".to_owned()),
            );
            negative_zero.insert("order".to_owned(), FieldValue::Float(-0.0));

            let opts = options(&[(
                "values",
                FieldValue::List(vec![
                    FieldValue::Object(nan),
                    FieldValue::Object(zero),
                    FieldValue::Object(negative_zero),
                ]),
            )]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let cache = SelectValuesFileCache::for_test();

            let field_type =
                SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert!(errors.is_empty());
            let def = select_field(&field_type).expect("expected select field");
            let values = def.values();
            let first = values.first().expect("first entry");
            let second = values.get(1).expect("second entry");
            let third = values.get(2).expect("third entry");

            assert_eq!(values.len(), 3);
            assert_eq!(
                first.value(),
                &FieldValue::String("negative_zero".to_owned())
            );
            assert_eq!(second.value(), &FieldValue::String("zero".to_owned()));
            assert_eq!(third.value(), &FieldValue::String("nan".to_owned()));
        }
    }

    mod file_sources {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn parses_valid_toml_and_json_files() {
            let temp = tempfile::tempdir().expect("tempdir");
            let values_dir = temp.path().join("values");
            std::fs::create_dir_all(&values_dir).expect("create_dir");

            let toml_path = values_dir.join("countries.toml");
            std::fs::write(
                &toml_path,
                "[[entries]]\nslug = \"us\"\nname = \"United States\"\nrank = \
                 1\n\n[[entries]]\nslug = \"ca\"\nname = \"Canada\"\nrank = \
                 2\n",
            )
            .expect("write toml");

            let json_path = values_dir.join("states.json");
            std::fs::write(
                &json_path,
                "{\n  \"entries\": [\n    \"California\",\n    \"New York\"\n  \
                 ]\n}",
            )
            .expect("write json");

            let cache = SelectValuesFileCache::new(temp.path());

            let mut subtable_toml = IndexMap::new();
            subtable_toml.insert(
                "path".to_owned(),
                FieldValue::String("values/countries.toml".to_owned()),
            );
            subtable_toml.insert(
                "value".to_owned(),
                FieldValue::String("slug".to_owned()),
            );
            subtable_toml.insert(
                "label".to_owned(),
                FieldValue::String("name".to_owned()),
            );
            subtable_toml.insert(
                "order".to_owned(),
                FieldValue::String("rank".to_owned()),
            );

            let opts_toml =
                options(&[("values", FieldValue::Object(subtable_toml))]);
            let addr = address();
            let mut parser_toml = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let field_type_toml = SchemaSelectField::parse(
                &cache,
                &mut parser_toml,
                &opts_toml,
                None,
            );
            let errors_toml = parser_toml.finish(&opts_toml);
            assert!(errors_toml.is_empty());

            let def_toml =
                select_field(&field_type_toml).expect("expected select");
            let first_toml =
                def_toml.values().first().expect("first TOML entry");
            assert_eq!(def_toml.values().len(), 2);
            assert_eq!(
                first_toml.value(),
                &FieldValue::String("us".to_owned())
            );
            assert_eq!(
                first_toml.label(),
                &FieldValue::String("United States".to_owned())
            );

            let mut subtable_json = IndexMap::new();
            subtable_json.insert(
                "path".to_owned(),
                FieldValue::String("values/states.json".to_owned()),
            );
            let opts_json =
                options(&[("values", FieldValue::Object(subtable_json))]);
            let mut parser_json = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let field_type_json = SchemaSelectField::parse(
                &cache,
                &mut parser_json,
                &opts_json,
                None,
            );
            let errors_json = parser_json.finish(&opts_json);
            assert!(errors_json.is_empty());

            let def_json =
                select_field(&field_type_json).expect("expected select");
            let first_json =
                def_json.values().first().expect("first JSON entry");
            assert_eq!(def_json.values().len(), 2);
            assert_eq!(
                first_json.value(),
                &FieldValue::String("California".to_owned())
            );
        }

        #[test]
        fn parses_json_null_as_null_field_value() {
            let temp = tempfile::tempdir().expect("tempdir");
            let json_path = temp.path().join("items.json");
            std::fs::write(
                &json_path,
                r#"{
  "entries": [
    { "slug": "a", "title": "Item A", "deprecated": null }
  ]
}"#,
            )
            .expect("write json");

            let cache = SelectValuesFileCache::new(temp.path());

            let mut subtable = IndexMap::new();
            subtable.insert(
                "path".to_owned(),
                FieldValue::String("items.json".to_owned()),
            );
            subtable.insert(
                "value".to_owned(),
                FieldValue::String("slug".to_owned()),
            );
            subtable.insert(
                "label".to_owned(),
                FieldValue::String("title".to_owned()),
            );

            let opts = options(&[("values", FieldValue::Object(subtable))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let field_type =
                SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert!(errors.is_empty());

            let def = select_field(&field_type).expect("expected select");
            let first = def.values().first().expect("first entry");
            assert_eq!(
                first.extra().get("deprecated"),
                Some(&FieldValue::Null)
            );
        }

        #[test]
        fn rejects_file_selectors_on_bare_strings() {
            let temp = tempfile::tempdir().expect("tempdir");
            let json_path = temp.path().join("items.json");
            std::fs::write(&json_path, "{\"entries\": [\"a\", \"b\"]}")
                .expect("write json");

            let cache = SelectValuesFileCache::new(temp.path());

            let mut subtable = IndexMap::new();
            subtable.insert(
                "path".to_owned(),
                FieldValue::String("items.json".to_owned()),
            );
            subtable.insert(
                "label".to_owned(),
                FieldValue::String("name".to_owned()),
            );

            let opts = options(&[("values", FieldValue::Object(subtable))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let _ = SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            let error = errors.first().expect("expected error");
            assert!(matches!(
                error,
                SchemaFieldParserError::SelectorOnBareEntries { selector, .. } if *selector == "label"
            ));
        }

        #[test]
        fn rejects_unsupported_file_extensions() {
            let temp = tempfile::tempdir().expect("tempdir");
            let txt_path = temp.path().join("items.txt");
            std::fs::write(&txt_path, "hello").expect("write txt");

            let cache = SelectValuesFileCache::new(temp.path());

            let mut subtable = IndexMap::new();
            subtable.insert(
                "path".to_owned(),
                FieldValue::String("items.txt".to_owned()),
            );
            let opts = options(&[("values", FieldValue::Object(subtable))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let _ = SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            let error = errors.first().expect("expected error");
            assert!(matches!(
                error,
                SchemaFieldParserError::BadValueFileExtension { .. }
            ));
        }

        #[test]
        fn rejects_files_missing_entries_key() {
            let temp = tempfile::tempdir().expect("tempdir");
            let bad_json = temp.path().join("no_entries.json");
            std::fs::write(&bad_json, "{}").expect("write json");

            let cache = SelectValuesFileCache::new(temp.path());

            let mut subtable = IndexMap::new();
            subtable.insert(
                "path".to_owned(),
                FieldValue::String("no_entries.json".to_owned()),
            );
            let opts = options(&[("values", FieldValue::Object(subtable))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let _ = SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            let error = errors.first().expect("expected error");
            assert!(matches!(
                error,
                SchemaFieldParserError::ValueFileMissingEntries { .. }
            ));
        }

        #[test]
        fn rejects_values_files_with_unknown_top_level_keys() {
            let temp = tempfile::tempdir().expect("tempdir");
            let json_path = temp.path().join("extra.json");
            std::fs::write(&json_path, "{\"entries\": [], \"typo\": 123}")
                .expect("write json");

            let cache = SelectValuesFileCache::new(temp.path());

            let mut subtable = IndexMap::new();
            subtable.insert(
                "path".to_owned(),
                FieldValue::String("extra.json".to_owned()),
            );
            let opts = options(&[("values", FieldValue::Object(subtable))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let _ = SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            let error = errors.first().expect("expected error");
            assert!(matches!(
                error,
                SchemaFieldParserError::ValueFileLoad { .. }
            ));
        }

        #[test]
        fn rejects_file_subtables_with_unknown_keys() {
            let temp = tempfile::tempdir().expect("tempdir");
            let json_path = temp.path().join("items.json");
            std::fs::write(&json_path, "{\"entries\": []}")
                .expect("write json");

            let cache = SelectValuesFileCache::new(temp.path());

            let mut subtable = IndexMap::new();
            subtable.insert(
                "path".to_owned(),
                FieldValue::String("items.json".to_owned()),
            );
            subtable.insert("typo".to_owned(), FieldValue::Bool(true));
            let opts = options(&[("values", FieldValue::Object(subtable))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );

            let _ = SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            let error = errors.first().expect("expected error");
            assert!(matches!(
                error,
                SchemaFieldParserError::UnknownKey { key, .. } if key == "typo"
            ));
        }

        #[test]
        fn rejects_file_paths_that_escape_schema_directory() {
            let temp = tempfile::tempdir().expect("tempdir");
            let cache = SelectValuesFileCache::new(temp.path());

            let mut subtable = IndexMap::new();
            subtable.insert(
                "path".to_owned(),
                FieldValue::String("../outside.json".to_owned()),
            );
            let opts = options(&[("values", FieldValue::Object(subtable))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );

            let _ = SchemaSelectField::parse(&cache, &mut parser, &opts, None);
            let errors = parser.finish(&opts);

            let error = errors.first().expect("expected error");
            assert!(matches!(
                error,
                SchemaFieldParserError::ValueFileLoad { path, .. } if path == "../outside.json"
            ));
        }
    }

    mod cache {
        use super::*;

        #[test]
        fn memoizes_loaded_files() {
            let temp = tempfile::tempdir().expect("tempdir");
            let json_path = temp.path().join("items.json");
            std::fs::write(&json_path, "{\"entries\": [\"a\", \"b\"]}")
                .expect("write json");

            let cache = SelectValuesFileCache::new(temp.path());
            let res1 = cache.load("items.json").expect("loads first time");
            let res2 = cache.load("items.json").expect("loads second time");

            assert!(Arc::ptr_eq(&res1, &res2));
        }
    }

    mod inheritance {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn falls_back_to_base_values_on_omission() {
            let base = SchemaSelectField::for_test(vec![
                SchemaSelectFieldEntry::literal("old".to_owned()),
            ]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            let cache = SelectValuesFileCache::for_test();

            let field_type = SchemaSelectField::parse(
                &cache,
                &mut parser,
                &IndexMap::new(),
                Some(&base),
            );
            let errors = parser.finish(&IndexMap::new());

            assert!(errors.is_empty());
            assert_eq!(field_type, SchemaFieldType::Select(Arc::new(base)));
        }
    }
}
