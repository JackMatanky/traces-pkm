# Research: salsa/incremental-computation frameworks — timing and cost of adopting now vs. later, and root cause of large-vault performance problems

Resolves ticket [38-research-salsa-incremental-computation](../issues/38-research-salsa-incremental-computation.md).

## Overview

This research examines whether Traces should adopt a `salsa`-style incremental query engine immediately or defer the decision to ticket 33 (performance targets). It investigates how `salsa` integrates into a language server architecture (specifically `rust-analyzer`) and analyzes the actual root causes of performance failures in Obsidian and Markdown Oxide for large vaults.

## Findings

### 1. Salsa Mechanics & Architectural Compatibility
**How salsa works**: `salsa` (v0.28.2) is a framework for on-demand, incrementalized computation. It strictly separates inputs from derived queries. 
- **Memoization & Dependency Tracking**: State is tracked using the `Database` trait. Inputs and intermediate structures are decorated with macros like `#[salsa::tracked]`.
- **Revision-based Invalidation**: When an input changes, `salsa` increments a global revision. On the next query, it checks if dependencies have changed. If an intermediate query's output is identical despite its inputs changing (e.g., a whitespace edit doesn't change the AST), downstream queries are short-circuited and skipped.

**Architectural cost of timing**: Adopting an `Arc<FileIndex>` behind a swap-on-refresh point now **does not foreclose or complicate** adding `salsa` later. It is a highly compatible foundation. 
- As evidenced by `rust-analyzer`'s source code (`crates/ide/src/host.rs`), the `AnalysisHost` / `Analysis` split is the exact outer shell used to house a `salsa` database. `AnalysisHost` owns the mutable `RootDatabase` (which implements `salsa::Database`), and `Analysis` is just a cheaply cloned snapshot (`Analysis { db: self.db.clone() }`).
- By building the `Arc<FileIndex>` Host/Snapshot boundary now, Traces establishes the rigorous concurrency and cancellation model required for a responsive LSP. If performance targets later demand `salsa`, the internal `Arc<FileIndex>` can be cleanly swapped for a `salsa::Database` without rewriting the LSP request routing or thread-pool logic.

### 2. Root Cause of Large-Vault Pain
The user's core motivation—that Obsidian and Markdown Oxide fail on large vaults (10k+ notes)—is real, but the **root cause is not a lack of sub-file incremental AST recomputation**.

- **Markdown Oxide**: Its large-vault friction stems from its architectural extremes. It uses an eager `WalkDir` on startup, stores the entire graph in-memory, has no on-disk persistence, and processes everything via parallel regex. Furthermore, external filesystem changes trigger a full vault drop and rebuild (`did_change_watched_files` bug). The failure mode is **memory exhaustion and massive initial O(N) CPU load**, not inefficient derived-data updates.
- **Obsidian**: Obsidian's core actually handles tens of thousands of files well. Web research confirms that slowness/crashing is almost exclusively caused by **plugin overload (specifically Dataview)** running unindexed O(N) linear scans across the entire vault on every render/edit, sync service churn, or massive attachments consuming memory.

The shared root cause is **unbounded memory growth, lack of persistence, and eager O(N) whole-vault scans**. 

### 3. Recommendation for Ticket 10
**Recommendation: Implement the `Arc<FileIndex>` snapshot model now; explicitly defer `salsa` to Ticket 33.**

1. **Root-cause alignment**: The user's large-vault pain is solved by Traces' planned persistent on-disk index (`redb`) and targeted graph lookups, which prevent the O(N) memory and CPU exhaustion seen in Markdown Oxide and Dataview. Fine-grained incremental re-evaluation (`salsa`) solves a different problem (optimizing post-edit derived data) that isn't the primary bottleneck yet.
2. **YAGNI & Complexity**: `salsa` requires rigorous data modeling discipline (using raw IDs instead of object references, defining granular tracked structs). Taking this on before setting concrete performance targets violates YAGNI.
3. **Extensible Foundation**: The Host/Snapshot pattern built around `Arc<FileIndex>` perfectly isolates the mutable state. It proves out the threading and cancellation architecture, serving as a ready-made drop-in shell if `salsa` proves necessary later.
