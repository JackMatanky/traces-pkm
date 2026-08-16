//! Owned config projections consumed by domains that cannot borrow [`Config`].
//!
//! # Why owned, not borrowed
//!
//! Every `template/engine/` namespace object ([`crate::schema::SchemaService`]
//! included) is wrapped via minijinja's `Value::from_object<T: Object + Send +
//! Sync + 'static>` (verified against minijinja 2.21.0's own source,
//! `value/mod.rs`): the compiler rejects a borrowed `&Config` there, full
//! stop. [`crate::template::TemplateEngine`] itself has no lifetime parameter
//! and takes `&Config` only as a constructor-scoped borrow, extracting owned
//! data before returning. So a namespace object was never in a position to
//! borrow `&Config` the way some other parts of the crate do; some owned
//! snapshot is unavoidable for these consumers.
//!
//! # Main Type
//!
//! - [`SchemaConfigSpec`] projects `[schemas]` and `[frontmatter]` config into
//!   the fields [`crate::schema::SchemaService`] needs, via
//!   [`Config::to_schema_spec`].

use std::{path::Path, sync::Arc};

use super::model::Config;
use crate::field::FieldKey;

/// Owned, `'static` projection of `[schemas]` and `[frontmatter]` config for
/// [`crate::schema::SchemaService`].
///
/// Private fields, `pub(crate)` accessors — matches [`Config`]'s own
/// convention ([`super::model::SchemasConfig`]/
/// [`super::model::FrontmatterConfig`], each chaining into the next) rather
/// than a plain-projection style: this spec is constructed once per
/// [`crate::template::TemplateEngine`] and held by a [`SchemaService`] for a
/// render's entire lifetime, crossing the `config` -> `schema` ->
/// `template::engine` module boundary each time something reads it. Full
/// immutability and hidden internals matter here in a way they wouldn't for a
/// borrowed, single-call-site filter struct.
///
/// [`SchemaService`]: crate::schema::SchemaService
#[derive(Clone, Debug)]
pub(crate) struct SchemaConfigSpec {
    root: Arc<Path>,
    directory: Arc<Path>,
    class_field: FieldKey,
    title_field: FieldKey,
    aliases_field: FieldKey,
}

impl SchemaConfigSpec {
    /// Returns the project root used to refresh the render-scoped
    /// `FileIndex`.
    #[inline]
    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the Schema registry directory, already resolved against
    /// [`Self::root`].
    #[inline]
    #[must_use]
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    /// Returns the frontmatter key naming a Note's File Class(es).
    #[inline]
    #[must_use]
    pub(crate) fn class_field(&self) -> &FieldKey {
        &self.class_field
    }

    /// Returns the frontmatter key holding a Note's display title, used as a
    /// `file`-field option label fallback.
    #[inline]
    #[must_use]
    pub(crate) fn title_field(&self) -> &FieldKey {
        &self.title_field
    }

    /// Returns the frontmatter key holding a Note's aliases, the first
    /// `file`-field option label source.
    #[inline]
    #[must_use]
    pub(crate) fn aliases_field(&self) -> &FieldKey {
        &self.aliases_field
    }

    /// Builds a spec directly from parts, for tests that do not exercise a
    /// full [`Config`].
    #[cfg(test)]
    #[must_use]
    pub(crate) fn for_test(root: &Path, directory: &Path) -> Self {
        Self {
            root: Arc::from(root),
            directory: Arc::from(directory),
            class_field: FieldKey::try_new("class")
                .expect("\"class\" is a valid field key"),
            title_field: FieldKey::try_new("title")
                .expect("\"title\" is a valid field key"),
            aliases_field: FieldKey::try_new("aliases")
                .expect("\"aliases\" is a valid field key"),
        }
    }
}

impl From<&Config> for SchemaConfigSpec {
    fn from(config: &Config) -> Self {
        Self {
            root: Arc::from(config.root()),
            directory: Arc::from(
                config.root().join(config.schemas().directory()),
            ),
            class_field: config.schemas().class_field().clone(),
            title_field: config.frontmatter().title().clone(),
            aliases_field: config.frontmatter().aliases().clone(),
        }
    }
}

impl Config {
    /// Projects this config's `[schemas]` and `[frontmatter]` settings into
    /// an owned [`SchemaConfigSpec`] for [`crate::schema::SchemaService`].
    #[inline]
    #[must_use]
    pub(crate) fn to_schema_spec(&self) -> SchemaConfigSpec {
        SchemaConfigSpec::from(self)
    }
}

#[cfg(test)]
mod tests {
    mod to_schema_spec {
        use std::path::PathBuf;

        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn projects_root_directory_and_frontmatter_keys_from_config() {
            let config = Config::for_test(
                PathBuf::from("/vault"),
                None,
                None,
                PathBuf::from("/vault"),
            );

            let spec = config.to_schema_spec();

            assert_eq!(spec.root(), Path::new("/vault"));
            assert_eq!(spec.directory(), Path::new("/vault/.traces/schemas/"));
            assert_eq!(spec.class_field().name(), "class");
            assert_eq!(spec.title_field().name(), "title");
            assert_eq!(spec.aliases_field().name(), "aliases");
        }
    }
}
