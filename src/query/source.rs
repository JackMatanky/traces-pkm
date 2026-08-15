//! Page source expression parsing and evaluation.
//!
//! Implements the parser and matcher for the source expression language
//! documented in the [parent module](super).
//!
//! The entry point is [`QuerySource::parse`]:
//! - Empty input yields [`QuerySource::All`] (matches everything)
//! - Nonempty input is parsed into a [`QuerySourceExpr`]
//!
//! # Source expressions
//!
//! Boolean combinations of:
//! - **Tags**: `#book`, `#projects/active` (nested match automatically)
//! - **Paths**: `books/`, `"books/dune.md"` (trailing `/` matches folders)
//! - **Classes**: `@Book`, `@Book+`, `@Book*`, or `class(Name)` with modifiers
//!
//! Precedence: `NOT` > `AND` > `OR`; parentheses override.
//! See the [parent module](super) for escaping rules.

use std::{collections::BTreeSet, path::PathBuf};

use logos::{Lexer, Logos};
use miette::SourceSpan;

use super::{
    QueryError,
    error::{QueryDialect, QuerySyntaxError},
    logic::{
        LogicalControl, LogicalExpr, LogicalGrammar, LogicalOp, Spanned,
        TokenCursor, parse_logical_expression,
    },
};
use crate::{
    index::FileRecord,
    note::{FieldValue, Note},
};

/// Top-level source selector: every Note or a parsed expression.
///
/// The CLI and template system pass a `--from` / `.from(...)` string to
/// [`QuerySource::parse`]. An empty or whitespace-only input yields
/// [`Self::All`] (match everything); any nonempty input is parsed into a
/// [`QuerySourceExpr`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuerySource {
    /// Every indexed Note satisfies this source.
    All,
    /// Only Notes matching this expression satisfy this source.
    Expr(QuerySourceExpr),
}

/// Parsed source expression wrapping a boolean tree of [`SourceAtom`] leaves.
///
/// Created by `QuerySourceExpr::parse` from a source expression string. The AST
/// is opaque to callers; the only way to inspect it is through the provided
/// methods: [`is_match`](Self::is_match) for evaluation, expansion is needed,
/// and [`visit_atoms_mut`](Self::visit_atoms_mut) to load class names before
/// matching.
///
/// Operator precedence is `NOT` > `AND` > `OR`; parentheses override.
/// See the [module-level syntax reference](self) for the full grammar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuerySourceExpr(LogicalExpr<SourceAtom>);

/// Atomic match predicate in a source expression.
///
/// Combined into expression trees by boolean operators (`and`, `or`, `not`).
///
/// # Matching rules
///
/// - **[`Tag`]**: matches if the Note carries the named tag or any nested
///   sub-tag (`#book` matches `#book/fiction`)
/// - **[`Path`]**: matches if the file path equals the stored path exactly, or
///   its folder starts with it (`books/` matches `books/dune.md`)
/// - **[`Class`]**: matches if any of the Note's class field values appears in
///   the resolved [`ClassExpansionMode`] set
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SourceAtom {
    /// Matches an exact tag or any nested sub-tag.
    Tag(String),
    /// Matches an exact path or folder prefix.
    Path(PathBuf),
    /// Matches any named File Class under the requested expansion mode.
    Class {
        /// Raw class names requested by the user.
        names: Vec<String>,
        /// Expansion depth and the precomputed class match set.
        mode: ClassExpansionMode,
    },
}

impl SourceAtom {
    /// Returns whether `file` and `note` match this atom.
    fn is_match(
        &self,
        file: &FileRecord,
        note: &Note,
        class_field: &str,
    ) -> bool {
        match self {
            Self::Tag(tag) => {
                note.tags().iter().any(|value| value.is_nested_under(tag))
            }
            Self::Path(path) => {
                file.path() == path || file.folder().starts_with(path)
            }
            Self::Class {
                mode,
                ..
            } => class_values(note, class_field)
                .any(|value| mode.classes().contains(value)),
        }
    }
}

