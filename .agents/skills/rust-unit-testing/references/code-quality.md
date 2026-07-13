# Code Quality

> Unit suite code is code. Review it for clarity, not only coverage.

Flag:
- Hidden assertions in helpers.
- Overbroad fixtures or builders that hide the arranged case.
- Excessive setup relative to the behavior.
- Repeated assertion blocks that should become table-driven cases.
- Assertions against large objects when one field or a snapshot is clearer.
- Production-like branching, loops, or abstractions inside suite code.

For each finding report file/line, why clarity suffers, and the smallest refactor.
