use std::{collections::BTreeSet, path::Path};

use logos::{Lexer, Logos};
use miette::SourceSpan;
use regex::Regex;

use super::{
    expr::{
        AtomParser, BooleanExpr, LogicalControl, LogicalOp, parse_boolean_expr,
    },
    lex::{Spanned, TokenStream},
};
use crate::{
    file::FileBase,
    note::{Note, NoteFieldValue},
    query::{
        QueryError,
        error::{QueryDialect, QuerySyntaxError},
    },
};

/// Top-level source selector: every Note or a parsed expression.
///
/// The CLI and template system pass a `--from` / `.from(...)` string to
/// [`SourceSelector::parse`]. An empty or whitespace-only input yields
/// [`Self::All`] (match everything); any nonempty input is parsed into a
/// [`SourceExpr`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceSelector {
    /// Every indexed Note satisfies this source.
    All,
    /// Only Notes matching this expression satisfy this source.
    Expr(SourceExpr),
}

/// Parsed source expression wrapping a boolean tree of [`SourceAtom`] leaves.
///
/// Created by [`SourceExpr::parse`] from a source expression string. The AST
/// is opaque to callers; the only way to inspect it is through the provided
/// methods: [`is_match`](Self::is_match) for evaluation,
/// [`has_classes`](Self::has_classes) to check whether class expansion is
/// needed, and [`visit_atoms_mut`](Self::visit_atoms_mut) to load class
/// names before matching.
///
/// Operator precedence is `NOT` > `AND` > `OR`; parentheses override.
/// See the [source expression language](super::super) documentation for the
/// full grammar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceExpr(BooleanExpr<SourceAtom>);

/// Atomic match predicate in a source expression.
///
/// Combined into expression trees by boolean operators (`and`, `or`, `not`).
///
/// # Matching rules
///
/// - **[`SourceAtom::Tag`]**: matches if the Note carries the named tag or any
///   nested sub-tag (`#book` matches `#book/fiction`)
/// - **[`SourceAtom::Path`]**: matches if the file path matches the compiled
///   glob (`books/` compiles to `books/**`, matching every file under it)
/// - **[`SourceAtom::Class`]**: matches if any of the Note's class field values
///   appears in the resolved [`ClassExpansionMode`] set
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SourceAtom {
    /// Matches an exact tag or any nested sub-tag.
    Tag(String),
    /// Matches a path against a compiled glob pattern.
    Path(GlobPattern),
    /// Matches any named File Class under the requested expansion mode.
    Class {
        /// Raw class names requested by the user.
        names: Vec<String>,
        /// Expansion depth and the precomputed class match set.
        mode: ClassExpansionMode,
    },
}

/// Compiled glob pattern backing [`SourceAtom::Path`].
///
/// Dialect: `*` matches any run of characters except `/`; `**` matches any run
/// of characters including `/`; every other character matches literally. The
/// compiled regex is anchored to match the whole path and is built once at
/// construction, not per-match.
#[derive(Clone)]
pub(crate) struct GlobPattern {
    regex: Regex,
    pattern: String,
}

impl std::fmt::Debug for GlobPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("GlobPattern").field(&self.pattern).finish()
    }
}