/// Expansion depth for a File Class match.
///
/// | Mode          | Matches                                    | Sigil | Function                |
/// | ------------- | ------------------------------------------ | ----- | ----------------------- |
/// | `Exact`       | Only the named class                       | `@C`  | `class(C)`              |
/// | `Children`    | Named class and its direct sub-classes     | `@C+` | `class(C, children)`    |
/// | `Descendants` | Named class and all transitive sub-classes | `@C*` | `class(C, descendants)` |
///
/// The inner [`BTreeSet`] holds precomputed class names. Populate via
/// [`set_classes`](Self::set_classes) before matching; the set is empty
/// after parsing and resolved at query execution time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClassExpansionMode {
    /// Match only the named class.
    Exact(BTreeSet<String>),
    /// Match the named class and its direct children.
    Children(BTreeSet<String>),
    /// Match the named class and every transitive descendant.
    Descendants(BTreeSet<String>),
}

impl ClassExpansionMode {
    /// Returns the resolved class names.
    ///
    /// Empty until populated by [`set_classes`](Self::set_classes).
    #[must_use]
    pub(crate) fn classes(&self) -> &BTreeSet<String> {
        match self {
            Self::Exact(classes)
            | Self::Children(classes)
            | Self::Descendants(classes) => classes,
        }
    }

    /// Populates the resolved class names.
    ///
    /// Replaces any existing set; does not alter the expansion depth.
    pub(crate) fn set_classes(&mut self, resolved: BTreeSet<String>) {
        match self {
            Self::Exact(classes)
            | Self::Children(classes)
            | Self::Descendants(classes) => *classes = resolved,
        }
    }
}

impl LogicalExpr<SourceAtom> {
    /// Parses `input` as a source expression.
    fn parse(input: &str) -> Result<Self, QueryError> {
        parse_logical_expression(input, tokenize(input)?, SourceGrammar)
    }

    /// Returns whether `file` and `note` satisfy this boolean expression.
    ///
    /// - `And` requires all children to match
    /// - `Or` requires at least one to match
    /// - `Not` inverts its child
    #[must_use]
    fn is_match(
        &self,
        file: &FileRecord,
        note: &Note,
        class_field: &str,
    ) -> bool {
        match self {
            Self::Atom(atom) => atom.is_match(file, note, class_field),
            Self::And(expressions) => expressions
                .iter()
                .all(|expression| expression.is_match(file, note, class_field)),
            Self::Or(expressions) => expressions
                .iter()
                .any(|expression| expression.is_match(file, note, class_field)),
            Self::Not(expression) => {
                !expression.is_match(file, note, class_field)
            }
        }
    }
}

impl QuerySourceExpr {
    /// Parses `input` as a source expression.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::Syntax`] on any tokenization or parse failure.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use traces_pkm::query::source::QuerySourceExpr;
    /// let expr = QuerySourceExpr::parse("#book").unwrap();
    /// ```
    pub(crate) fn parse(input: &str) -> Result<Self, QueryError> {
        <LogicalExpr<SourceAtom>>::parse(input).map(Self)
    }

    /// Returns whether `file` and `note` satisfy this expression.
    #[must_use]
    pub(crate) fn is_match(
        &self,
        file: &FileRecord,
        note: &Note,
        class_field: &str,
    ) -> bool {
        self.0.is_match(file, note, class_field)
    }

    /// Returns whether this expression contains any
    /// [`Class`](SourceAtom::Class) atom.
    ///
    /// When `true`, resolve class names via
    /// [`visit_atoms_mut`](Self::visit_atoms_mut) before calling
    /// [`is_match`](Self::is_match).
    #[must_use]
    pub(crate) fn has_classes(&self) -> bool {
        self.0.any_atom(|atom| matches!(atom, SourceAtom::Class { .. }))
    }

    /// Applies `visitor` to every [`SourceAtom`] in the expression tree.
    ///
    /// Used by the query execution pipeline to populate
    /// [`ClassExpansionMode::set_classes`] on each
    /// [`Class`](SourceAtom::Class) atom before matching.
    pub(crate) fn visit_atoms_mut(
        &mut self,
        visitor: &mut impl FnMut(&mut SourceAtom),
    ) {
        self.0.visit_atoms_mut(visitor);
    }
}

impl QuerySource {
    /// Parses `input` as a source expression.
    ///
    /// Empty or whitespace-only input yields [`Self::All`]; any nonempty input
    /// is parsed into a [`QuerySourceExpr`].
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::Syntax`] when a nonempty `input` is invalid.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use traces_pkm::query::QuerySource;
    ///
    /// let source = QuerySource::parse("#book").unwrap();
    /// assert!(!source.has_classes());
    /// ```
    pub fn parse(input: &str) -> Result<Self, QueryError> {
        if input.trim().is_empty() {
            Ok(Self::All)
        } else {
            QuerySourceExpr::parse(input).map(Self::Expr)
        }
    }

