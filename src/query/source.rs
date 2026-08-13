//! Parse and evaluate page source expressions.

use std::{collections::BTreeSet, path::PathBuf, vec};

use logos::{Lexer, Logos};

use super::{QueryError, operators::LogicalOp};
use crate::{
    index::FileRecord,
    note::{FieldValue, Note},
};

/// Selects every Note or a parsed source expression.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuerySource {
    /// Matches every indexed Note.
    All,
    /// Matches Notes selected by the expression.
    Expr(QuerySourceExpr),
}

/// A composable source-selection expression.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuerySourceExpr {
    /// Matches an exact tag or any nested sub-tag.
    Tag(String),
    /// Matches an exact file path or a folder prefix.
    Path(PathBuf),
    /// Matches any named File Class under the requested expansion mode.
    Class {
        /// Raw class names requested by the user.
        names: Vec<String>,
        /// Expansion depth and resolved class match set.
        mode: ClassExpansionMode,
    },
    /// Matches when every child expression matches.
    And(Vec<Self>),
    /// Matches when any child expression matches.
    Or(Vec<Self>),
    /// Matches when the child expression does not match.
    Not(Box<Self>),
}

/// Requested File Class expansion depth and its resolved match set.
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
    /// Returns the precomputed class names used during Note matching.
    #[must_use]
    pub(crate) fn classes(&self) -> &BTreeSet<String> {
        match self {
            Self::Exact(classes)
            | Self::Children(classes)
            | Self::Descendants(classes) => classes,
        }
    }

    /// Replaces the precomputed class names without changing expansion depth.
    pub(crate) fn set_classes(&mut self, resolved: BTreeSet<String>) {
        match self {
            Self::Exact(classes)
            | Self::Children(classes)
            | Self::Descendants(classes) => *classes = resolved,
        }
    }
}

impl QuerySourceExpr {
    /// Parses `input` as a source expression.
    ///
    /// Boolean operators use `not` > `and` > `or` precedence. Leaves are tags,
    /// quoted or bare paths, and File Classes written as `@Class`, `@Class+`,
    /// `@Class*`, or `class(Name)` with an optional expansion modifier.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnparsableSourceExpression`] when `input` is not a
    /// complete source expression.
    pub fn parse(input: &str) -> Result<Self, QueryError> {
        let tokens = tokenize(input)?;
        let mut parser = SourceParser::new(input, tokens);
        let expression = parser.parse_expr()?;
        if parser.peek().is_some() {
            return Err(QueryError::unparsable_source(input));
        }
        Ok(expression)
    }

    /// Returns whether the indexed Note satisfies this expression.
    #[must_use]
    pub(crate) fn is_match(
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
            Self::And(expressions) => expressions
                .iter()
                .all(|expr| expr.is_match(file, note, class_field)),
            Self::Or(expressions) => expressions
                .iter()
                .any(|expr| expr.is_match(file, note, class_field)),
            Self::Not(expr) => !expr.is_match(file, note, class_field),
        }
    }

    /// Return whether this expression contains any File Class leaf.
    #[must_use]
    pub(crate) fn has_classes(&self) -> bool {
        match self {
            Self::Class {
                ..
            } => true,
            Self::And(expressions) | Self::Or(expressions) => {
                expressions.iter().any(Self::has_classes)
            }
            Self::Not(expression) => expression.has_classes(),
            Self::Tag(_) | Self::Path(_) => false,
        }
    }
}

impl QuerySource {
    /// Parses a source, treating an empty or whitespace-only string as all
    /// indexed Notes.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnparsableSourceExpression`] when a non-empty
    /// `input` is invalid.
    pub fn parse(input: &str) -> Result<Self, QueryError> {
        if input.trim().is_empty() {
            Ok(Self::All)
        } else {
            QuerySourceExpr::parse(input).map(Self::Expr)
        }
    }

    /// Returns whether the indexed Note belongs to this source.
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

    /// Return whether this source requires Schema registry expansion.
    #[must_use]
    pub(crate) fn has_classes(&self) -> bool {
        match self {
            Self::All => false,
            Self::Expr(expression) => expression.has_classes(),
        }
    }
}

/// Yields the string File Class values held by `class_field`.
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
    #[regex(r"[^\s(),!&|#@]+", |lex| lex.slice().to_owned())]
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

fn tokenize(input: &str) -> Result<Vec<SourceToken>, QueryError> {
    SourceToken::lexer(input)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|()| QueryError::unparsable_source(input))
}

struct SourceParser<'a> {
    input: &'a str,
    tokens: std::iter::Peekable<vec::IntoIter<SourceToken>>,
}

impl<'a> SourceParser<'a> {
    fn new(input: &'a str, tokens: Vec<SourceToken>) -> Self {
        Self {
            input,
            tokens: tokens.into_iter().peekable(),
        }
    }