impl PartialEq for GlobPattern {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for GlobPattern {}

/// Expansion depth for a File Class match.
///
/// | Mode          | Matches                                    | Sigil | Function                |
/// | ------------- | ------------------------------------------ | ----- | ----------------------- |
/// | `Exact`       | Only the named class                       | `@C`  | `class(C)`              |
/// | `Children`    | Named class and its direct sub-classes     | `@C+` | `class(C, children)`    |
/// | `Descendants` | Named class and all transitive sub-classes | `@C*` | `class(C, descendants)` |
///
/// The inner [`BTreeSet`] holds precomputed class names. Populate via
/// [`set_classes`](Self::set_classes) before matching; the set is empty after
/// parsing and resolved at query execution time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClassExpansionMode {
    /// Match only the named class.
    Exact(BTreeSet<String>),
    /// Match the named class and its direct children.
    Children(BTreeSet<String>),
    /// Match the named class and every transitive descendant.
    Descendants(BTreeSet<String>),
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

/// Resolves File Class names against the Schema domain.
///
/// Implemented by `schema::SchemaService` in a dedicated composition module
/// (`crate::file_class_expander`) so neither `query` nor `schema` depends on
/// the other's types.
pub(crate) trait FileClassExpander {
    /// Populates `mode`'s match set from `classes` at its requested depth.
    fn expand(&self, classes: &[String], mode: &mut ClassExpansionMode);
}

impl GlobPattern {
    /// Compiles `pattern` (glob syntax: `*`, `**`, literal characters) into an
    /// anchored path matcher.
    ///
    /// # Errors
    ///
    /// Returns a [`regex::Error`] if the translated pattern fails to compile as
    /// a regex (not expected for this fixed `*`/`**`-only translation, but the
    /// compile step is fallible in principle).
    fn compile(pattern: &str) -> Result<Self, regex::Error> {
        let mut regex_source = String::from("^");
        let mut rest = pattern;
        while let Some(index) = rest.find('*') {
            regex_source.push_str(&regex::escape(&rest[..index]));
            rest = &rest[index.saturating_add(1)..];
            if let Some(stripped) = rest.strip_prefix('*') {
                regex_source.push_str(".*");
                rest = stripped;
            } else {
                regex_source.push_str("[^/]*");
            }
        }
        regex_source.push_str(&regex::escape(rest));
        regex_source.push('$');
        Regex::new(&regex_source).map(|regex| Self {
            regex,
            pattern: pattern.to_owned(),
        })
    }

    /// Returns whether `path` matches this glob.
    #[must_use]
    fn is_match(&self, path: &Path) -> bool {
        self.regex.is_match(&path.to_string_lossy())
    }
}

impl SourceAtom {
    /// Constructs a path atom from a compiled glob `pattern`.
    pub(crate) fn path(pattern: &str) -> Result<Self, regex::Error> {
        GlobPattern::compile(pattern).map(Self::Path)
    }