    /// Returns whether `file` and `note` satisfy this source.
    ///
    /// [`Self::All`] matches every Note unconditionally.
    #[must_use]
    pub(crate) fn is_match(
        &self,
        file: &FileRecord,
        note: &Note,
        class_field: &str,
    ) -> bool {
        match self {
            Self::All => true,
            Self::Expr(expr) => expr.is_match(file, note, class_field),
        }
    }

    /// Returns whether this source contains File Class atoms requiring
    /// Schema-level class expansion.
    ///
    /// [`Self::All`] always returns `false`.
    #[must_use]
    pub(crate) fn has_classes(&self) -> bool {
        match self {
            Self::All => false,
            Self::Expr(expression) => expression.has_classes(),
        }
    }
}

/// Extracts string File Class values from the frontmatter of a note.
///
/// Returns string values from the field named `class_field` (single string or
/// list of strings). Empty iterator when the field is missing, not a
/// string/list, or contains no string elements.
pub(crate) fn class_values<'a>(
    note: &'a Note,
    class_field: &str,
) -> impl Iterator<Item = &'a str> {
    let value = note.frontmatter().and_then(|frontmatter| {
        let field = frontmatter
            .fields()
            .iter()
            .find(|field| field.key().is_match(class_field))?;
        Some(field.value())
    });
    let list = match value {
        Some(FieldValue::List(items)) => items.as_slice(),
        _ => &[],
    };
    let scalar = match value {
        Some(FieldValue::List(_)) | None => None,
        Some(other) => other.as_str(),
    };
    scalar.into_iter().chain(list.iter().filter_map(FieldValue::as_str))
}

/// Lexical tokens for the page source expression language.
#[derive(Logos, Clone, Debug, PartialEq)]
#[logos(skip r"[ \t\n\r\f]+")]
enum SourceToken {
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token(",")]
    Comma,
    #[regex(
        "&&|and|\\|\\||or",
        |lex| LogicalOp::try_from(lex.slice()),
        ignore(case)
    )]
    Logical(LogicalOp),
    #[token("!", priority = 3)]
    #[token("not", priority = 3, ignore(case))]
    Not,
    #[token("class", priority = 3, ignore(case))]
    Class,
    #[token(".with_children()", priority = 4)]
    WithChildren,
    #[token(".with_descendants()", priority = 4)]
    WithDescendants,
    #[regex(r"#[a-zA-Z0-9_\-./]+", |lex| lex.slice().to_owned())]
    Tag(String),
    #[regex(r"@[a-zA-Z0-9_\-./]+[+*]?", |lex| lex.slice().to_owned())]
    ClassSigil(String),
    #[regex(r#"\"([^\"\\]|\\.)*\""#, quoted_callback)]
    #[regex(r#"'([^'\\]|\\.)*'"#, quoted_callback)]
    Quoted(String),
    #[regex(r#"[^\s(),!&|#@'"]+"#, |lex| lex.slice().to_owned())]
    Bare(String),
}

fn quoted_callback(lexer: &mut Lexer<'_, SourceToken>) -> String {
    let raw = lexer.slice();
    let inner = raw.get(1..raw.len().saturating_sub(1)).unwrap_or_default();
    let mut value = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                value.push(escaped);
            }
        } else {
            value.push(ch);
        }
    }
    value
}

/// Helper to construct a [`QueryError::Syntax`] diagnostic for this source
/// dialect.
fn syntax_error(
    input: &str,
    span: SourceSpan,
    expected: &'static str,
) -> QueryError {
    QuerySyntaxError::new(QueryDialect::Source, input, span, expected).into()
}

/// Returns the span of the next token in the cursor, or the end of input.
fn cursor_span(
    input: &str,
    tokens: &mut TokenCursor<Spanned<SourceToken>>,
) -> SourceSpan {
    tokens
        .peek()
        .map_or_else(|| SourceSpan::from((input.len(), 0)), |token| token.span)
}