    fn invalid(&self) -> QueryError {
        QueryError::unparsable_source(self.input)
    }

    fn peek(&mut self) -> Option<&SourceToken> {
        self.tokens.peek()
    }

    fn next(&mut self) -> Option<SourceToken> {
        self.tokens.next()
    }

    fn expect(&mut self, expected: SourceToken) -> Result<(), QueryError> {
        if self.next() == Some(expected) {
            Ok(())
        } else {
            Err(self.invalid())
        }
    }

    fn parse_expr(&mut self) -> Result<QuerySourceExpr, QueryError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<QuerySourceExpr, QueryError> {
        self.parse_logical_chain(LogicalOp::Or, Self::parse_and)
    }

    fn parse_and(&mut self) -> Result<QuerySourceExpr, QueryError> {
        self.parse_logical_chain(LogicalOp::And, Self::parse_not)
    }

    fn parse_logical_chain(
        &mut self,
        operator: LogicalOp,
        mut parse_term: impl FnMut(&mut Self) -> Result<QuerySourceExpr, QueryError>,
    ) -> Result<QuerySourceExpr, QueryError> {
        let first = parse_term(self)?;
        if self.peek() != Some(&SourceToken::Logical(operator)) {
            return Ok(first);
        }
        self.next();
        let second = parse_term(self)?;
        let mut expressions = vec![first, second];
        while self.peek() == Some(&SourceToken::Logical(operator)) {
            self.next();
            expressions.push(parse_term(self)?);
        }
        Ok(match operator {
            LogicalOp::And => QuerySourceExpr::And(expressions),
            LogicalOp::Or => QuerySourceExpr::Or(expressions),
        })
    }

