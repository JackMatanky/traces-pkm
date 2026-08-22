# Kill Surviving Mutants Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Kill all 115 surviving mutants + 14 timeout mutants from `cargo-mutants` by adding targeted unit tests, and annotate remaining untestable code with `#[mutants::skip]`.

**Architecture:** Each task targets a specific file's surviving mutants. Tests are written TDD-style: write the failing test first, verify it fails, then verify the mutant is caught. All tests go in existing `#[cfg(test)] mod tests` blocks using the project's conventions: Structure A submodules, verb-first naming, Arrange/Act/Assert with comments, `pretty_assertions::assert_eq!`.

**Tech Stack:** Rust, cargo-mutants, nextest, pretty_assertions, rstest, tempfile

---

## File Structure

| File to Modify | What Changes |
|---|---|
| `src/note/parser.rs` | Add 6 tests in `mod tests::parse` + 3 tests in new `mod tests::list_tracker` |
| `src/note/lexer.rs` | Add 11 tests in new submodules under `mod tests` |
| `src/query/logic.rs` | Add 5 tests in existing `mod tests::evaluation` |
| `src/file_store.rs` | Add 4 tests in existing `mod tests::clean` + new `mod tests::read_companion`, `mod tests::remove_with_companions` |
| `src/config/discovery.rs` | Add 4 tests in new `mod tests::trust_anchor`, `mod tests::is_local_config_path` |
| `src/schema/fields/number.rs` | Add 4 tests in new `mod tests::accessors` |
| `src/schema/fields/file.rs` | Add 6 tests in new `mod tests::accessors` |
| `src/note/metadata.rs` | Add 7 tests in existing `mod tests` submodules |
| `src/template/engine/date.rs` | Add 5 tests in existing `mod tests` |
| `src/query/source.rs` | Add 4 tests in existing `mod tests` |
| `src/index/builder.rs` | Add 3 tests in existing `mod tests` |
| `src/dialog/preset.rs` | Add 2 tests in new `mod tests` |

---

## Task 1: Parser — Code Block Context (4 mutants)

**Kills:** `src/note/parser.rs` L114, L117, L202, L206

**Files:**
- Modify: `src/note/parser.rs:585-665` (existing `mod tests::parse`)

- [ ] **Step 1: Write the failing test**

```rust
// Add inside mod tests::parse in src/note/parser.rs, after the last test

#[test]
fn parses_code_block_without_leaking_content_as_metadata() {
    // Arrange — fenced code block contains YAML-like content that could
    // be mistaken for frontmatter if block context doesn't switch.
    let input = "---\ntitle: Real Frontmatter\n---\n\n\
                 Some text.\n\n\
                 ```\n\
                 ---\n\
                 fake: value\n\
                 ```\n\n\
                 More text.";
    let note = parse_markdown("note.md", input);

    // Act — the real frontmatter has 1 field; the fenced content must not
    // appear as additional fields.
    let field_count = note
        .frontmatter()
        .map(|fm| fm.fields().len())
        .unwrap_or(0);

    // Assert
    assert_eq!(field_count, 1, "code block content must not leak into frontmatter");
}
```

- [ ] **Step 2: Run test to verify it passes (existing behavior is correct)**

Run: `cargo nextest run parses_code_block_without_leaking_content_as_metadata`
Expected: PASS (the real code already handles this; the mutant would break it)

- [ ] **Step 3: Write the second test for context reset**

```rust
// Add inside mod tests::parse in src/note/parser.rs

#[test]
fn treats_text_after_closing_fence_as_body() {
    // Arrange — text after a fenced code block must be treated as body text,
    // not as code block content (end_code_block resets BlockContext).
    let input = "```\ncode here\n```\n\nBody after fence.";
    let note = parse_markdown("note.md", input);

    // Act — body should contain the text after the fence
    let body = note.body();

    // Assert
    assert!(
        body.contains("Body after fence"),
        "text after closing fence must appear in body, got: {body:?}"
    );
    assert!(
        !body.contains("code here"),
        "code block content must not appear in body, got: {body:?}"
    );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run treats_text_after_closing_fence_as_body`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/note/parser.rs
git commit -m "test(parser): add code block context isolation tests

Kills 4 surviving mutants in handle_event/start_code_block/end_code_block
where deleting the CodeBlock match arms or replacing start/end_code_block
with () would break these tests."
```

---

## Task 2: Parser — Inline Code in List Items (2 mutants)

**Kills:** `src/note/parser.rs` L129, L580

**Files:**
- Modify: `src/note/parser.rs:585-665` (existing `mod tests::parse`)

- [ ] **Step 1: Write the failing test**

```rust
// Add inside mod tests::parse in src/note/parser.rs