/// Tokenizes the source expression input string.
///
/// # Errors
///
/// Returns [`QueryError::Syntax`] if invalid tokens are encountered.
fn tokenize(input: &str) -> Result<Vec<Spanned<SourceToken>>, QueryError> {
    let mut lexer = SourceToken::lexer(input);
    let mut tokens = Vec::new();
    while let Some(result) = lexer.next() {
        let range = lexer.span();
        let span = SourceSpan::from((range.start, range.len()));
        let value =
            result.map_err(|()| syntax_error(input, span, "a source term"))?;
        tokens.push(Spanned::new(value, span));
    }
    Ok(tokens)
}

/// Grammatical implementation of [`LogicalGrammar`] for source selection
/// expressions.
struct SourceGrammar;

impl LogicalGrammar for SourceGrammar {
    type Atom = SourceAtom;
    type Token = SourceToken;

    fn control(&self, token: &Self::Token) -> Option<LogicalControl> {
        match token {
            SourceToken::Logical(operator) => {
                Some(LogicalControl::Operator(*operator))
            }
            SourceToken::Not => Some(LogicalControl::Not),
            SourceToken::LParen => Some(LogicalControl::LeftParen),
            SourceToken::RParen => Some(LogicalControl::RightParen),
            SourceToken::Comma
            | SourceToken::Class
            | SourceToken::WithChildren
            | SourceToken::WithDescendants
            | SourceToken::Tag(_)
            | SourceToken::ClassSigil(_)
            | SourceToken::Quoted(_)
            | SourceToken::Bare(_) => None,
        }
    }

    fn parse_atom(
        &self,
        input: &str,
        tokens: &mut TokenCursor<Spanned<Self::Token>>,
    ) -> Result<Self::Atom, QueryError> {
        match tokens.next() {
            Some(Spanned {
                value: SourceToken::Tag(tag),
                ..
            }) => Ok(SourceAtom::Tag(tag)),
            Some(Spanned {
                value: SourceToken::ClassSigil(sigil),
                span,
            }) => parse_sigil(input, &sigil, span),
            Some(Spanned {
                value: SourceToken::Quoted(path) | SourceToken::Bare(path),
                ..
            }) => Ok(SourceAtom::Path(PathBuf::from(path))),
            Some(Spanned {
                value: SourceToken::Class,
                span,
            }) => parse_class_function(input, tokens, span),
            Some(token) => {
                Err(syntax_error(input, token.span, "a source term"))
            }
            None => Err(syntax_error(
                input,
                SourceSpan::from((input.len(), 0)),
                "a source term",
            )),
        }
    }

    fn syntax_error(
        &self,
        input: &str,
        span: SourceSpan,
        expected: &'static str,
    ) -> QuerySyntaxError {
        QuerySyntaxError::new(QueryDialect::Source, input, span, expected)
    }
}

/// Parses a sigil-form File Class term (e.g., `@Class`, `@Class+`, `@Class*`).
///
/// # Errors
///
/// Returns [`QueryError::Syntax`] if the sigil format or class name is invalid.
fn parse_sigil(
    input: &str,
    sigil: &str,
    span: SourceSpan,
) -> Result<SourceAtom, QueryError> {
    let raw = sigil
        .strip_prefix('@')
        .ok_or_else(|| syntax_error(input, span, "a File Class name"))?;
    let (name, mode) = if let Some(name) = raw.strip_suffix('+') {
        (name, ClassExpansionMode::Children(BTreeSet::new()))
    } else if let Some(name) = raw.strip_suffix('*') {
        (name, ClassExpansionMode::Descendants(BTreeSet::new()))
    } else {
        (raw, ClassExpansionMode::Exact(BTreeSet::new()))
    };
    if name.is_empty() {
        return Err(syntax_error(input, span, "a File Class name"));
    }
    Ok(SourceAtom::Class {
        names: vec![name.to_owned()],
        mode,
    })
}