    /// Returns whether `file` and `note` match this atom.
    ///
    /// `note` is `None` for files with no parsed [`Note`] (non-Markdown files);
    /// [`Self::Tag`] and [`Self::Class`] never match a note-less file, while
    /// [`Self::Path`] never reads `note`.
    fn is_match(
        &self,
        base: &FileBase,
        note: Option<&Note>,
        canonical_class_field: &str,
    ) -> bool {
        match self {
            Self::Tag(tag) => note.is_some_and(|note| {
                note.tags().iter().any(|value| value.is_contained_in(tag))
            }),
            Self::Path(pattern) => pattern.is_match(base.path()),
            Self::Class {
                mode,
                ..
            } => note.is_some_and(|note| {
                class_values(note, canonical_class_field)
                    .any(|value| mode.classes().contains(value))
            }),
        }
    }
}

impl ClassExpansionMode {
    /// Returns the resolved class names.
    ///
    /// Empty until populated by [`set_classes`](Self::set_classes).
    #[must_use]
    pub(crate) const fn classes(&self) -> &BTreeSet<String> {
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

impl SourceExpr {
    /// Parses `input` as a source expression.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::Syntax`] on any tokenization or parse failure.
    pub(crate) fn parse(input: &str) -> Result<Self, QueryError> {
        parse_boolean_expr(
            input,
            TokenStream::new(tokenize(input)?),
            SourceGrammar,
        )
        .map(Self)
    }

    /// Returns whether `file` and `note` satisfy this expression.
    #[must_use]
    pub(crate) fn is_match(
        &self,
        base: &FileBase,
        note: Option<&Note>,
        canonical_class_field: &str,
    ) -> bool {
        self.0.is_satisfied_by(|atom| {
            atom.is_match(base, note, canonical_class_field)
        })
    }

    /// Returns whether this expression contains any
    /// [`Class`](SourceAtom::Class) atom.
    ///
    /// When `true`, resolve class names via
    /// [`visit_atoms_mut`](Self::visit_atoms_mut) before calling
    /// [`is_match`](Self::is_match).
    #[must_use]
    pub(crate) fn has_classes(&self) -> bool {
        self.0.has_any_atom(|atom| matches!(atom, SourceAtom::Class { .. }))
    }

    /// Applies `visitor` to every [`SourceAtom`] in the expression tree.
    pub(crate) fn visit_atoms_mut(
        &mut self,
        visitor: &mut impl FnMut(&mut SourceAtom),
    ) {
        self.0.visit_atoms_mut(visitor);
    }

    /// Wraps a single atom as an expression.
    #[must_use]
    pub(crate) const fn atom(atom: SourceAtom) -> Self {
        Self(BooleanExpr::Atom(atom))
    }

    /// Builds a disjunction (OR) of `first` and `rest`.
    #[must_use]
    pub(crate) fn disjunction(
        first: SourceAtom,
        rest: Vec<SourceAtom>,
    ) -> Self {
        if rest.is_empty() {
            Self::atom(first)
        } else {
            let mut atoms = Vec::with_capacity(rest.len().saturating_add(1));
            atoms.push(BooleanExpr::Atom(first));
            atoms.extend(rest.into_iter().map(BooleanExpr::Atom));
            Self(BooleanExpr::Or(atoms))
        }
    }

    /// Builds a conjunction (AND) of `first` and `rest`.
    #[must_use]
    pub(crate) fn conjunction(first: Self, rest: Vec<Self>) -> Self {
        if rest.is_empty() {
            first
        } else {
            let mut terms = Vec::with_capacity(rest.len().saturating_add(1));
            terms.push(first.0);
            terms.extend(rest.into_iter().map(|term| term.0));
            Self(BooleanExpr::And(terms))
        }
    }
}

impl SourceSelector {
    /// Parses `input` as a source expression.
    ///
    /// Empty or whitespace-only input yields [`Self::All`]; any nonempty input
    /// is parsed into a [`SourceExpr`].
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::Syntax`] when a nonempty `input` is invalid.
    #[inline]
    pub fn parse(input: &str) -> Result<Self, QueryError> {
        if input.trim().is_empty() {
            Ok(Self::All)
        } else {
            SourceExpr::parse(input).map(Self::Expr)
        }
    }

    /// Returns whether `file` and `note` satisfy this source.
    #[must_use]
    pub(crate) fn is_match(
        &self,
        base: &FileBase,
        note: Option<&Note>,
        canonical_class_field: &str,
    ) -> bool {
        match self {
            Self::All => true,
            Self::Expr(expr) => {
                expr.is_match(base, note, canonical_class_field)
            }
        }
    }

    /// Returns whether this source contains File Class atoms requiring
    /// Schema-level class expansion.
    #[must_use]
    pub(crate) fn has_classes(&self) -> bool {
        match self {
            Self::All => false,
            Self::Expr(expression) => expression.has_classes(),
        }
    }

    /// Resolve every File Class leaf in `self` against `expander`.
    pub(crate) fn resolve_classes(
        &mut self,
        expander: &(impl FileClassExpander + ?Sized),
    ) {
        if let Self::Expr(expression) = self {
            expression.visit_atoms_mut(&mut |atom| {
                if let SourceAtom::Class {
                    names,
                    mode,
                } = atom
                {
                    expander.expand(names, mode);
                }
            });
        }
    }
}

impl SourceGrammar {
    /// Parses a sigil-form File Class term (e.g., `@Class`, `@Class+`,
    /// `@Class*`).
    ///
    /// # Errors
    ///
    /// Returns a [`QueryError::Syntax`] diagnostic if the sigil is empty or
    /// malformed.
    fn parse_sigil(
        input: &str,
        sigil: &str,
        span: SourceSpan,
    ) -> Result<SourceAtom, QueryError> {
        let raw = sigil
            .strip_prefix('@')
            .ok_or_else(|| syntax_error(input, span, "a File Class name"))?;
        let (name, mode) = raw.strip_suffix('+').map_or_else(
            || {
                raw.strip_suffix('*').map_or(
                    (raw, ClassExpansionMode::Exact(BTreeSet::new())),
                    |name| {
                        (name, ClassExpansionMode::Descendants(BTreeSet::new()))
                    },
                )
            },
            |name| (name, ClassExpansionMode::Children(BTreeSet::new())),
        );
        if name.is_empty() {
            return Err(syntax_error(input, span, "a File Class name"));
        }
        Ok(SourceAtom::Class {
            names: vec![name.to_owned()],
            mode,
        })
    }

    /// Parses a function-form File Class term (e.g., `class(Name)`,
    /// `class(Name, children)`).
    ///
    /// # Errors
    ///
    /// Returns a [`QueryError::Syntax`] diagnostic if the function form is
    /// malformed, the class name is empty, an unknown expansion mode is
    /// given, or both an argument and a method modifier are present.
    fn parse_class_function(
        input: &str,
        tokens: &mut TokenStream<Spanned<SourceToken>>,
        class_span: SourceSpan,
    ) -> Result<SourceAtom, QueryError> {
        tokens.expect(
            input,
            &SourceToken::LParen,
            "`(` after `class`",
            QueryDialect::Source,
        )?;

        let name_spanned = tokens.expect_map(
            input,
            "a File Class name",
            QueryDialect::Source,
            |token| match token {
                SourceToken::Quoted(name) | SourceToken::Bare(name) => {
                    Some(name)
                }
                _ => None,
            },
        )?;
        if name_spanned.value.is_empty() {
            return Err(syntax_error(input, class_span, "a File Class name"));
        }

        let mut mode = ClassExpansionMode::Exact(BTreeSet::new());
        let has_mode_argument = tokens.is_taken(&SourceToken::Comma);
        if has_mode_argument {
            mode = tokens
                .expect_map(
                    input,
                    "`children` or `descendants`",
                    QueryDialect::Source,
                    |token| match token {
                        SourceToken::Bare(value) if value == "children" => {
                            Some(ClassExpansionMode::Children(BTreeSet::new()))
                        }
                        SourceToken::Bare(value) if value == "descendants" => {
                            Some(ClassExpansionMode::Descendants(
                                BTreeSet::new(),
                            ))
                        }
                        _ => None,
                    },
                )?
                .value;
        }

        tokens.expect(
            input,
            &SourceToken::RParen,
            "`)` after `class(...)`",
            QueryDialect::Source,
        )?;

        let modifier = if tokens.is_taken(&SourceToken::WithChildren) {
            Some(ClassExpansionMode::Children(BTreeSet::new()))
        } else if tokens.is_taken(&SourceToken::WithDescendants) {
            Some(ClassExpansionMode::Descendants(BTreeSet::new()))
        } else {
            None
        };

        if has_mode_argument && modifier.is_some() {
            let next_span = tokens.next_span(input);
            return Err(syntax_error(
                input,
                next_span,
                "one File Class expansion mode",
            ));
        }

        if let Some(modifier) = modifier {
            mode = modifier;
        }

        Ok(SourceAtom::Class {
            names: vec![name_spanned.value],
            mode,
        })
    }
}

struct SourceGrammar;

impl AtomParser for SourceGrammar {
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
        tokens: &mut TokenStream<Spanned<Self::Token>>,
    ) -> Result<Self::Atom, QueryError> {
        let next_span = tokens.next_span(input);
        match tokens.next() {
            Some(Spanned {
                value: SourceToken::Tag(tag),
                ..
            }) => Ok(SourceAtom::Tag(tag)),
            Some(Spanned {
                value: SourceToken::ClassSigil(sigil),
                span,
            }) => Self::parse_sigil(input, &sigil, span),
            Some(Spanned {
                value: SourceToken::Quoted(path) | SourceToken::Bare(path),
                span,
            }) => {
                let glob = if path.ends_with('/') {
                    format!("{path}**")
                } else {
                    path
                };
                GlobPattern::compile(&glob)
                    .map(SourceAtom::Path)
                    .map_err(|_| syntax_error(input, span, "a valid path glob"))
            }
            Some(Spanned {
                value: SourceToken::Class,
                span,
            }) => Self::parse_class_function(input, tokens, span),
            Some(token) => {
                Err(syntax_error(input, token.span, "a source term"))
            }
            None => Err(syntax_error(input, next_span, "a source term")),
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

/// Extracts string File Class values from the frontmatter of a note.
pub(crate) fn class_values<'a, 'b>(
    note: &'a Note,
    canonical_class_field: &'b str,
) -> impl Iterator<Item = &'a str> + 'b
where
    'a: 'b,
{
    let values =
        note.frontmatter().map(|fm| fm.get_values(canonical_class_field));
    values.into_iter().flatten().filter_map(NoteFieldValue::as_str)
}

#[expect(
    clippy::needless_pass_by_ref_mut,
    reason = "logos Callback trait requires &mut Lexer"
)]
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

fn syntax_error(
    input: &str,
    span: SourceSpan,
    expected: &'static str,
) -> QueryError {
    QuerySyntaxError::new(QueryDialect::Source, input, span, expected).into()
}

/// Tokenizes the source expression input string.
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

        fn tag(value: &str) -> BooleanExpr<SourceAtom> {
            BooleanExpr::Atom(SourceAtom::Tag(value.to_owned()))
        }

        fn path(value: &str) -> BooleanExpr<SourceAtom> {
            let SourceExpr(parsed) =
                SourceExpr::parse(value).expect("valid path");
            match parsed {
                atom @ BooleanExpr::Atom(SourceAtom::Path(_)) => Some(atom),
                _ => None,
            }
            .expect("parsed value must be a path atom")
        }

        fn class(
            name: &str,
            mode: ClassExpansionMode,
        ) -> BooleanExpr<SourceAtom> {
            BooleanExpr::Atom(SourceAtom::Class {
                names: vec![name.to_owned()],
                mode,
            })
        }

        #[test]
        fn parses_tag_and_paths() {
            assert_eq!(
                SourceExpr::parse("#projects/active"),
                Ok(SourceExpr(tag("#projects/active")))
            );
            assert_eq!(
                SourceExpr::parse("\"books/dune.md\""),
                Ok(SourceExpr(path("books/dune.md")))
            );
            assert_eq!(
                SourceExpr::parse("books/"),
                Ok(SourceExpr(path("books/")))
            );
        }

        #[test]
        fn parses_each_class_expansion_form() {
            assert_eq!(
                SourceExpr::parse("@Book"),
                Ok(SourceExpr(class("Book", exact())))
            );
            assert_eq!(
                SourceExpr::parse("@Book+"),
                Ok(SourceExpr(class("Book", children())))
            );
            assert_eq!(
                SourceExpr::parse("@Book*"),
                Ok(SourceExpr(class("Book", descendants())))
            );
            assert_eq!(
                SourceExpr::parse("class(Book, children)"),
                Ok(SourceExpr(class("Book", children())))
            );
            assert_eq!(
                SourceExpr::parse("class(Book).with_descendants()"),
                Ok(SourceExpr(class("Book", descendants())))
            );
        }

        #[test]
        fn parses_precedence_grouping_and_repeated_negation() {
            assert_eq!(
                SourceExpr::parse("#book or books/ and not @Archived"),
                Ok(SourceExpr(BooleanExpr::Or(vec![
                    tag("#book"),
                    BooleanExpr::And(vec![
                        path("books/"),
                        BooleanExpr::Not(Box::new(class("Archived", exact()))),
                    ]),
                ])))
            );
            assert_eq!(
                SourceExpr::parse("(#book or #movie) and !archive/"),
                Ok(SourceExpr(BooleanExpr::And(vec![
                    BooleanExpr::Or(vec![tag("#book"), tag("#movie")]),
                    BooleanExpr::Not(Box::new(path("archive/"))),
                ])))
            );
            assert_eq!(
                SourceExpr::parse("not not #book and #movie and archive/"),
                Ok(SourceExpr(BooleanExpr::And(vec![
                    BooleanExpr::Not(Box::new(BooleanExpr::Not(Box::new(
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
                SourceExpr::parse("#book && !@Archived || @Movie"),
                Ok(SourceExpr(BooleanExpr::Or(_)))
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
                r"'books/dune.md",
                "class()",
                "class(Book, unknown)",
                "class(Book, children).with_descendants()",
                "class(Book).with_children() extra",
            ] {
                assert!(SourceExpr::parse(invalid).is_err(), "{invalid}");
            }
        }

        #[test]
        fn parses_a_single_star_wildcard_path_glob() {
            assert!(SourceExpr::parse("covers/*.md").is_ok());
        }

        #[test]
        fn parses_a_double_star_wildcard_path_glob() {
            assert!(SourceExpr::parse("**/draft.md").is_ok());
        }
    }

    mod constructors {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn collapses_disjunction_with_no_rest_to_bare_atom() {
            let atom = SourceAtom::Tag("book".to_owned());
            assert_eq!(
                SourceExpr::disjunction(atom.clone(), Vec::new()),
                SourceExpr::atom(atom)
            );
        }

        #[test]
        fn wraps_disjunction_with_rest_in_order() {
            let first = SourceAtom::Tag("book".to_owned());
            let second = SourceAtom::Tag("movie".to_owned());
            assert_eq!(
                SourceExpr::disjunction(first.clone(), vec![second.clone()]),
                SourceExpr(BooleanExpr::Or(vec![
                    BooleanExpr::Atom(first),
                    BooleanExpr::Atom(second),
                ]))
            );
        }

        #[test]
        fn collapses_conjunction_with_no_rest_to_first_term() {
            let term = SourceExpr::atom(SourceAtom::Tag("book".to_owned()));
            assert_eq!(SourceExpr::conjunction(term.clone(), Vec::new()), term);
        }

        #[test]
        fn wraps_conjunction_with_rest_in_order() {
            let first = SourceExpr::atom(SourceAtom::Tag("book".to_owned()));
            let second = SourceExpr::atom(SourceAtom::Tag("movie".to_owned()));
            assert_eq!(
                SourceExpr::conjunction(first.clone(), vec![second.clone()]),
                SourceExpr(BooleanExpr::And(vec![first.0, second.0]))
            );
        }
    }

    mod is_match {
        use std::{fs, path::Path};

        use super::*;
        use crate::{FileIndex, index::IndexerService};

        fn find_base<'a>(
            bases: &'a [crate::file::FileBase],
            path: &Path,
        ) -> &'a crate::file::FileBase {
            bases.iter().find(|r| r.path() == path).expect("base not found")
        }

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
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            (temp, index)
        }

        #[test]
        fn matches_boolean_combinations_of_tags_and_paths() {
            let (_temp, index) = indexed_note("#book", "books/dune.md");
            let file = find_base(index.bases(), Path::new("books/dune.md"));
            let note = index.note(Path::new("books/dune.md")).expect("Note");
            let expression =
                SourceExpr::parse("(#book and books/) and not archive/")
                    .expect("valid source");

            assert!(expression.is_match(file, Some(note), "class"));
        }

        #[test]
        fn matches_exact_file_path_without_matching_sibling() {
            let (_temp, index) = indexed_note("#book", "books/dune.md");
            let file = find_base(index.bases(), Path::new("books/dune.md"));
            let note = index.note(Path::new("books/dune.md")).expect("Note");

            assert!(
                SourceExpr::parse("books/dune.md")
                    .expect("valid source")
                    .is_match(file, Some(note), "class")
            );
            assert!(
                !SourceExpr::parse("books/hyperion.md")
                    .expect("valid source")
                    .is_match(file, Some(note), "class")
            );
        }

        #[test]
        fn reads_classes_from_the_execution_field() {
            let (_temp, index) =
                indexed_note("---\nkind: Book\n---\n", "book.md");
            let file = find_base(index.bases(), Path::new("book.md"));
            let note = index.note(Path::new("book.md")).expect("Note");
            let expression = SourceExpr::atom(SourceAtom::Class {
                names: vec!["Book".to_owned()],
                mode: ClassExpansionMode::Exact(BTreeSet::from([
                    "Book".to_owned()
                ])),
            });

            assert!(expression.is_match(file, Some(note), "kind"));
            assert!(!expression.is_match(file, Some(note), "class"));
        }

        #[test]
        fn matches_one_folder_level_but_not_subfolder() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("covers/sub"))
                .expect("create nested dir");
            fs::write(temp.path().join("covers/dune.md"), "# Dune")
                .expect("write direct file");
            fs::write(temp.path().join("covers/sub/hidden.md"), "# Hidden")
                .expect("write nested file");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let expression =
                SourceExpr::parse("covers/*.md").expect("valid source");
            let direct = find_base(index.bases(), Path::new("covers/dune.md"));
            let nested =
                find_base(index.bases(), Path::new("covers/sub/hidden.md"));

            assert!(expression.is_match(direct, None, "class"));
            assert!(!expression.is_match(nested, None, "class"));
        }

        #[test]
        fn crosses_folder_boundaries_with_double_star() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("covers/sub"))
                .expect("create nested dir");
            fs::write(temp.path().join("covers/dune.md"), "# Dune")
                .expect("write direct file");
            fs::write(temp.path().join("covers/sub/hidden.md"), "# Hidden")
                .expect("write nested file");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            // Requires an intermediate segment between `covers/` and the
            // final `*.md`, so a direct child does not match — proving
            // `**` (unlike `*`) crosses `/` boundaries, not merely that it
            // behaves like the `covers/` folder shorthand.
            let expression =
                SourceExpr::parse("covers/**/*.md").expect("valid source");
            let direct = find_base(index.bases(), Path::new("covers/dune.md"));
            let nested =
                find_base(index.bases(), Path::new("covers/sub/hidden.md"));

            assert!(!expression.is_match(direct, None, "class"));
            assert!(expression.is_match(nested, None, "class"));
        }
    }

    mod has_classes {
        use super::*;

        #[test]
        fn returns_true_when_expression_contains_class_atom() {
            let expr = SourceExpr::atom(SourceAtom::Class {
                names: vec!["project".to_owned()],
                mode: ClassExpansionMode::Children(BTreeSet::new()),
            });
            assert!(
                expr.has_classes(),
                "expression with Class atom must return true"
            );
        }

        #[test]
        fn returns_false_for_all_source() {
            let source = SourceSelector::All;
            assert!(
                !source.has_classes(),
                "SourceSelector::All must return false"
            );
        }
    }

    mod class_values_extraction {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn extracts_values_from_list_field() {
            let note = crate::note::parse_markdown(
                "test.md",
                "---\ntags:\n  - rust\n  - pkm\n---\nBody.",
            );
            let values: Vec<&str> = class_values(&note, "tags").collect();
            assert_eq!(values, vec!["rust", "pkm"]);
        }
    }

    mod parse_class_function {
        use super::*;

        #[test]
        fn parses_descendants_expansion_mode() {
            let result = SourceExpr::parse("class(Book, descendants)");
            assert!(
                result.is_ok(),
                "class(Book, descendants) must parse: {result:?}"
            );
        }
    }

    mod resolve_classes {
        use pretty_assertions::assert_eq;

        use super::*;

        /// Test double for [`FileClassExpander`]: records every call and
        /// resolves each class name to itself, so unresolved (not-yet-a-
        /// Schema) names are preserved rather than dropped.
        #[derive(Default)]
        struct RecordingExpander {
            calls: std::cell::RefCell<Vec<Vec<String>>>,
        }

        impl FileClassExpander for RecordingExpander {
            fn expand(
                &self,
                classes: &[String],
                mode: &mut ClassExpansionMode,
            ) {
                self.calls.borrow_mut().push(classes.to_vec());
                mode.set_classes(classes.iter().cloned().collect());
            }
        }

        #[test]
        fn walks_nested_expressions_and_preserves_unknowns() {
            let mut source = SourceSelector::parse("@thing+ and not @ghost*")
                .expect("source parses");
            let expander = RecordingExpander::default();

            source.resolve_classes(&expander);

            assert_eq!(expander.calls.into_inner(), vec![
                vec!["thing".to_owned()],
                vec!["ghost".to_owned()],
            ]);
        }
    }
}