    fn parse_not(&mut self) -> Result<QuerySourceExpr, QueryError> {
        if self.peek() == Some(&SourceToken::Not) {
            self.next();
            Ok(QuerySourceExpr::Not(Box::new(self.parse_not()?)))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<QuerySourceExpr, QueryError> {
        match self.next() {
            Some(SourceToken::Tag(tag)) => Ok(QuerySourceExpr::Tag(tag)),
            Some(SourceToken::ClassSigil(sigil)) => self.parse_sigil(&sigil),
            Some(SourceToken::Quoted(path) | SourceToken::Bare(path)) => {
                Ok(QuerySourceExpr::Path(PathBuf::from(path)))
            }
            Some(SourceToken::Class) => self.parse_class_function(),
            Some(SourceToken::LParen) => {
                let expression = self.parse_expr()?;
                self.expect(SourceToken::RParen)?;
                Ok(expression)
            }
            _ => Err(self.invalid()),
        }
    }

    fn parse_sigil(&self, sigil: &str) -> Result<QuerySourceExpr, QueryError> {
        let raw = sigil.strip_prefix('@').ok_or_else(|| self.invalid())?;
        let (name, mode) = if let Some(name) = raw.strip_suffix('+') {
            (name, ClassExpansionMode::Children(BTreeSet::new()))
        } else if let Some(name) = raw.strip_suffix('*') {
            (name, ClassExpansionMode::Descendants(BTreeSet::new()))
        } else {
            (raw, ClassExpansionMode::Exact(BTreeSet::new()))
        };
        if name.is_empty() {
            return Err(self.invalid());
        }
        Ok(QuerySourceExpr::Class {
            names: vec![name.to_owned()],
            mode,
        })
    }

    fn parse_class_function(&mut self) -> Result<QuerySourceExpr, QueryError> {
        self.expect(SourceToken::LParen)?;
        let Some(SourceToken::Quoted(name) | SourceToken::Bare(name)) =
            self.next()
        else {
            return Err(self.invalid());
        };
        if name.is_empty() {
            return Err(self.invalid());
        }
        let mut mode = ClassExpansionMode::Exact(BTreeSet::new());
        if self.peek() == Some(&SourceToken::Comma) {
            self.next();
            mode = match self.next() {
                Some(SourceToken::Bare(value)) if value == "children" => {
                    ClassExpansionMode::Children(BTreeSet::new())
                }
                Some(SourceToken::Bare(value)) if value == "descendants" => {
                    ClassExpansionMode::Descendants(BTreeSet::new())
                }
                _ => return Err(self.invalid()),
            };
        }
        self.expect(SourceToken::RParen)?;
        mode = match self.peek() {
            Some(SourceToken::WithChildren) => {
                self.next();
                ClassExpansionMode::Children(BTreeSet::new())
            }
            Some(SourceToken::WithDescendants) => {
                self.next();
                ClassExpansionMode::Descendants(BTreeSet::new())
            }
            _ => mode,
        };
        Ok(QuerySourceExpr::Class {
            names: vec![name],
            mode,
        })
    }
}

#[cfg(test)]
mod tests {

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

        #[test]
        fn parses_tag_leaf() {
            assert_eq!(
                QuerySourceExpr::parse("#projects/active"),
                Ok(QuerySourceExpr::Tag("#projects/active".to_owned()))
            );
        }

        #[test]
        fn parses_quoted_and_bare_paths() {
            assert_eq!(
                QuerySourceExpr::parse("\"books/dune.md\""),
                Ok(QuerySourceExpr::Path(PathBuf::from("books/dune.md")))
            );
            assert_eq!(
                QuerySourceExpr::parse("books/"),
                Ok(QuerySourceExpr::Path(PathBuf::from("books/")))
            );
        }

        #[test]
        fn parses_class_sigils_at_each_depth() {
            assert_eq!(
                QuerySourceExpr::parse("@Book"),
                Ok(QuerySourceExpr::Class {
                    names: vec!["Book".to_owned()],
                    mode: exact(),
                })
            );
            assert_eq!(
                QuerySourceExpr::parse("@Book+"),
                Ok(QuerySourceExpr::Class {
                    names: vec!["Book".to_owned()],
                    mode: children(),
                })
            );
            assert_eq!(
                QuerySourceExpr::parse("@Book*"),
                Ok(QuerySourceExpr::Class {
                    names: vec!["Book".to_owned()],
                    mode: descendants(),
                })
            );
        }

        #[test]
        fn parses_class_function_forms_at_each_depth() {
            assert_eq!(
                QuerySourceExpr::parse("class(Book)"),
                Ok(QuerySourceExpr::Class {
                    names: vec!["Book".to_owned()],
                    mode: exact(),
                })
            );
            assert_eq!(
                QuerySourceExpr::parse("class(Book, children)"),
                Ok(QuerySourceExpr::Class {
                    names: vec!["Book".to_owned()],
                    mode: children(),
                })
            );
            assert_eq!(
                QuerySourceExpr::parse("class(Book).with_descendants()"),
                Ok(QuerySourceExpr::Class {
                    names: vec!["Book".to_owned()],
                    mode: descendants(),
                })
            );
        }

        #[test]
        fn parses_boolean_precedence_and_parentheses() {
            assert_eq!(
                QuerySourceExpr::parse("#book or books/ and not @Archived"),
                Ok(QuerySourceExpr::Or(vec![
                    QuerySourceExpr::Tag("#book".to_owned()),
                    QuerySourceExpr::And(vec![
                        QuerySourceExpr::Path(PathBuf::from("books/")),
                        QuerySourceExpr::Not(Box::new(
                            QuerySourceExpr::Class {
                                names: vec!["Archived".to_owned()],
                                mode: exact(),
                            }
                        )),
                    ]),
                ]))
            );
            assert_eq!(
                QuerySourceExpr::parse("(#book or #movie) and !archive/"),
                Ok(QuerySourceExpr::And(vec![
                    QuerySourceExpr::Or(vec![
                        QuerySourceExpr::Tag("#book".to_owned()),
                        QuerySourceExpr::Tag("#movie".to_owned()),
                    ]),
                    QuerySourceExpr::Not(Box::new(QuerySourceExpr::Path(
                        PathBuf::from("archive/"),
                    ))),
                ]))
            );
        }

        #[test]
        fn accepts_symbolic_boolean_operators() {
            let parsed =
                QuerySourceExpr::parse("#book && !@Archived || @Movie");

            assert!(matches!(parsed, Ok(QuerySourceExpr::Or(_))));
        }

        #[test]
        fn rejects_empty_incomplete_and_trailing_input() {
            for invalid in [
                "",
                "#book and",
                "(#book",
                "#book #movie",
                "class()",
                "class(Book, unknown)",
                "class(Book).with_children() extra",
            ] {
                assert_eq!(
                    QuerySourceExpr::parse(invalid),
                    Err(QueryError::UnparsableSourceExpression {
                        expr: invalid.to_owned(),
                    })
                );
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
                QuerySourceExpr::Path(PathBuf::from("books/dune.md"))
                    .is_match(file, note, "class")
            );
            assert!(
                !QuerySourceExpr::Path(PathBuf::from("books/hyperion.md"))
                    .is_match(file, note, "class")
            );
        }

        #[test]
        fn reads_classes_from_the_execution_field() {
            let (_temp, index) =
                indexed_note("---\nkind: Book\n---\n", "book.md");
            let file = index.record(Path::new("book.md")).expect("File Record");
            let note = index.note(Path::new("book.md")).expect("Note");
            let expression = QuerySourceExpr::Class {
                names: vec!["Book".to_owned()],
                mode: ClassExpansionMode::Exact(BTreeSet::from([
                    "Book".to_owned()
                ])),
            };

            assert!(expression.is_match(file, note, "kind"));
            assert!(!expression.is_match(file, note, "class"));
        }
    }
}
