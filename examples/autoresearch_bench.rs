//! Autoresearch benchmark harness for `src/index/**` and `src/query/**`.
//!
//! Deterministic, criterion-free workload driving the public
//! [`IndexerService`]/[`QueryService`] lifecycle end to end: build, persist,
//! load, no-op refresh, single-note-edit refresh, five representative query
//! shapes (full-index filter+sort+limit, tag-narrowed page query, task
//! query, and two cold `sync_and_run` store-scoped shapes - a one-row match
//! and a broad ~20%-of-vault match, the latter exercising
//! `IndexStore::read_notes_batch`/`read_files_batch`/`read_links_for_targets`
//! with a realistic multi-row candidate set), plus an ambiguous-wikilink
//! cluster (many same-stem files linked by basename-only wikilinks) that
//! exercises `LinkResolver::nearest_by_stem`'s O(candidates) folder-distance
//! tie-break path during build and refresh - the base fixture's unique note
//! stems never triggered it.
//!
//! Not a criterion suite: no statistical sampling, so one run stays in the
//! low hundreds of milliseconds and is safe to run every autoresearch
//! iteration. `benches/*.rs` remain the source of truth for regression
//! guarding; this binary exists purely to give the autoresearch loop one fast,
//! reproducible number per architectural change.
//!
//! Run via `bash autoresearch.sh`, which wraps
//! `cargo run --release --example autoresearch_bench --features test-utils`.

use std::{fmt::Write as _, fs, sync::Arc, time::Instant};

use tempfile::tempdir;
use traces_pkm::{IndexerService, QueryBuilder, QueryService, SourceSelector};

/// Notes in the synthetic vault. Large enough to make O(n) vs. O(n²)
/// regressions visible, small enough that a full run stays well under one
/// second even at `opt-level = "z"`.
const NOTE_COUNT: usize = 3_000;

/// Same-stem files (`ambiguous/dir-{i}/index.md`) sharing the basename
/// `index`, giving `LinkResolver::nearest_by_stem` a real candidate set to
/// disambiguate by folder distance.
const AMBIGUOUS_STEM_COUNT: usize = 150;

/// Notes each carrying one basename-only `[[index]]` wikilink, forcing
/// [`AMBIGUOUS_STEM_COUNT`]-candidate ambiguity resolution per link.
const LINKER_COUNT: usize = 50;

/// Returns deterministic Markdown content for note `i` of `note_count`.
///
/// Every note carries frontmatter (`rating`, `class`), a common tag, a
/// bucketed nested tag (`#area/topic-{i % 5}`), two wikilinks into
/// neighboring notes, and three checkbox tasks. Note `0` additionally carries
/// a unique tag so tag-narrowed source resolution has a stable one-note
/// target unaffected by the later single-note edit.
fn note_source(i: usize, note_count: usize) -> String {
    let class = match i % 3 {
        0 => "Book",
        1 => "Article",
        _ => "Note",
    };
    let bucket = i % 5;
    let link_a = (i + 1) % note_count;
    let link_b = (i + 7) % note_count;
    let mut out = String::with_capacity(512);
    let _ = write!(
        out,
        "---\nrating: {rating}\nclass: {class}\n---\n\n#note \
         #area/topic-{bucket}{rare}\n\nLinks to [[note-{link_a}]] and \
         [[note-{link_b}]].\n\n- [ ] task one for note {i}\n- [x] task two \
         for note {i}\n- [ ] task three for note {i}\n",
        rating = i % 10,
        rare = if i == 0 {
            " #rare_0"
        } else {
            ""
        },
    );
    out
}

/// Writes [`AMBIGUOUS_STEM_COUNT`] same-stem `index.md` files across
/// distinct folders, plus [`LINKER_COUNT`] notes each carrying one
/// basename-only `[[index]]` wikilink, under `root`.
///
/// A basename-only wikilink with no folder qualifier and no unique stem
/// match forces the index's internal link resolver into its
/// nearest-by-stem tier, scanning every same-stem candidate and computing
/// folder distance to break the tie - the ambiguity-resolution path the base
/// fixture's unique note stems (`note-{i}.md`) never exercises.
fn write_ambiguous_cluster(root: &std::path::Path) -> std::io::Result<()> {
    for i in 0..AMBIGUOUS_STEM_COUNT {
        let dir = root.join(format!("ambiguous/dir-{i}"));
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("index.md"), format!("# Index {i}\n"))?;
    }
    for i in 0..LINKER_COUNT {
        fs::write(
            root.join(format!("ambiguous/linker-{i}.md")),
            "Links ambiguously to [[index]].\n",
        )?;
    }
    Ok(())
}

/// Times `op`, returning its result alongside elapsed microseconds.
fn timed<T, E>(op: impl FnOnce() -> Result<T, E>) -> Result<(T, u128), E> {
    let start = Instant::now();
    let value = op()?;
    Ok((value, start.elapsed().as_micros()))
}

/// Metric names, in the fixed order [`run_once`] returns their timings.
const METRIC_NAMES: [&str; 10] = [
    "build_us",
    "persist_us",
    "load_us",
    "refresh_noop_us",
    "refresh_edit_us",
    "query_all_us",
    "query_tag_us",
    "task_query_us",
    "sync_and_run_us",
    "sync_and_run_broad_us",
];