/// Parses a function-form File Class term (e.g., `class(Name)`, `class(Name,
/// children)`).
///
/// # Errors
///
/// Returns [`QueryError::Syntax`] if parameters, parentheses, or modifiers are
/// invalid.
fn parse_class_function(
    input: &str,
    tokens: &mut TokenCursor<Spanned<SourceToken>>,
    class_span: SourceSpan,
) -> Result<SourceAtom, QueryError> {
    if !tokens.take(&SourceToken::LParen) {
        return Err(syntax_error(
            input,
            cursor_span(input, tokens),
            "`(` after `class`",
        ));
    }
    let Some(Spanned {
        value: SourceToken::Quoted(name) | SourceToken::Bare(name),
        ..
    }) = tokens.next()
    else {
        return Err(syntax_error(
            input,
            cursor_span(input, tokens),
            "a File Class name",
        ));
    };
    if name.is_empty() {
        return Err(syntax_error(input, class_span, "a File Class name"));
    }

    let mut mode = ClassExpansionMode::Exact(BTreeSet::new());
    let has_mode_argument = tokens.take(&SourceToken::Comma);
    if has_mode_argument {
        mode = match tokens.next() {
            Some(Spanned {
                value: SourceToken::Bare(value),
                ..
            }) if value == "children" => {
                ClassExpansionMode::Children(BTreeSet::new())
            }
            Some(Spanned {
                value: SourceToken::Bare(value),
                ..
            }) if value == "descendants" => {
                ClassExpansionMode::Descendants(BTreeSet::new())
            }
            Some(token) => {
                return Err(syntax_error(
                    input,
                    token.span,
                    "`children` or `descendants`",
                ));
            }
            None => {
                return Err(syntax_error(
                    input,
                    SourceSpan::from((input.len(), 0)),
                    "`children` or `descendants`",
                ));
            }
        };
    }
    if !tokens.take(&SourceToken::RParen) {
        return Err(syntax_error(
            input,
            cursor_span(input, tokens),
            "`)` after `class(...)`",
        ));
    }
    let modifier = if tokens.take(&SourceToken::WithChildren) {
        Some(ClassExpansionMode::Children(BTreeSet::new()))
    } else if tokens.take(&SourceToken::WithDescendants) {
        Some(ClassExpansionMode::Descendants(BTreeSet::new()))
    } else {
        None
    };
    if has_mode_argument && modifier.is_some() {
        return Err(syntax_error(
            input,
            cursor_span(input, tokens),
            "one File Class expansion mode",
        ));
    }
    if let Some(modifier) = modifier {
        mode = modifier;
    }
    Ok(SourceAtom::Class {
        names: vec![name],
        mode,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn exact() -> ClassExpansionMode {
        ClassExpansionMode::Exact(BTreeSet::new())
    }

    fn children() -> ClassExpansionMode {
        ClassExpansionMode::Children(BTreeSet::new())
    }

    fn descendants() -> ClassExpansionMode {
        ClassExpansionMode::Descendants(BTreeSet::new())
    }

    mod parse {
        use pretty_assertions::assert_eq;

        use super::*;

        fn tag(value: &str) -> LogicalExpr<SourceAtom> {
            LogicalExpr::Atom(SourceAtom::Tag(value.to_owned()))
        }

        fn path(value: &str) -> LogicalExpr<SourceAtom> {
            LogicalExpr::Atom(SourceAtom::Path(PathBuf::from(value)))
        }

        fn class(
            name: &str,
            mode: ClassExpansionMode,
        ) -> LogicalExpr<SourceAtom> {
            LogicalExpr::Atom(SourceAtom::Class {
                names: vec![name.to_owned()],
                mode,
            })
        }

        #[test]
        fn parses_tag_and_paths() {
            assert_eq!(
                QuerySourceExpr::parse("#projects/active"),
                Ok(QuerySourceExpr(tag("#projects/active")))
            );
            assert_eq!(
                QuerySourceExpr::parse("\"books/dune.md\""),
                Ok(QuerySourceExpr(path("books/dune.md")))
            );
            assert_eq!(
                QuerySourceExpr::parse("books/"),
                Ok(QuerySourceExpr(path("books/")))
            );
        }

        #[test]
        fn parses_each_class_expansion_form() {
            assert_eq!(
                QuerySourceExpr::parse("@Book"),
                Ok(QuerySourceExpr(class("Book", exact())))
            );
            assert_eq!(
                QuerySourceExpr::parse("@Book+"),
                Ok(QuerySourceExpr(class("Book", children())))
            );
            assert_eq!(
                QuerySourceExpr::parse("@Book*"),
                Ok(QuerySourceExpr(class("Book", descendants())))
            );
            assert_eq!(
                QuerySourceExpr::parse("class(Book, children)"),
                Ok(QuerySourceExpr(class("Book", children())))
            );
            assert_eq!(
                QuerySourceExpr::parse("class(Book).with_descendants()"),
                Ok(QuerySourceExpr(class("Book", descendants())))
            );
        }

        #[test]
        fn parses_precedence_grouping_and_repeated_negation() {
            assert_eq!(
                QuerySourceExpr::parse("#book or books/ and not @Archived"),
                Ok(QuerySourceExpr(LogicalExpr::Or(vec![
                    tag("#book"),
                    LogicalExpr::And(vec![
                        path("books/"),
                        LogicalExpr::Not(Box::new(class("Archived", exact()))),
                    ]),
                ])))
            );
            assert_eq!(
                QuerySourceExpr::parse("(#book or #movie) and !archive/"),
                Ok(QuerySourceExpr(LogicalExpr::And(vec![
                    LogicalExpr::Or(vec![tag("#book"), tag("#movie")]),
                    LogicalExpr::Not(Box::new(path("archive/"))),
                ])))
            );
            assert_eq!(
                QuerySourceExpr::parse("not not #book and #movie and archive/"),
                Ok(QuerySourceExpr(LogicalExpr::And(vec![
                    LogicalExpr::Not(Box::new(LogicalExpr::Not(Box::new(
                        tag("#book"),
                    )))),
                    tag("#movie"),
                    path("archive/"),
                ])))
            );
        }

        #[test]
        fn accepts_symbolic_boolean_operators() {
            assert!(matches!(
                QuerySourceExpr::parse("#book && !@Archived || @Movie"),
                Ok(QuerySourceExpr(LogicalExpr::Or(_)))
            ));
        }

        #[test]
        fn rejects_incomplete_and_trailing_input() {
            for invalid in [
                "",
                "#book and",
                "(#book",
                "#book #movie",
                r#""books/dune.md"#,
                r#"'books/dune.md"#,
                "class()",
                "class(Book, unknown)",
                "class(Book, children).with_descendants()",
                "class(Book).with_children() extra",
            ] {
                assert!(QuerySourceExpr::parse(invalid).is_err(), "{invalid}");
            }
        }
    }

    mod is_match {
        use std::{fs, path::Path};

        use super::*;
        use crate::index::FileIndex;

        fn indexed_note(
            content: &str,
            path: &str,
        ) -> (tempfile::TempDir, FileIndex) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let full_path = temp.path().join(path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(full_path, content).expect("write Note");
            let index = FileIndex::build(temp.path()).expect("build index");
            (temp, index)
        }

        #[test]
        fn matches_boolean_combinations_of_tags_and_paths() {
            let (_temp, index) = indexed_note("#book", "books/dune.md");
            let file =
                index.record(Path::new("books/dune.md")).expect("File Record");
            let note = index.note(Path::new("books/dune.md")).expect("Note");
            let expression =
                QuerySourceExpr::parse("(#book and books/) and not archive/")
                    .expect("valid source");

            assert!(expression.is_match(file, note, "class"));
        }

        #[test]
        fn matches_exact_file_path_without_matching_sibling() {
            let (_temp, index) = indexed_note("#book", "books/dune.md");
            let file =
                index.record(Path::new("books/dune.md")).expect("File Record");
            let note = index.note(Path::new("books/dune.md")).expect("Note");

            assert!(
                LogicalExpr::Atom(SourceAtom::Path(PathBuf::from(
                    "books/dune.md"
                )))
                .is_match(file, note, "class")
            );
            assert!(
                !LogicalExpr::Atom(SourceAtom::Path(PathBuf::from(
                    "books/hyperion.md"
                )))
                .is_match(file, note, "class")
            );
        }

        #[test]
        fn reads_classes_from_the_execution_field() {
            let (_temp, index) =
                indexed_note("---\nkind: Book\n---\n", "book.md");
            let file = index.record(Path::new("book.md")).expect("File Record");
            let note = index.note(Path::new("book.md")).expect("Note");
            let expression = LogicalExpr::Atom(SourceAtom::Class {
                names: vec!["Book".to_owned()],
                mode: ClassExpansionMode::Exact(BTreeSet::from([
                    "Book".to_owned()
                ])),
            });

            assert!(expression.is_match(file, note, "kind"));
            assert!(!expression.is_match(file, note, "class"));
        }
    }
}
