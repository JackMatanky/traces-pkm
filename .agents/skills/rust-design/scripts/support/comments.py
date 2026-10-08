"""Lexical Rust source code tokenizer for SLoC and comment analysis.

Provides accurate classification of Rust lines into executable code (SLoC),
documentation comments (///, //!, /** ... */), and implementation comments
(//, /* ... */). Correctly tracks nested block comments, raw string literals
(r#"..."#), standard strings, and character literals across line boundaries.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(slots=True)
class LineLexResult:
    """Lexical classification result for a single source line."""

    has_code: bool
    has_doc_comment: bool
    has_impl_comment: bool

    @property
    def has_any_comment(self) -> bool:
        """True if line has any documentation or implementation comment."""
        return self.has_doc_comment or self.has_impl_comment


class RustCommentLexer:
    """Stateful tokenizer for comments, strings, and SLoC in Rust source."""

    def __init__(self) -> None:
        self.block_comment_depth: int = 0
        self.is_block_doc: bool = False
        self.in_standard_string: bool = False
        self.in_raw_string: bool = False
        self.raw_string_hashes: int = 0

    def process_line(self, line: str) -> LineLexResult:
        """Evaluate a single line of Rust code and update lexer state.

        Args:
            line: The raw line string from source code.

        Returns:
            LineLexResult classifying code and comment presence on this line.
        """
        has_code: bool = False
        has_doc_comment: bool = False
        has_impl_comment: bool = False

        if self.block_comment_depth > 0:
            if self.is_block_doc:
                has_doc_comment = True
            else:
                has_impl_comment = True

        index: int = 0
        line_len: int = len(line)

        while index < line_len:
            # ---------------------------------------------------------------
            # 1. State: Inside a multi-line raw string literal r###" ... "###
            # ---------------------------------------------------------------
            if self.in_raw_string:
                has_code = True
                close_marker: str = '"' + ("#" * self.raw_string_hashes)
                close_pos: int = line.find(close_marker, index)
                if close_pos != -1:
                    index = close_pos + len(close_marker)
                    self.in_raw_string = False
                    self.raw_string_hashes = 0
                    continue
                break

            # ---------------------------------------------------------------
            # 2. State: Inside a standard double-quoted string literal " ... "
            # ---------------------------------------------------------------
            if self.in_standard_string:
                has_code = True
                while index < line_len:
                    char = line[index]
                    if char == "\\":
                        index += 2  # skip escaped character
                        continue
                    if char == '"':
                        self.in_standard_string = False
                        index += 1
                        break
                    index += 1
                continue

            # ---------------------------------------------------------------
            # 3. State: Inside a block comment /* ... */ (supports nesting)
            # ---------------------------------------------------------------
            if self.block_comment_depth > 0:
                if self.is_block_doc:
                    has_doc_comment = True
                else:
                    has_impl_comment = True

                # Check for nested block comment start /*
                if (
                    index + 1 < line_len
                    and line[index] == "/"
                    and line[index + 1] == "*"
                ):
                    self.block_comment_depth += 1
                    index += 2
                    continue

                # Check for block comment end */
                if (
                    index + 1 < line_len
                    and line[index] == "*"
                    and line[index + 1] == "/"
                ):
                    self.block_comment_depth -= 1
                    index += 2
                    if self.block_comment_depth == 0:
                        self.is_block_doc = False
                    continue

                index += 1
                continue

            # ---------------------------------------------------------------
            # 4. State: Outside strings and comments (normal code context)
            # ---------------------------------------------------------------
            current_char: str = line[index]

            # Whitespace
            if current_char.isspace():
                index += 1
                continue

            # Line comment: //
            if (
                index + 1 < line_len
                and current_char == "/"
                and line[index + 1] == "/"
            ):
                # Distinguish doc comment: /// or //!
                # Note: //// is a regular comment in Rust conventions
                is_doc: bool = False
                if index + 2 < line_len:
                    third_char: str = line[index + 2]
                    if third_char in ("/", "!"):
                        if index + 3 >= line_len or line[index + 3] != "/":
                            is_doc = True

                if is_doc:
                    has_doc_comment = True
                else:
                    has_impl_comment = True
                # Line comment consumes the rest of the line
                break

            # Block comment start: /*
            if (
                index + 1 < line_len
                and current_char == "/"
                and line[index + 1] == "*"
            ):
                self.block_comment_depth = 1
                # Check for block doc comment: /** ... */ or /*! ... */
                # /*** is not a doc comment
                if index + 2 < line_len:
                    third = line[index + 2]
                    if third == "!":
                        self.is_block_doc = True
                    elif third == "*":
                        if index + 3 < line_len and line[index + 3] == "*":
                            self.is_block_doc = False
                        else:
                            self.is_block_doc = True
                    else:
                        self.is_block_doc = False
                else:
                    self.is_block_doc = False

                if self.is_block_doc:
                    has_doc_comment = True
                else:
                    has_impl_comment = True

                index += 2
                continue

            # Raw string literal start: r"...", r#"..."#, br"...", etc.
            if current_char == "r" or (
                current_char == "b"
                and index + 1 < line_len
                and line[index + 1] == "r"
            ):
                r_offset: int = 1 if current_char == "r" else 2
                probe: int = index + r_offset
                hash_count: int = 0
                while probe < line_len and line[probe] == "#":
                    hash_count += 1
                    probe += 1

                if probe < line_len and line[probe] == '"':
                    has_code = True
                    self.in_raw_string = True
                    self.raw_string_hashes = hash_count
                    index = probe + 1
                    # Scan for potential close on same line
                    close_marker = '"' + ("#" * hash_count)
                    close_pos = line.find(close_marker, index)
                    if close_pos != -1:
                        index = close_pos + len(close_marker)
                        self.in_raw_string = False
                        self.raw_string_hashes = 0
                    else:
                        break
                    continue

            # Standard double-quoted string literal start: "..." or b"..."
            if current_char == '"' or (
                current_char == "b"
                and index + 1 < line_len
                and line[index + 1] == '"'
            ):
                has_code = True
                self.in_standard_string = True
                index = (index + 2) if current_char == "b" else (index + 1)
                while index < line_len:
                    ch = line[index]
                    if ch == "\\":
                        index += 2
                        continue
                    if ch == '"':
                        self.in_standard_string = False
                        index += 1
                        break
                    index += 1
                continue

            # Character literal start: 'c' or '\n' or '\''
            if current_char == "'":
                has_code = True
                index += 1
                if index < line_len and line[index] == "\\":
                    index += 2  # skip escape
                elif index < line_len:
                    index += 1  # skip char
                if index < line_len and line[index] == "'":
                    index += 1  # closing quote
                continue

            # Regular executable code token
            has_code = True
            index += 1

        return LineLexResult(
            has_code=has_code,
            has_doc_comment=has_doc_comment,
            has_impl_comment=has_impl_comment,
        )


def analyze_rust_source_lines(
    lines: list[str],
) -> tuple[int, int, int]:
    """Analyze source lines for SLoC, doc comment lines, and impl comments.

    Args:
        lines: List of raw lines from a Rust source file.

    Returns:
        Tuple of (sloc_count, doc_comment_count, impl_comment_count).
    """
    lexer = RustCommentLexer()
    sloc: int = 0
    doc_comments: int = 0
    impl_comments: int = 0

    for line in lines:
        result = lexer.process_line(line)
        if result.has_code:
            sloc += 1
        if result.has_doc_comment:
            doc_comments += 1
        if result.has_impl_comment:
            impl_comments += 1

    return sloc, doc_comments, impl_comments