/// Number of pipeline repetitions per run. [`METRIC_NAMES`] are reported as
/// the per-phase minimum across repetitions, the standard noise-reduction
/// technique for wall-clock microbenchmarks (OS scheduling and page-cache
/// warmup only ever add latency, never subtract it, so the minimum is the
/// closest approximation of steady-state cost a single-process harness can
/// observe).
const REPEATS: usize = 5;

/// Runs one full build -> persist -> load -> refresh -> query pipeline
/// against `indexer`'s already-populated project root, returning each
/// phase's elapsed microseconds in [`METRIC_NAMES`] order.
fn run_once(
    indexer: &IndexerService,
    root: &std::path::Path,
) -> Result<[u128; 10], Box<dyn std::error::Error>> {
    let (index, build_us) = timed(|| indexer.build())?;
    let (_, persist_us) = timed(|| indexer.persist(&index))?;
    drop(index);
    let (loaded, load_us) = timed(|| indexer.load())?;
    let (_, refresh_noop_us) = timed(|| indexer.refresh())?;

    // Single-note edit: bump a mid-vault note's rating, exercising the
    // incremental upsert + inlink-patch path without disturbing note 0's
    // unique tag (used below by the sync_and_run cold-path query). Rewriting
    // the same content every repetition keeps this idempotent.
    let edit_target = NOTE_COUNT / 2;
    fs::write(
        root.join(format!("note-{edit_target}.md")),
        format!(
            "---\nrating: 9\nclass: Book\n---\n\n#note #area/topic-0\n\nLinks \
             to [[note-1]] and [[note-7]].\n\n- [ ] task one for note \
             {edit_target}\n- [x] task two for note {edit_target}\n- [ ] task \
             three for note {edit_target}\n"
        ),
    )?;
    let (_, refresh_edit_us) = timed(|| indexer.refresh())?;

    let index = Arc::new(loaded);
    let service = QueryService::new("class");

    let (page_set, query_all_us) =
        timed(|| -> Result<_, Box<dyn std::error::Error>> {
            let builder = QueryBuilder::pages(SourceSelector::All)
                .filter("rating >= 5")?
                .sort("file.name", false)?
                .limit(500)?;
            Ok(service.run(&index, builder))
        })?;

    let (tag_set, query_tag_us) =
        timed(|| -> Result<_, Box<dyn std::error::Error>> {
            let source = SourceSelector::parse("#area/topic-2")?;
            let builder =
                QueryBuilder::pages(source).sort("file.name", false)?;
            Ok(service.run(&index, builder))
        })?;

    let (task_set, task_query_us) =
        timed(|| -> Result<_, Box<dyn std::error::Error>> {
            let source = SourceSelector::parse("#area/topic-3")?;
            let builder = QueryBuilder::tasks(source)
                .filter("task.completed == false")?;
            Ok(service.run(&index, builder))
        })?;

    let (sync_set, sync_and_run_us) =
        timed(|| -> Result<_, Box<dyn std::error::Error>> {
            let source = SourceSelector::parse("#rare_0")?;
            let builder = QueryBuilder::pages(source);
            Ok(service.sync_and_run(indexer, builder)?)
        })?;

    // Broad store-scoped candidate set (~20% of the vault, i % 5 == 2):
    // exercises IndexStore's per-candidate batch reads
    // (read_notes_batch/read_files_batch/read_links_for_targets) with a
    // realistic multi-row match, unlike the one-row `#rare_0` query above.
    let (sync_broad_set, sync_and_run_broad_us) =
        timed(|| -> Result<_, Box<dyn std::error::Error>> {
            let source = SourceSelector::parse("#area/topic-2")?;
            let builder = QueryBuilder::pages(source);
            Ok(service.sync_and_run(indexer, builder)?)
        })?;

    // Sanity checks: a broken candidate resolver or transform pipeline should
    // fail the harness outright rather than silently report a bogus timing.
    assert!(page_set.len() <= 500, "limit transform did not bound rows");
    assert!(!tag_set.is_empty(), "tag-narrowed source matched nothing");
    assert!(!task_set.is_empty(), "task source matched nothing");
    assert_eq!(sync_set.len(), 1, "unique tag must resolve to exactly one row");
    assert!(
        sync_broad_set.len() > 100,
        "broad tag source should match a meaningful vault fraction"
    );

    Ok([
        build_us,
        persist_us,
        load_us,
        refresh_noop_us,
        refresh_edit_us,
        query_all_us,
        query_tag_us,
        task_query_us,
        sync_and_run_us,
        sync_and_run_broad_us,
    ])
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempdir()?;
    for i in 0..NOTE_COUNT {
        fs::write(
            temp.path().join(format!("note-{i}.md")),
            note_source(i, NOTE_COUNT),
        )?;
    }
    write_ambiguous_cluster(temp.path())?;

    let indexer = IndexerService::new(temp.path());

    let mut mins = [u128::MAX; 10];
    for _ in 0..REPEATS {
        let sample = run_once(&indexer, temp.path())?;
        for (slot, value) in mins.iter_mut().zip(sample) {
            *slot = (*slot).min(value);
        }
    }

    let total_us: u128 = mins.iter().sum();
    println!("METRIC total_pipeline_us={total_us}");
    for (name, value) in METRIC_NAMES.iter().zip(mins) {
        println!("METRIC {name}={value}");
    }

    Ok(())
}