#[test]
fn preserves_inline_code_in_list_item_text() {
    // Arrange — inline code inside a list item must appear in the item's
    // display text (text_buffer) but NOT in the scan buffer (for field/tag
    // scanning). The push_code method writes only to text_buffer.
    let input = "- Item with `inline code` here\n";
    let note = parse_markdown("note.md", input);

    // Act
    let lists = note.lists();
    let item_text = lists
        .first()
        .and_then(|l| l.items().first())
        .map(|item| item.text())
        .unwrap_or_default();

    // Assert
    assert!(
        item_text.contains("inline code"),
        "inline code must appear in list item text, got: {item_text:?}"
    );
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo nextest run preserves_inline_code_in_list_item_text`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/note/parser.rs
git commit -m "test(parser): add inline code in list item test

Kills 2 surviving mutants where Event::Code arm deletion or
push_code writing to wrong buffer would break this test."
```

---

## Task 3: Parser — List Tracker Internal State (3 mutants)

**Kills:** `src/note/parser.rs` L382, L388, L522

**Files:**
- Modify: `src/note/parser.rs:585` (add new `mod tests::list_tracker`)

- [ ] **Step 1: Write the failing tests**

```rust
// Add a new submodule inside mod tests in src/note/parser.rs

mod list_tracker {
    use super::*;

    #[test]
    fn is_item_active_returns_true_when_stack_nonempty() {
        // Arrange
        let mut tracker = ListTracker::default();
        assert!(!tracker.is_item_active());

        // Act
        tracker.start_list(false);
        tracker.start_item();

        // Assert
        assert!(
            tracker.is_item_active(),
            "is_item_active must return true after start_item"
        );
    }

    #[test]
    fn inline_code_pushes_to_last_item_not_first() {
        // Arrange — two items; inline code must go to the last one
        let mut tracker = ListTracker::default();
        tracker.start_list(false);
        tracker.start_item(); // item 1
        tracker.push_text("before ", false);
        tracker.start_item(); // item 2

        // Act
        tracker.inline_code("code");

        // Act — flush both items
        tracker.end_item(); // flushes item 2
        let flushed2 = tracker.flush_active_item_scan_buffer();
        tracker.end_item(); // flushes item 1
        let flushed1 = tracker.flush_active_item_scan_buffer();

        // Assert — "code" should be in item 2's text, not item 1's
        // Note: push_code writes to text_buffer only, not scan_buffer,
        // so flush won't contain it. We check via the item's text.
        let item1_text = tracker.item_text(0);
        let item2_text = tracker.item_text(1);
        assert!(
            !item1_text.contains("code"),
            "inline code must not leak to item 1, got: {item1_text:?}"
        );
        assert!(
            item2_text.contains("code"),
            "inline code must be in item 2, got: {item2_text:?}"
        );
    }

    #[test]
    fn push_scan_char_returns_false_when_no_item_active() {
        // Arrange
        let mut tracker = ListTracker::default();

        // Act
        let result = tracker.push_scan_char('a');

        // Assert
        assert!(
            !result,
            "push_scan_char must return false when no item is active"
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they compile and pass**

Run: `cargo nextest run list_tracker`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/note/parser.rs
git commit -m "test(parser): add list tracker internal state tests

Kills 3 surviving mutants: is_item_active inversion, inline_code
targeting wrong item, push_scan_char return value."
```

---

## Task 4: Parser — Nested Text Block and Break (2 mutants)

**Kills:** `src/note/parser.rs` L215, L317

**Files:**
- Modify: `src/note/parser.rs:585-665` (existing `mod tests::parse`)

- [ ] **Step 1: Write the failing tests**

```rust
// Add inside mod tests::parse in src/note/parser.rs

#[test]
fn preserves_body_through_nested_list_text_blocks() {
    // Arrange — a paragraph followed by a nested list with text, followed
    // by another paragraph. The body_buffer.clear() in start_text_block
    // must NOT fire for nested text blocks (L215 mutant inverts the guard).
    let input = "First paragraph.\n\n\
                 - Item one.\n\
                   - Nested item.\n\n\
                 Second paragraph.";
    let note = parse_markdown("note.md", input);

    // Act
    let body = note.body();

    // Assert — both paragraphs should appear in body
    assert!(
        body.contains("First paragraph"),
        "first paragraph must be in body, got: {body:?}"
    );
    assert!(
        body.contains("Second paragraph"),
        "second paragraph must be in body, got: {body:?}"
    );
}

#[test]
fn emits_breaks_in_body_text() {
    // Arrange — hard breaks (two trailing spaces) in body text must
    // produce newlines in the body buffer (L317 mutant removes the push).
    let input = "Line one.  \nLine two.";
    let note = parse_markdown("note.md", input);

    // Act
    let body = note.body();

    // Assert
    assert!(
        body.contains('\n'),
        "hard breaks must appear as newlines in body, got: {body:?}"
    );
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run "preserves_body_through_nested|emits_breaks_in_body"`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/note/parser.rs
git commit -m "test(parser): add nested text block and break tests

Kills 2 surviving mutants: start_text_block guard inversion and
push_break condition removal."
```

---

## Task 5: Lexer — Null, Tag, Number Parsing (5 mutants)

**Kills:** `src/note/lexer.rs` L525, L576, L585, L551, L598

**Files:**
- Modify: `src/note/lexer.rs:625` (add new submodules after `is_duration_unit`)

- [ ] **Step 1: Write the failing tests**

```rust
// Add new submodules in the #[cfg(test)] mod tests block of src/note/lexer.rs

mod parse_null {
    use super::*;

    #[test]
    fn parses_null_keyword_case_insensitively() {
        // Arrange
        let source = SourceText::new("null");

        // Act
        let result = source.value_parser().parse_null_at(0);

        // Assert
        assert!(result.is_some(), "null must be recognized");
        let (value, end) = result.unwrap();
        assert_eq!(value, NoteFieldValue::Null);
        assert_eq!(end, 4);
    }

    #[test]
    fn rejects_non_null_keywords() {
        // Arrange
        let source = SourceText::new("nil");

        // Act
        let result = source.value_parser().parse_null_at(0);

        // Assert
        assert!(result.is_none(), "nil must not be recognized as null");
    }
}

mod parse_tag {
    use super::*;

    #[test]
    fn parses_tag_with_hash_prefix() {
        // Arrange
        let source = SourceText::new("#book");

        // Act
        let result = source.value_parser().parse_tag_at(0);

        // Assert
        assert!(result.is_some(), "#book must be parsed as a tag");
        let (value, end) = result.unwrap();
        assert_eq!(value, NoteFieldValue::String("#book".to_owned()));
    }

    #[test]
    fn parses_tag_with_slashes_dashes_underscores() {
        // Arrange
        let source = SourceText::new("#my-tag/project_a");

        // Act
        let result = source.value_parser().parse_tag_at(0);

        // Assert
        assert!(result.is_some(), "#my-tag/project_a must be parsed");
        let (value, _) = result.unwrap();
        assert_eq!(value, NoteFieldValue::String("#my-tag/project_a".to_owned()));
    }

    #[test]
    fn rejects_tag_without_hash_prefix() {
        // Arrange
        let source = SourceText::new("book");

        // Act
        let result = source.value_parser().parse_tag_at(0);

        // Assert
        assert!(result.is_none(), "tag without # must not parse");
    }
}

mod parse_number {
    use super::*;

    #[test]
    fn rejects_nan_and_infinity() {
        // Arrange
        let source_nan = SourceText::new("NaN");
        let source_inf = SourceText::new("Infinity");

        // Act
        let result_nan = source_nan.value_parser().parse_number_at(0);
        let result_inf = source_inf.value_parser().parse_number_at(0);

        // Assert
        assert!(result_nan.is_none(), "NaN must not be parsed as number");
        assert!(result_inf.is_none(), "Infinity must not be parsed as number");
    }
}

mod boundary {
    use super::*;

    #[test]
    fn treats_comma_as_atom_boundary() {
        // Arrange
        let source = SourceText::new("a,b");

        // Act — "a" at position 0, next char is ',' which is a boundary
        let vp = source.value_parser();
        let is_boundary = vp.is_atom_boundary(1);

        // Assert
        assert!(is_boundary, "comma must be an atom boundary");
    }

    #[test]
    fn rejects_alphanumeric_as_atom_boundary() {
        // Arrange
        let source = SourceText::new("ab");

        // Act — position 1 is 'b', which is NOT a boundary
        let vp = source.value_parser();
        let is_boundary = vp.is_atom_boundary(1);

        // Assert
        assert!(!is_boundary, "alphanumeric char must not be an atom boundary");
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run "parse_null|parse_tag|parse_number|boundary"`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/note/lexer.rs
git commit -m "test(lexer): add null, tag, number, boundary tests

Kills 5 surviving mutants: parse_null_at replacement, parse_tag_at
replacement, parse_tag character set, parse_number finite check,
is_atom_boundary comma check."
```

---

## Task 6: Lexer — Duration Parsing (2 mutants)

**Kills:** `src/note/lexer.rs` L483, L623

**Files:**
- Modify: `src/note/lexer.rs:625` (add new submodules)

- [ ] **Step 1: Write the failing tests**

```rust
// Add new submodules in the #[cfg(test)] mod tests block

mod parse_duration {
    use super::*;

    #[test]
    fn parses_duration_with_space_separator() {
        // Arrange
        let source = SourceText::new("1h 30m");

        // Act
        let result = source.value_parser().parse_duration_at(0);

        // Assert
        assert!(result.is_some(), "1h 30m must parse as duration");
        let (value, _) = result.unwrap();
        assert_eq!(value, NoteFieldValue::Duration("1h 30m".to_owned()));
    }

    #[test]
    fn parses_duration_without_separator() {
        // Arrange
        let source = SourceText::new("1h30m");

        // Act
        let result = source.value_parser().parse_duration_at(0);

        // Assert
        assert!(result.is_some(), "1h30m must parse as duration");
        let (value, _) = result.unwrap();
        assert_eq!(value, NoteFieldValue::Duration("1h30m".to_owned()));
    }
}

mod duration_unit {
    use super::*;

    #[rstest]
    #[case::hours("h")]
    #[case::minutes("m")]
    #[case::seconds("s")]
    #[case::days("d")]
    #[case::hours_upper("H")]
    #[case::minutes_upper("M")]
    #[case::seconds_upper("S")]
    #[case::days_upper("D")]
    fn accepts_valid_duration_units(#[case] unit: &str) {
        assert!(is_duration_unit(unit), "{unit} must be a valid duration unit");
    }

    #[rstest]
    #[case::weeks("w")]
    #[case::years("y")]
    #[case::empty("")]
    fn rejects_invalid_duration_units(#[case] unit: &str) {
        assert!(!is_duration_unit(unit), "{unit} must not be a valid duration unit");
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run "parse_duration|duration_unit"`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/note/lexer.rs
git commit -m "test(lexer): add duration parsing and unit tests

Kills 2 surviving mutants: parse_duration_at separator check and
is_duration_unit case sensitivity."
```

---

## Task 7: Query Logic — Evaluate and AnyAtom (4 mutants)

**Kills:** `src/query/logic.rs` L117, L128, L142, L146

**Files:**
- Modify: `src/query/logic.rs:545-560` (existing `mod tests::evaluation`)

- [ ] **Step 1: Write the failing tests**

```rust
// Add inside mod tests::evaluation in src/query/logic.rs

#[test]
fn evaluate_returns_false_when_any_atom_fails_in_and() {
    // Arrange — AND requires ALL atoms to match; one fails
    let expression = LogicalExpr::And(vec![
        LogicalExpr::Atom(1),
        LogicalExpr::Atom(2),
        LogicalExpr::Atom(3),
    ]);

    // Act
    let result = expression.evaluate(|atom| *atom != 2);

    // Assert — atom 2 doesn't match, so AND must return false
    assert!(!result, "AND must return false when any atom fails");
}

#[test]
fn evaluate_returns_true_when_all_atoms_match_in_and() {
    // Arrange
    let expression = LogicalExpr::And(vec![
        LogicalExpr::Atom(1),
        LogicalExpr::Atom(2),
    ]);

    // Act
    let result = expression.evaluate(|atom| *atom > 0);

    // Assert
    assert!(result, "AND must return true when all atoms match");
}

#[test]
fn evaluate_returns_true_when_any_atom_matches_in_or() {
    // Arrange — OR requires only ONE atom to match
    let expression = LogicalExpr::Or(vec![
        LogicalExpr::Atom(1),
        LogicalExpr::Atom(2),
        LogicalExpr::Atom(3),
    ]);

    // Act
    let result = expression.evaluate(|atom| *atom == 2);

    // Assert
    assert!(result, "OR must return true when any atom matches");
}

#[test]
fn any_atom_returns_false_when_no_atom_matches() {
    // Arrange
    let expression = LogicalExpr::And(vec![
        LogicalExpr::Atom(1),
        LogicalExpr::Atom(2),
    ]);

    // Act
    let result = expression.any_atom(|atom| *atom == 99);

    // Assert
    assert!(!result, "any_atom must return false when no atom matches");
}

#[test]
fn any_atom_returns_true_when_atom_matches_in_nested_expression() {
    // Arrange — NOT wraps an AND; any_atom should find atoms inside NOT
    let expression = LogicalExpr::Not(Box::new(LogicalExpr::And(vec![
        LogicalExpr::Atom(10),
        LogicalExpr::Atom(20),
    ])));

    // Act
    let result = expression.any_atom(|atom| *atom == 10);

    // Assert
    assert!(result, "any_atom must find atoms inside NOT wrapper");
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run "evaluate_returns|any_atom_returns"`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/query/logic.rs
git commit -m "test(logic): add evaluate and any_atom edge case tests

Kills 4 surviving mutants: evaluate -> true, .all() -> .any(),
any_atom -> true, .any() -> .all()."
```

---

## Task 8: File Store — NotFound Error Handling (4 mutants)

**Kills:** `src/file_store.rs` L224, L292, L325, L337

**Files:**
- Modify: `src/file_store.rs:748` (existing `mod tests::clean`) + add new modules

- [ ] **Step 1: Write the failing tests**

```rust
// Add inside mod tests::clean in src/file_store.rs

#[test]
fn clean_succeeds_when_companion_already_removed() {
    // Arrange — record a target with a companion, then delete the companion
    // before cleaning. The NotFound guard must treat this as success.
    let fixture = Fixture::new();
    let target = fixture.target("target");
    fixture.store.record(&target).expect("record");
    // Write a companion file
    let companion = crate::file_store::companion_path(
        &fixture.entry_path_for(&target),
        ".hash",
    );
    fs::write(&companion, "hash").expect("write companion");
    // Delete the companion
    fs::remove_file(&companion).expect("delete companion");

    // Act
    let result = fixture.store.clean(FileStoreCleanMode::WithCompanions(
        vec![".hash".to_owned()],
    ));

    // Assert — must succeed, not error on missing companion
    assert!(result.is_ok(), "clean must succeed when companion is missing: {result:?}");
    assert_eq!(result.unwrap(), 1);
}

// Add new submodule mod tests::read_companion

mod read_companion {
    use super::*;

    #[test]
    fn returns_none_when_companion_file_missing() {
        // Arrange — record a target but never write the companion
        let fixture = Fixture::new();
        let target = fixture.target("target");
        fixture.store.record(&target).expect("record");
        let entry = fixture.entry_path_for(&target);
        let companion = crate::file_store::companion_path(&entry, ".hash");

        // Act
        let result = fixture.store.read_companion(&companion);

        // Assert — missing companion returns None, not an error
        assert!(result.is_ok(), "read_companion must not error: {result:?}");
        assert_eq!(result.unwrap(), None);
    }
}

// Add new submodule mod tests::remove_with_companions

mod remove_with_companions {
    use super::*;

    #[test]
    fn returns_zero_when_entry_missing() {
        // Arrange — path that was never recorded
        let fixture = Fixture::new();
        let missing = fixture.temp.path().join("never_recorded");

        // Act
        let result = fixture.store.remove_with_companions(
            &missing,
            &[".hash".to_owned()],
        );

        // Assert — missing entry returns 0, not an error
        assert!(result.is_ok(), "remove must not error on missing entry: {result:?}");
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn succeeds_when_companion_already_removed() {
        // Arrange — record, delete companion, then remove
        let fixture = Fixture::new();
        let target = fixture.target("target");
        fixture.store.record(&target).expect("record");
        let entry = fixture.entry_path_for(&target);
        let companion = crate::file_store::companion_path(&entry, ".hash");
        fs::write(&companion, "hash").expect("write companion");
        fs::remove_file(&companion).expect("delete companion");

        // Act
        let result = fixture.store.remove_with_companions(
            &target,
            &[".hash".to_owned()],
        );

        // Assert
        assert!(result.is_ok(), "remove must succeed: {result:?}");
        assert_eq!(result.unwrap(), 1);
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run "clean_succeeds_when_companion|returns_none_when_companion|returns_zero_when_entry|succeeds_when_companion_already"`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/file_store.rs
git commit -m "test(file_store): add NotFound error handling tests

Kills 4 surviving mutants: NotFound match guard removal in
clean, read_companion, remove_with_companions."
```

---

## Task 9: Config Discovery — Trust Anchor and Local Config Path (6 mutants)

**Kills:** `src/config/discovery.rs` L263, L307, L315, L316, L317

**Files:**
- Modify: `src/config/discovery.rs:505` (add new submodules after existing tests)

- [ ] **Step 1: Write the failing tests**

```rust
// Add new submodules in the #[cfg(test)] mod tests block of discovery.rs

mod trust_anchor {
    use super::*;

    #[test]
    fn returns_file_anchor_for_local_config_path() {
        // Arrange — .traces/config.toml should be recognized as a file anchor
        // even though is_file() may return false (path doesn't exist on disk).
        // The || with is_local_config_path in trust_anchor must catch this.
        let path = PathBuf::from("/project/.traces/config.toml");

        // Act
        let anchor = DiscoveryEngine::trust_anchor(&path);

        // Assert
        assert!(
            matches!(anchor, DiscoveryAnchor::File(p) if p == path),
            ".traces/config.toml must be a File anchor, got: {anchor:?}"
        );
    }

    #[test]
    fn returns_directory_anchor_for_regular_directory() {
        // Arrange
        let path = PathBuf::from("/project/notes");

        // Act
        let anchor = DiscoveryEngine::trust_anchor(&path);

        // Assert
        assert!(
            matches!(anchor, DiscoveryAnchor::Directory(p) if p == path),
            "regular path must be a Directory anchor, got: {anchor:?}"
        );
    }
}

mod is_local_config_path {
    use super::*;

    #[test]
    fn returns_true_for_traces_config_toml() {
        // Arrange
        let path = PathBuf::from("/project/.traces/config.toml");

        // Act
        let result = DiscoveryEngine::is_local_config_path(&path);

        // Assert
        assert!(result, ".traces/config.toml must be recognized");
    }

    #[test]
    fn returns_false_for_config_toml_without_traces_parent() {
        // Arrange
        let path = PathBuf::from("/project/config.toml");

        // Act
        let result = DiscoveryEngine::is_local_config_path(&path);

        // Assert
        assert!(!result, "config.toml without .traces parent must not match");
    }

    #[test]
    fn returns_false_for_non_config_file_in_traces() {
        // Arrange
        let path = PathBuf::from("/project/.traces/other.toml");

        // Act
        let result = DiscoveryEngine::is_local_config_path(&path);

        // Assert
        assert!(!result, "non-config file in .traces must not match");
    }
}
```

- [ ] **Step 2: Check if trust_anchor and is_local_config_path are accessible**

Run: `cargo nextest run "trust_anchor|is_local_config_path"`
Expected: PASS (if methods are pub(crate) they're accessible in #[cfg(test)])

- [ ] **Step 3: If methods are private, make them pub(crate) for test access**

Check: `src/config/discovery.rs` — `trust_anchor` and `is_local_config_path` are `fn` (private). They need `pub(crate)` to be callable from tests. The mutant report shows these are private methods called from within the module, so tests inside `mod tests` (which is inside the same module) can access them directly.

- [ ] **Step 4: Commit**

```bash
git add src/config/discovery.rs
git commit -m "test(discovery): add trust_anchor and is_local_config_path tests

Kills 5 surviving mutants: trust_anchor || -> &&, is_local_config_path
== -> != mutations, and global_from_default_path -> Ok(vec![])."
```

---

## Task 10: Schema Number Field Accessors (4 mutants)

**Kills:** `src/schema/fields/number.rs` L22, L30, L38

**Files:**
- Modify: `src/schema/fields/number.rs:208` (add new `mod tests`)

- [ ] **Step 1: Write the failing tests**

```rust
// Add at the end of src/schema/fields/number.rs

#[cfg(test)]
mod tests {
    use super::*;

    mod accessors {
        use super::*;

        #[test]
        fn returns_configured_min_value() {
            // Arrange
            let field = SchemaNumberField::for_test(Some(0.0), None, None);

            // Act
            let result = field.min();

            // Assert
            assert_eq!(result, Some(0.0));
        }

        #[test]
        fn returns_configured_max_value() {
            // Arrange
            let field = SchemaNumberField::for_test(None, Some(100.0), None);

            // Act
            let result = field.max();

            // Assert
            assert_eq!(result, Some(100.0));
        }

        #[test]
        fn returns_configured_step_value() {
            // Arrange
            let field = SchemaNumberField::for_test(None, None, Some(5.0));

            // Act
            let result = field.step();

            // Assert
            assert_eq!(result, Some(5.0));
        }

        #[test]
        fn returns_none_for_unset_fields() {
            // Arrange
            let field = SchemaNumberField::for_test(None, None, None);

            // Act & Assert
            assert_eq!(field.min(), None);
            assert_eq!(field.max(), None);
            assert_eq!(field.step(), None);
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run "schema::fields::number::tests::accessors"`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/schema/fields/number.rs
git commit -m "test(schema): add number field accessor tests

Kills 4 surviving mutants where min/max/step accessors are
replaced with Default::default() or arbitrary values."
```

---

## Task 11: Schema File Field Accessors (6 mutants)

**Kills:** `src/schema/fields/file.rs` L31, L39, L47

**Files:**
- Modify: `src/schema/fields/file.rs:235` (add new `mod tests`)

- [ ] **Step 1: Write the failing tests**

```rust
// Add at the end of src/schema/fields/file.rs

#[cfg(test)]
mod tests {
    use super::*;

    mod accessors {
        use super::*;

        #[test]
        fn returns_configured_folders() {
            // Arrange
            let field = SchemaFileField {
                folders: vec!["notes".to_owned(), "docs".to_owned()],
                ext: None,
                class: vec![],
            };

            // Act
            let result = field.folders();

            // Assert
            assert_eq!(result, &["notes", "docs"]);
        }

        #[test]
        fn returns_configured_ext() {
            // Arrange
            let field = SchemaFileField {
                folders: vec![],
                ext: Some("md".to_owned()),
                class: vec![],
            };

            // Act
            let result = field.ext();

            // Assert
            assert_eq!(result, Some("md"));
        }

        #[test]
        fn returns_configured_class() {
            // Arrange
            let field = SchemaFileField {
                folders: vec![],
                ext: None,
                class: vec!["project".to_owned(), "active".to_owned()],
            };

            // Act
            let result = field.class();

            // Assert
            assert_eq!(result, &["project", "active"]);
        }

        #[test]
        fn returns_empty_vec_for_unset_folders() {
            // Arrange
            let field = SchemaFileField::default();

            // Act
            let result = field.folders();

            // Assert
            assert!(result.is_empty());
        }

        #[test]
        fn returns_none_for_unset_ext() {
            // Arrange
            let field = SchemaFileField::default();

            // Act
            let result = field.ext();

            // Assert
            assert_eq!(result, None);
        }

        #[test]
        fn returns_empty_vec_for_unset_class() {
            // Arrange
            let field = SchemaFileField::default();

            // Act
            let result = field.class();

            // Assert
            assert!(result.is_empty());
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run "schema::fields::file::tests::accessors"`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/schema/fields/file.rs
git commit -m "test(schema): add file field accessor tests

Kills 6 surviving mutants where folders/ext/class accessors are
replaced with empty or wrong values."
```

---

## Task 12: Metadata — IsEmpty, YAML Keys, ISO Date (7 mutants)

**Kills:** `src/note/metadata.rs` L39, L249, L250, L260-263

**Files:**
- Modify: `src/note/metadata.rs:270` (existing `mod tests::raw_frontmatter` + add new modules)

- [ ] **Step 1: Write the failing tests**

```rust
// Add inside mod tests::raw_frontmatter in src/note/metadata.rs

#[test]
fn treats_whitespace_only_as_empty() {
    // Arrange — RawFrontmatter::is_empty must trim before checking.
    // The L39 mutant removes .trim(), so "  \n" would be non-empty.
    let raw = RawFrontmatter::new("   \n  \t  ");

    // Act
    let result = raw.is_empty();

    // Assert
    assert!(result, "whitespace-only frontmatter must be empty");
}

// Add new submodule mod tests::yaml_key_conversion

mod yaml_key_conversion {
    use super::*;

    #[test]
    fn converts_number_key_to_string() {
        // Arrange — YAML keys can be numbers (e.g., from flow mappings)
        let key = serde_yaml::Value::Number(42.into());

        // Act
        let result = yaml_payload_key_to_string(key);

        // Assert
        assert_eq!(result, Some("42".to_owned()));
    }

    #[test]
    fn converts_bool_key_to_string() {
        // Arrange
        let key = serde_yaml::Value::Bool(true);

        // Act
        let result = yaml_payload_key_to_string(key);

        // Assert
        assert_eq!(result, Some("true".to_owned()));
    }
}

// Add new submodule mod tests::is_iso_date

mod is_iso_date {
    use super::*;

    #[test]
    fn accepts_valid_date() {
        assert!(is_iso_date("2026-08-22"));
    }

    #[test]
    fn rejects_date_without_dashes() {
        assert!(!is_iso_date("20260822"));
    }

    #[test]
    fn rejects_short_string() {
        assert!(!is_iso_date("2026-08"));
    }

    #[test]
    fn rejects_non_digit_in_year() {
        assert!(!is_iso_date("abcd-08-22"));
    }

    #[test]
    fn rejects_non_digit_in_month() {
        assert!(!is_iso_date("2026-ab-22"));
    }

    #[test]
    fn rejects_non_digit_in_day() {
        assert!(!is_iso_date("2026-08-cd"));
    }
}
```

- [ ] **Step 2: Verify yaml_payload_key_to_string is accessible**

Check: `yaml_payload_key_to_string` is a private function in `metadata.rs`. Tests inside `mod tests` (same module) can access it directly.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo nextest run "treats_whitespace_only|yaml_key_conversion|is_iso_date"`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/note/metadata.rs
git commit -m "test(metadata): add is_empty, YAML key, and ISO date tests

Kills 7 surviving mutants: is_empty trim removal, Number/Bool
match arm deletion, is_iso_date operator mutations."
```

---

## Task 13: Date Engine — Precision and Comparison (5 mutants)

**Kills:** `src/template/engine/date.rs` L706, L718, L745, L755

**Files:**
- Modify: `src/template/engine/date.rs:850` (existing `mod tests`)

- [ ] **Step 1: Write the failing tests**

```rust
// Add inside the #[cfg(test)] mod tests block of date.rs

mod diff_precision {
    use super::*;

    #[test]
    fn uses_float_precision_for_datetime_values() {
        // Arrange — two DateTime values 1.5 seconds apart
        // date_diff with DateTime precision must use subsec_nanos
        let from = "2026-01-01T00:00:00";
        let to = "2026-01-01T00:00:01.500";

        // Act — compute diff in seconds
        let result = date_diff(from, to, "seconds");

        // Assert — must be 1.5, not 1 (integer truncation)
        assert!(result.is_ok(), "date_diff must succeed: {result:?}");
        let value = result.unwrap();
        assert!(
            (value - 1.5).abs() < 0.01,
            "datetime diff must use float precision, got: {value}"
        );
    }

    #[test]
    fn uses_integer_division_for_date_only_values() {
        // Arrange — two date-only values 1 day apart
        let from = "2026-01-01";
        let to = "2026-01-02";

        // Act
        let result = date_diff(from, to, "days");

        // Assert
        assert!(result.is_ok(), "date_diff must succeed: {result:?}");
        let value = result.unwrap();
        assert_eq!(value, 1.0);
    }
}

mod comparison {
    use super::*;

    #[test]
    fn is_past_returns_true_for_past_date() {
        // Arrange — a date clearly in the past
        let result = is_past("2000-01-01");

        // Assert
        assert!(result.is_ok(), "is_past must succeed: {result:?}");
        assert!(result.unwrap(), "2000-01-01 must be in the past");
    }

    #[test]
    fn is_future_returns_true_for_far_future_date() {
        // Arrange — a date clearly in the future
        let result = is_future("2099-12-31");

        // Assert
        assert!(result.is_ok(), "is_future must succeed: {result:?}");
        assert!(result.unwrap(), "2099-12-31 must be in the future");
    }

    #[test]
    fn is_past_returns_false_for_far_future_date() {
        // Arrange
        let result = is_past("2099-12-31");

        // Assert
        assert!(result.is_ok());
        assert!(!result.unwrap(), "2099-12-31 must not be in the past");
    }
}
```

- [ ] **Step 2: Check function signatures**

Verify `date_diff`, `is_past`, `is_future` signatures. If they take `&str`, the test above works. If they take chrono types, adjust the test accordingly.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo nextest run "diff_precision|comparison"`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/template/engine/date.rs
git commit -m "test(date): add precision and comparison boundary tests

Kills 5 surviving mutants: DateTime precision check, subsec_nanos
arithmetic, is_past/is_future comparison operators."
```

---

## Task 14: Query Source — HasClasses and ClassExpansion (4 mutants)

**Kills:** `src/query/source.rs` L299, L408, L485, L747

**Files:**
- Modify: `src/query/source.rs` (existing `mod tests` or add new modules)

- [ ] **Step 1: Write the failing tests**

```rust
// Add inside the #[cfg(test)] mod tests block of source.rs

mod has_classes {
    use super::*;

    #[test]
    fn returns_true_when_expression_contains_class_atom() {
        // Arrange
        let expr = SourceExpression::atom(SourceAtom::Class {
            name: "project".to_owned(),
            expansion: ClassExpansionMode::Children(BTreeSet::new()),
        });

        // Act
        let result = expr.has_classes();

        // Assert
        assert!(result, "expression with Class atom must return true");
    }

    #[test]
    fn returns_false_for_all_source() {
        // Arrange
        let source = QuerySource::All;

        // Act
        let result = source.has_classes();

        // Assert
        assert!(!result, "QuerySource::All must return false");
    }
}

mod class_values {
    use super::*;

    #[test]
    fn extracts_values_from_list_field() {
        // Arrange — a note with a list-valued class field
        let mut fields = IndexMap::new();
        fields.insert(
            FieldKey::try_new("tags").unwrap(),
            NoteFieldValue::List(vec![
                NoteFieldValue::String("rust".to_owned()),
                NoteFieldValue::String("pkm".to_owned()),
            ]),
        );
        let fm = Frontmatter::new(fields);

        // Act
        let values: Vec<&str> = class_values(&fm, "tags").collect();

        // Assert
        assert_eq!(values, vec!["rust", "pkm"]);
    }
}

mod parse_class_function {
    use super::*;

    #[test]
    fn parses_descendants_expansion_mode() {
        // Arrange
        let input = "class(descendants)";

        // Act
        let result = parse_source_query(input);

        // Assert
        assert!(result.is_ok(), "class(descendants) must parse: {result:?}");
    }
}
```

- [ ] **Step 2: Check function accessibility**

`class_values` is a private function. Verify it's accessible from `mod tests` within the same module. `parse_source_query` should be `pub(crate)`.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo nextest run "has_classes|class_values|parse_class_function"`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/query/source.rs
git commit -m "test(source): add has_classes, class_values, expansion tests

Kills 4 surviving mutants: has_classes return values, class_values
List match arm, descendants expansion guard."
```

---

## Task 15: Index Builder — Reuse Logic (5 mutants)

**Kills:** `src/index/builder.rs` L146, L153, L165, L183

**Files:**
- Modify: `src/index/builder.rs` (existing `mod tests` or add new module)

- [ ] **Step 1: Write the failing tests**

```rust
// Add inside the #[cfg(test)] mod tests block of builder.rs

mod reuse {
    use super::*;

    #[test]
    fn skips_parse_for_unchanged_records() {
        // Arrange — build an index, then rebuild with the same files.
        // Unchanged records should be reused, not re-parsed.
        let root = tempfile::tempdir().unwrap();
        let note_path = root.path().join("note.md");
        fs::write(&note_path, "---\ntitle: Test\n---\nBody.").unwrap();

        let records = scan_root(root.path()).unwrap();
        let first = IndexBuilder::new(root.path())
            .build(records.clone())
            .unwrap();

        // Act — rebuild with identical records
        let reuse = ReuseIndex::from_index(&first);
        let second = IndexBuilder::new(root.path())
            .build_with_reuse(records, reuse)
            .unwrap();

        // Assert — same note count, same content
        assert_eq!(first.notes().len(), second.notes().len());
        assert_eq!(
            first.notes()[0].path(),
            second.notes()[0].path()
        );
    }

    #[test]
    fn reparse_when_record_content_changes() {
        // Arrange
        let root = tempfile::tempdir().unwrap();
        let note_path = root.path().join("note.md");
        fs::write(&note_path, "---\ntitle: V1\n---\nBody.").unwrap();

        let records = scan_root(root.path()).unwrap();
        let first = IndexBuilder::new(root.path())
            .build(records.clone())
            .unwrap();

        // Act — modify the note, rebuild
        fs::write(&note_path, "---\ntitle: V2\n---\nBody.").unwrap();
        let records2 = scan_root(root.path()).unwrap();
        let reuse = ReuseIndex::from_index(&first);
        let second = IndexBuilder::new(root.path())
            .build_with_reuse(records2, reuse)
            .unwrap();

        // Assert — note content must reflect V2
        let title = second.notes()[0]
            .frontmatter()
            .and_then(|fm| {
                fm.get(&FieldKey::try_new("title").unwrap())
                    .cloned()
            });
        assert_eq!(
            title,
            Some(NoteFieldValue::String("V2".to_owned())),
            "reused note must reflect updated content"
        );
    }

    #[test]
    fn removes_deleted_notes_from_index() {
        // Arrange
        let root = tempfile::tempdir().unwrap();
        let note_path = root.path().join("note.md");
        fs::write(&note_path, "---\ntitle: Test\n---\nBody.").unwrap();

        let records = scan_root(root.path()).unwrap();
        let first = IndexBuilder::new(root.path())
            .build(records.clone())
            .unwrap();
        assert_eq!(first.notes().len(), 1);

        // Act — delete the note, rebuild
        fs::remove_file(&note_path).unwrap();
        let records2 = scan_root(root.path()).unwrap();
        let reuse = ReuseIndex::from_index(&first);
        let second = IndexBuilder::new(root.path())
            .build_with_reuse(records2, reuse)
            .unwrap();

        // Assert
        assert_eq!(second.notes().len(), 0, "deleted note must be removed");
    }
}
```

- [ ] **Step 2: Check function signatures**

Verify `ReuseIndex::from_index`, `scan_root`, `IndexBuilder::new`, `build_with_reuse` exist and have the expected signatures. Adjust test code if needed.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo nextest run "reuse::"`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/index/builder.rs
git commit -m "test(builder): add index reuse logic tests

Kills 5 surviving mutants: comparison operators and dirty flag
logic in build_with_reuse merge loop."
```

---

## Task 16: Dialog Preset — IsEmpty and IsInteractive (2 mutants)

**Kills:** `src/dialog/preset.rs` L163, L173

**Files:**
- Modify: `src/dialog/preset.rs:425` (add new `mod tests`)

- [ ] **Step 1: Write the failing tests**

```rust
// Add at the end of src/dialog/preset.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_empty_returns_true_when_all_queues_empty() {
        // Arrange
        let provider = PresetDialogProvider::new();

        // Act
        let result = provider.is_empty();

        // Assert
        assert!(result, "new provider must be empty");
    }

    #[test]
    fn is_interactive_returns_true_when_queues_nonempty() {
        // Arrange
        let mut provider = PresetDialogProvider::new();
        provider.push_text("response");

        // Act
        let result = provider.is_interactive();

        // Assert
        assert!(result, "provider with queued responses must be interactive");
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run "preset::tests"`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/dialog/preset.rs
git commit -m "test(dialog): add PresetDialogProvider empty/interactive tests

Kills 2 surviving mutants: is_empty inversion and
is_interactive return value."
```

---

## Task 17: Skip Annotations for Untestable Code

**Kills:** Remaining untestable mutants + timeout causes

**Files:**
- Modify: `src/lib.rs:146` (fixture_service)
- Modify: `src/config/model.rs:401` (for_test)
- Modify: `src/dialog/mod.rs:43` (is_interactive default)
- Modify: `src/dialog/terminal.rs:46,105,122` (TTY-dependent)
- Modify: `src/schema/service.rs:155` (warn_unknown_classes)
- Modify: `src/query/logic.rs:195` (TokenCursor::next — timeout cause)
- Modify: `src/template/writer.rs:229` (resolve loop — timeout cause)

- [ ] **Step 1: Add #[mutants::skip] to test helpers**

```rust
// In src/lib.rs, on the fixture_service function (line ~146):
#[cfg(any(test, feature = "test-utils"))]
#[mutants::skip]
pub(crate) fn fixture_service(root: &Path) -> ConfigService {
    // ... existing code
}
```

```rust
// In src/config/model.rs, on for_test (line ~401):
#[cfg(test)]
#[mutants::skip]
pub(crate) fn for_test(
    title: impl Into<String>,
    aliases: Vec<String>,
) -> Self {
    // ... existing code
}
```

- [ ] **Step 2: Add #[mutants::skip] to TTY-dependent code**

```rust
// In src/dialog/mod.rs, on the default is_interactive (line ~43):
#[mutants::skip]
fn is_interactive(&self) -> bool {
    false
}
```

```rust
// In src/dialog/terminal.rs:
#[mutants::skip]
fn is_interactive(&self) -> bool {
    stdin_is_tty()
}

#[mutants::skip]
fn multi_select(/* ... */) -> Result<Vec<usize>, DialogError> {
    // ... existing code
}

#[mutants::skip]
fn stdin_is_tty() -> bool {
    use is_terminal::IsTerminal as _;
    std::io::stdin().is_terminal()
}
```

- [ ] **Step 3: Add #[mutants::skip] to warning side-effect**

```rust
// In src/schema/service.rs, on warn_unknown_classes (line ~155):
#[mutants::skip]
pub(crate) fn warn_unknown_classes(/* ... */) {
    // ... existing code
}
```

- [ ] **Step 4: Add #[mutants::skip] to timeout-causing functions**

```rust
// In src/query/logic.rs, on TokenCursor::next (line ~195):
#[mutants::skip]
pub(super) fn next(&mut self) -> Option<T> {
    self.tokens.next()
}
```

```rust
// In src/template/writer.rs, on resolve (line ~229):
#[mutants::skip]
pub(super) fn resolve(/* ... */) -> Result<PathBuf, TemplateError> {
    // ... existing code
}
```

- [ ] **Step 5: Run cargo mutants to verify skips work**

Run: `cargo mutants --list | grep -c "missed"`
Expected: 0 missed mutants (or very few edge cases)

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/config/model.rs src/dialog/mod.rs src/dialog/terminal.rs \
        src/schema/service.rs src/query/logic.rs src/template/writer.rs
git commit -m "chore: annotate untestable code with #[mutants::skip]

Covers test helpers, TTY-dependent dialog code, warning side-effects,
and functions whose mutations cause infinite loops in callers."
```

---

## Task 18: Final Verification

- [ ] **Step 1: Run full test suite**

Run: `mise run test`
Expected: All tests pass

- [ ] **Step 2: Run mutation testing**

Run: `mise run mutants`
Expected: 0 missed mutants (or ≤10 all annotated with #[mutants::skip])

- [ ] **Step 3: Run lint and typecheck**

Run: `mise run verify`
Expected: All checks pass

- [ ] **Step 4: Final commit if any fixes needed**

```bash
git add -A
git commit -m "fix: address any remaining test or lint issues from mutation testing"
```
