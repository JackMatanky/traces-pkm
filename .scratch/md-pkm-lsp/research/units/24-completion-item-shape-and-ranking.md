# Ticket 24 part-work: `CompletionItem` shape, ranking, filtering & caps (unit 3 → ticket 24)

Unit: **`completion-item-shape-and-ranking`** (kind: `mixed`) — per
[24-part-decomposition](24-part-decomposition.md) §3 (lines 45-51). Input artifact for ticket
[24](../../issues/24-completion-architecture.md); §11 is the ready-to-paste block.

**What this unit decides** (decomposition `:51`): the per-context item-shape table (fields set/unset),
one cross-context ranking policy reconciling 19/23/15/20's four locked ordering schemes, the
`textEdit` vs `additional_text_edits` decision table, and the cap/`is_incomplete` policy — plus (d)
the `CompletionItemKind` taxonomy and `commitCharacters` use.

**What it does not decide**: the dispatcher (unit 1 — its `ContextKind` enum is this unit's input
contract, `dispatch:260-261`), the trigger set (unit 2), the resolve split (unit 4), snippet
shapes (unit 5), capability gating (unit 8 — this file only *declares* what must be gated).

---

## 0. Sources, corpus caveat, shorthand

`docs/refs/lsp_spec.md` cannot serve as citation for completion — its body is literally
`{% include_relative language/completion.md %}` (`docs/refs/lsp_spec.md:659`). Full reasoning and
the changelog table are in [capability-negotiation](24-completion-capability-negotiation.md) §0.
Spec facts below are therefore cited to the live 3.18 document:

- **[S1]** completion feature spec —
  <https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.18/language/completion.md>
  (fetched 2026-10-06; 1083 lines). Line numbers in this file refer to that fetch.

Ecosystem claims carry `file:line` of the project's **own source** (GitHub `master`/`main` raw fetch
or local digest). Digest shorthand used below:

| Shorthand | File |
| :--- | :--- |
| `mdoxide-src:` | `docs/digests/lsp_feel-ix-343-markdown-oxide-src-digest.txt` |
| `mdoxide:` | `docs/digests/lsp_feel-ix-343-markdown-oxide-digest.txt` |
| `marksman:` | `docs/digests/lsp_artempyanykh-marksman-digest.txt` |
| `rumdl-src:` | `docs/digests/lsp_rvben-rumdl-src-digest.txt` |
| `msmd:` | `docs/digests/lsp_microsoft-vscode-markdown-languageservice-digest.txt` |
| `zk-src:` / `zk:` | `docs/digests/zk-src-digest.txt` / `zk-digest.txt` |
| `crates:` | [24-part-crates](24-part-crates.md) |
| `capability-negotiation:` / `latency:` / `novel:` / `dispatch:` | sibling research files in this dir |

Sibling grounding: [capability-negotiation](24-completion-capability-negotiation.md) (§1 capability
inventory, §1.2 kind cliff, §5 filtering/sorting ownership, §8 `ls-types` gap list),
[latency](24-completion-latency-architecture.md) (§3 budgets, §4 stage budget),
[novel](24-novel-embedded-context-precedent.md) (§4 layering, §5 single-list discipline),
[dispatch](24-completion-dispatch-architecture.md) (§5 `ContextKind` inventory C1–C17).

---

## 1. What the spec demands (**spec source**, not opinion)

### 1.1 The two insertion modes, and what text-edit mode costs

[S1:8-15] states the model outright:

> "usually the client is responsible for filtering and sorting… However, servers can enforce
> different behavior by setting a `filterText` / `sortText`." [S1:8]
>
> "for speed, clients should be able to filter an already received completion list if the user
> continues typing. Servers can opt out of this using a `CompletionList` and mark it as
> `isIncomplete`." [S1:9]

> **insertText mode** — "the client should filter against what the user has already typed using the
> word boundary rules of the language" [S1:13].
>
> **textEdit mode** — "the server tells the client that it actually knows what it is doing… no word
> guessing takes place and no automatic filtering (like with an `insertText`) should happen… If the
> text edit is a replace edit then **the range denotes the word used for filtering**. If the replace
> changes the text it most likely makes sense to specify a filter text to be used." [S1:15]

Ticket 21 already mandates text-edit mode for *all* completions (`issues/21:48`). Everything in §4
and §6 follows from that one commitment. The single most load-bearing derived rule:

> **R-SHARED-FILTER-WORD.** For every item, `filterText` must match the text in
> `textEdit.range.start .. cursor`. The client derives that word *only* from the replace range, so
> `range.start` and `filterText` are one contract, not two.

Evidence that servers get this wrong when they treat them independently — rumdl, verbatim:

> `// Filter against the full replacement text (e.g. `/img/icons/`)`
> `// since the edit replaces the whole typed path, not just the`
> `// child name; otherwise clients filter out valid items.`
> (`rumdl-src:29927-29929`)

And the corollary [S1:198-216 via VS Code's implementation, §2.1]: **if `filterText` is present and
does not match the current word, the item is dropped entirely.** A wrong `filterText` is not a
cosmetic defect; it deletes the item from the widget.

Also from [S1]:

- `insertText` is "subject to interpretation by the client… VSCode will only insert `sole`…
  **recommended to use `textEdit` instead**" [S1:836-843].
- `textEdit.range` "must be a single line range and it must contain the position at which completion
  has been requested" [S1:864-866]; `InsertReplaceEdit` needs `insertReplaceSupport` and insert
  range must be a prefix of the replace range [S1:874-886].
- `additionalTextEdits` "must not overlap (including the same insert position) with the main edit
  nor with themselves" and are "for text unrelated to the current cursor position"
  [S1:917-923].
- **`insertTextFormat` applies to `insertText` *and* to `textEdit.newText`, but "doesn't apply to
  `additionalTextEdits`"** [S1:845-854]. ⇒ snippets live in the main edit only.
- `insertText`/`textEdit` are ignored rules: "When an edit is provided, the value of `insertText` is
  ignored" [S1:869-871].

### 1.2 Field-by-field contract (spec rule → what it binds)

| Field | Spec rule [S1] | Line(s) | Binds |
| :--- | :--- | :--- | :--- |
| `label` | "If label details are provided, the label itself should be an **unqualified name**" | :762-767 | §5 label policy |
| `labelDetails.detail` | rendered "directly after label, without any spacing… function signatures or type annotations" | :706-709 | counts (`47 references`) |
| `labelDetails.description` | "rendered less prominently **after** `detail`… fully qualified names or file paths" | :714-716 | provenance badges |
| `kind` | "an icon is chosen by the editor" | :778-781 | §7 |
| `detail` | "additional information about this item, like type or symbol information" — **no capability gate** | :786-788 | fallback for `labelDetails` |
| `documentation` | doc-comment / markup | :794 | unit 4 (resolve) |
| `preselect` | "only one completion item can be selected and the tool / client decides… the rule is that the **first** item of those that match best is selected"; needs `preselectSupport` | :804-812, :96-98 | §4.4 |
| `sortText` | "should be used when comparing this item with other items. **When omitted, the label is used**" | :819-823 | §4.1 |
| `filterText` | "should be used when filtering a set of completion items. **When omitted, the label is used**" | :826-830 | §4.3 |
| `insertText` | "When omitted, the label is used" | :832-843 | unused (textEdit mode) |
| `insertTextFormat` | applies to `insertText` **and** `textEdit.newText`; default `PlainText`; **not** to `additionalTextEdits` | :845-854 | unit 5 |
| `insertTextMode` | client default from `completion.insertTextMode` capability | :856-860 | leave unset |
| `textEdit` | single-line, contains request position; `InsertReplaceEdit` capability-gated | :862-890 | §6 |
| `textEditText` | 3.17; only honoured with `completionList.itemDefaults` | :893-903 | **unavailable** (§1.5) |
| `additionalTextEdits` | non-overlapping, unrelated-to-cursor | :917-923 | §6 |
| `commitCharacters` | "length=1… superfluous characters will be ignored" | :925-929 | §8 |
| `command` | executed **after** inserting; further doc changes belong in `additionalTextEdits` | :931-935 | §8 |
| `data` | preserved between completion and resolve | :937-939 | unit 4 |
| `tags` | 3.15 `Deprecated` | :784, :175 | none needed |
| **resolve immutability** | "`sortText`, `filterText`, `insertText` and `textEdit` **must be provided in the `textDocument/completion` response and must not be changed during resolve**" | :4 | §9 seam |

### 1.3 `CompletionList`

- `isIncomplete: boolean` [S1:410]. A bare `CompletionItem[]` response is interpreted as
  `{ isIncomplete: false, items }`; `null` as an empty list [S1:381].
- `itemDefaults` (3.17, five fields) + `applyKind` (3.18) [S1:413-450, :723-746] — **unavailable to
  Traces**: `ls-types 0.0.6`'s `CompletionList { is_incomplete, items }` has no `item_defaults`
  (`crates:486`, `capability-negotiation:486`). Do not design around them.
- Because `textEditText` exists only as an `itemDefaults` partner [S1:893-903], and `itemDefaults`
  cannot be sent, **every item must carry its own `textEdit`**. Ticket 21's "explicit range for all"
  (`issues/21:48`) is therefore not just a style choice — it is forced by the crate gap.

### 1.4 The kind cliff

Absent `completionItemKind.valueSet` ⇒ "client supports only 1–18" (`capability-negotiation:94-113`).
Traces-contested values `FOLDER`=19 and `ENUM_MEMBER`=20 sit above it; Helix does not advertise
`valueSet` at all (`capability-negotiation:420,450-454`). §7 adopts the stricter of the two available
answers.

---

## 2. What clients and servers actually do (**ecosystem practice**, primary source)

### 2.1 VS Code — the reference algorithm, exactly

VS Code is where the "server sets `sortText`" folk model comes from, and its real algorithm is
**not** "sort by `sortText`". Two phases, both line-cited:

**Phase 1 — initial order** (`src/vs/editor/contrib/suggest/browser/suggest.ts`,
<https://github.com/microsoft/vscode>):

```
250-253  // fill in default sortText when missing
         suggestion.sortText = typeof suggestion.label === 'string' ? suggestion.label : suggestion.label.label;
331      result.sort(getSuggestionComparator(options.snippetSortOrder));
339-356  function defaultComparator(a, b) {
           // check with 'sortText'
           if (a.sortTextLow && b.sortTextLow) { lexicographic on lowercased sortText }
           // check with 'label'
           // check with 'type'  (kind)
         }
```

- `sortTextLow`/`filterTextLow` are **lowercased** (`suggest.ts:96-97`) ⇒ VS Code's `sortText`
  comparison is case-insensitive.
- All four `snippetSortOrder` comparators fall through to `defaultComparator` (`suggest.ts:366,377,384`)
  ⇒ the rule holds under every `snippetSuggestions` setting.
- **Hazard**: the `sortText` branch runs only `if (a.sortTextLow && b.sortTextLow)`. An item *without*
  `sortText` silently demotes the whole comparison to label order. ⇒ **every item must carry
  `sortText`** — this is rust-analyzer's stated reason for always setting it (§2.2).

**Phase 2 — order while the user types**
(`src/vs/editor/contrib/suggest/browser/completionModel.ts`):

```
140     scoreFn = (!filterGraceful || source.length > 2000) ? fuzzyScore : fuzzyScoreGracefulAggressive
172-178 if (wordLen === 0) {
          // when there is nothing to score against, don't even try to do. Use a const rank and
          // rely on the fallback-sort using the initial sort order. use a score of `-100` because
          // that is out of the bound of values `fuzzyScore` will return
          item.score = FuzzyScore.Default;
        }
198-216 } else if (typeof item.completion.filterText === 'string') {
          const match = scoreFn(word, ..., item.completion.filterText, ...);
          if (!match) continue;                       // ← item DROPPED
          if (filterText === label) item.score = match;
          else { item.score = anyScore(word, ..., item.textLabel, ...); item.score[0] = match[0]; }
        } else { score against item.textLabel }
227-228 item.idx = i;      item.distance = wordDistance.distance(...)
235     this._filteredItems = target.sort(this._snippetCompareFn);
244-259 _compareCompletionItems: score[0] DESC → distance ASC → idx ASC
```

`item.idx` is the index in the phase-1 array, i.e. **`sortText` order is the final tiebreak**.

**Three consequences that decide this unit's whole ranking section:**

1. **`sortText` never outranks a better fuzzy match.** It decides only among items the client scored
   identically (and it decides *everything* when the filter word is empty — the `wordLen === 0`
   branch says so in as many words: *"rely on the fallback-sort using the initial sort order"*).
2. **When `filterText` is present, the label does not participate in matching or in the score** —
   `score[0]` comes from the `filterText` match (`:214`). So a badge/qualification in `label` cannot
   break filtering *provided `filterText` is set*. When `filterText` is absent, `label` is matched
   (`:219`) and a badge **does** break filtering. ⇒ **always set `filterText`.**
3. **The filter word is derived from `textEdit.range.start`** — `overwriteBefore =
   item.position.column - item.editStart.column` (`completionModel.ts:152-155`). So the first
   character of `filterText` must be consistent with the first character of the replace range. This
   is R-SHARED-FILTER-WORD (§1.1), observed in the reference client.

**Locality**: `distance` is only live when `editor.suggest.localityBonus` is on, and its **default is
`false`** (`src/vs/editor/common/config/editorOptions.ts:5170`), which yields `WordDistance.None` and
a constant `0` (`wordDistance.ts:45-51`). ⇒ **Default VS Code ordering is exactly: fuzzy score DESC,
then `sortText` ASC (case-folded), then label, then kind.** Two keys, no more.

**Provider merging** (relevant because rumdl shares the selector): providers are "ordered in groups of
equal score and once a group produces a result the process stops"
(`suggest.ts:291-318`, `if (didAddResult || …) break;`). VS Code merges into one widget
(`novel:175-180`) — no double popup — but a *server's* items never get interleaved with another
provider's item-by-item; grouping is per provider.

### 2.2 How servers encode rank

| Server | `sortText` | `preselect` | `isIncomplete` | Cap / eviction | Source |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **gopls** | `fmt.Sprintf("%05d", i)` — *"a hack so that the client sorts completion results in the order according to their score. This can be removed upon the resolution of https://github.com/Microsoft/language-server-protocol/issues/348."* | `Preselect: i == 0` | `incompleteResults = options.DeepCompletion \|\| options.Matcher == settings.Fuzzy`; empty list also `true` | none (metrics `complLong` if `>10`) | `gopls/internal/server/completion.go:69-71,96,196-205` |
| **TS server** | 9 fixed-width 2-char tiers `"10"`…`"18"`; deprecation demotes by prefixing `"z"`; `SortBelow(x)` = `x + "1"`; object-literal tiebreak `tier + "\x00" + displayName + "\x00"` | `preselect := isRecommendedCompletionMatch(symbol, data.recommendedCompletion, typeChecker)` — only the context-recommended symbol | `IsIncomplete: data.hasUnresolvedAutoImports` | n/a | `microsoft/TypeScript` `tsc/internal/ls/completions.go:269-292,1948,2439` |
| **rust-analyzer** | `format!("{sort_score:08x}")` with `sort_score = relevance.score() ^ 0xFF_FF_FF_FF` and the comment *"Zero pad the string to ensure values can be properly sorted by the client."* | `res.preselect = Some(true)` only when `relevance.is_relevant() && relevance.score() == max_relevance` | **always `true`** | `res.sort_by(sort_text); res.truncate(limit)` when a limit is configured | `crates/rust-analyzer/src/lsp/to_proto.rs:477-495,283-285`; `crates/rust-analyzer/src/handlers/request.rs:1194-1200` |
| **pyrefly** | `format!("{base}.{rank:04}.{label}")`, `base ∈ {0 local, 1 reexport, 2 `_`name, 3 dunder, 4a autoimport-public, 4b autoimport-private, 9 deprecated}`, `+ "z"` if incompatible; MRU-inactive ⇒ `base` only; MRU-absent ⇒ `{base}.9999.{label}` | `preselect = rank == 0` (MRU rank) | `true` while below `MIN_CHARACTERS_TYPED_AUTOIMPORT` (3) **or** while a local match might be masking autoimports | none | `facebook/pyrefly` `pyrefly/lib/lsp/wasm/completion.rs:96-140` |
| **MS markdown LS** | `sortTexts = { localHeader: '1', workspaceHeader: '2' }` | — | — | — | `msmd:5517-5520` |

**Readings worth carrying forward:**

- **Every serious server sets `sortText` on every item**, and the two with a *documented* reason
  (gopls, rust-analyzer) both say it is because clients sort unreliably — gopls cites LSP issue
  **#348** (`SortText` to express ranking) which is *still unresolved*; rust-analyzer's comment is
  about lexicographic order, not about client quality.
- **Zero-padding is an explicit, commented engineering requirement** in rust-analyzer and rumdl
  (§2.3). pyrefly and TS server use fixed-width/`{:04}` for the same reason. Markdown Oxide does
  **not** (§2.4) — the counter-example.
- **`preselect` is never "the first item by accident"** except in gopls: TS uses type-checker
  recommendation, rust-analyzer uses top relevance, pyrefly uses MRU rank 0.
- pyrefly's own integration test asserts the response is `is_sorted_by_key(|x| (&x.sort_text, &x.label))`
  (`pyrefly/lib/test/lsp/lsp_interaction/completion.rs`) — i.e. **a server may be relied upon to emit
  an already-sorted array**, and some test suites treat that as contract.

### 2.3 Markdown / PKM servers

**rumdl** — the closest structural analogue to Traces' text-edit mode, and the strongest single
precedent for this unit:

```
rumdl-src:29810-29812   const MAX_ITEMS: usize = 50;
                         let is_incomplete = matches.len() > MAX_ITEMS;
                         matches.truncate(MAX_ITEMS);
rumdl-src:29818-29822   kind: FILE,
                         // Encode distance in the sort key so the editor keeps nearer files
                         // on top even when its own ordering would otherwise be lexical.
                         sort_text: Some(format!("{distance:04}{rel_str}")),
rumdl-src:29796-29800   // Collect (distance, relative path) pairs so we can rank before truncating.
                         matches.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
rumdl-src:29926-29930   sort_text: Some(format!("{}{}", if is_dir { '0' } else { '1' }, label)),
                         // Filter against the full replacement text … otherwise clients filter out valid items.
                         filter_text: Some(new_text.clone()),
rumdl-src:29941-29946   // Re-open completion after a directory so the user keeps drilling.
                         command: is_dir.then(|| Command { command: "editor.action.triggerSuggest", … })
rumdl-src:29952-29954   // Always incomplete: each new path segment needs a fresh directory listing.
                         CompletionList { is_incomplete: true, items, … }
```

Four reusable decisions: **rank-then-truncate** (never truncate-then-sort), **zero-padded rank +
label**, **`filterText` = the whole replacement text**, **`isIncomplete` = "my next answer would be
different"**.

**Markdown Oxide**

- Kind map (`mdoxide-src:4491-4499`): `File → FILE`, `Heading|Block → REFERENCE`,
  `Unresolved → KEYWORD`, `Alias → ENUM`, `DailyNote → EVENT`; tags `→ KEYWORD`
  (`mdoxide-src:5371`); callouts `→ SNIPPET` (`mdoxide-src:3645`); footnotes `→ REFERENCE`
  (`mdoxide-src:3763`); unindexed blocks `→ REFERENCE` when indexed, else `TEXT`
  (`mdoxide-src:5565,5627`).
- `label_details.detail = "{n} reference(s)"` for tags (`mdoxide-src:5375-5379`), and
  `label_details.description = "Unresolved"` for unresolved links (`mdoxide-src:4503-4509`) — i.e.
  Markdown Oxide already uses **`detail` for counts and `description` for provenance**, exactly the
  split §5 adopts.
- `preselect: Some(match self { … daily.relative_name(completer) == Some(completer.entered_refname()) …
  link_completion.refname() == completer.entered_refname() })` (`mdoxide-src:4520-4526`) —
  **preselect only on an exact match of what the user already typed**, not on "first item".
- `is_incomplete: true` unconditionally (`mdoxide-src:5198`), items `.take(20)` (`mdoxide-src:5191`).
- `completion_filter_text(&self) = format!("{}{}", preceding_text, name)` (`mdoxide-src:3552-3554`) —
  the filterText is built from the *text preceding the token in the line*, i.e. Markdown Oxide
  independently derives the same `range.start`-consistent rule as rumdl.
- **Anti-pattern**: `fuzzy_match_completions` maps nucleo's `u32` score straight to
  `OrderedCompletion::new(item, score.to_string())` and that becomes `sort_text`
  (`mdoxide-src:4835,4820`) — **an unpadded decimal**, so `"10" < "9"` lexicographically. This is the
  concrete reason §4.1 makes zero-padding a hard rule.

**Marksman**

- `IsIncomplete = Array.length candidates >= maxCompletions` after `Seq.truncate maxCompletions`
  (`marksman:11777-11785`) — **cap-conditional `isIncomplete`**, with `complCandidates` default
  **50** (`marksman:5421`, config key `completion.candidates`, `:5532`).
- Dedup *within* a candidate class: headings `|> Set.ofSeq  // Remove duplicates in completion
  candidates` (`marksman:5113-5116`); tags `|> Seq.countBy id` (`marksman:5141-5150`) — count is
  computed, not carried on the item.
- **Class exclusivity, not union**: *"The priority is generally link > partialElement > tag"*
  (`marksman:5153-5165`). Matches `novel:185-189` (one region → one completer → one list).

**zk**

- Notes: `kind := protocol.CompletionItemKindReference` (`zk-src:4959`), `Label` = title with path
  fallback, `FilterText = label + " " + note.Path`, `TextEdit` for the link
  (`zk-src:4972-5010`), `CompletionItemResolve` fills documentation.
- Tags: `Label: tag.Name`, **`InsertText: s.buildInsertForTag(...)`** (not `textEdit`),
  **top-level `Detail: "3 notes"`** (not `labelDetails`), **no `kind` at all**
  (`zk-src:4864-4868`).
- No `sortText`, no `preselect`, no `insertTextFormat` anywhere; `additionalTextEdits` used for
  notebook-wide edits alongside a main `TextEdit` (`zk-digest:4184-4260`).

**Microsoft markdown-language-service**

- `sortTexts = { localHeader: '1', workspaceHeader: '2' }` (`msmd:5517-5520`) — single-char tiers.
- `kind: isDir ? Folder : File` (`msmd:5907`), `Reference`/`Value` for headers/other
  (`msmd:5764,5802,5817`).
- `command: isDir ? { command: 'editor.action.triggerSuggest', title: '' } : undefined`
  (`msmd:5915`) — same "keep drilling" trick as rumdl.

### 2.4 Cross-cutting matrix

| Practice | gopls | TS | RA | pyrefly | rumdl | mdoxide | Marksman | zk | MS md LS |
| :--- | :-: | :-: | :-: | :-: | :-: | :-: | :-: | :-: | :-: |
| sets `sortText` on **every** item | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ (`:4820` only on ordered lists) | ✗ | ✗ | partial |
| zero-padded rank | ✓ `%05d` | ✓ fixed-width | ✓ `%08x` | ✓ `{:04}` | ✓ `{:04}` | **✗** | — | — | ✓ (digit tiers) |
| sets `filterText` | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✗ | partial | ✓ |
| uses `textEdit` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | notes only, tags `insertText` | ✓ |
| `preselect` | first item | recommended symbol | top relevance | MRU #0 | ✗ | exact typed match | ✗ | ✗ | ✗ |
| `isIncomplete` conditional on cap/more-work | conditional | ✓ (`hasUnresolvedAutoImports`) | always ✓ | ✓ (threshold) | ✓ (dirs always; paths if capped) | always ✓ | ✓ (capped) | ✗ | ✗ |
| server-side cap | ✗ | ✗ | ✓ (config `limit`) | ✗ | ✓ 50 | ✓ 20 | ✓ 50 | ✗ | ✗ |
| `commitCharacters` | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | `None` (`marksman:11154`) | ✗ | ✗ |
| `command: triggerSuggest` | ✗ | ✗ | ✗ | ✗ | ✓ | ✓ (`apply_edits`) | ✗ | ✗ | ✓ |

**No Markdown/LSP-completion precedent in this corpus sets `commitCharacters` at all** — the only
hits in every digest are `all_commit_characters: None` in *server* capabilities
(`marksman:11154`, `mdoxide:4999`). That settles (d).

---

## 3. The four locked ordering schemes, reconciled

### 3.1 The structural finding

Schemes locked by other tickets:

| Ticket | Demand | Location |
| :--- | :--- | :--- |
| **19** | three layers deduped by `FieldKey`; badges `none`/`⟨global⟩`/`⟨inferred⟩`; **`sortText` prefixed by layer number (0/1/2)** | `issues/19:44-48` |
| **23** | status list ordered by **config insertion order = `sort_text`**; `preselect` top; no removal item | `issues/23:24` |
| **15** | **`fuzzy-matcher` scoring** for wikilinks; GitHub-slug heading matching | `issues/15:64,239` |
| **20** | file-field completion **cap 100**, `is_incomplete: true`, relevance sort *recently opened → same folder → alphabetical* | `issues/20:37` |
| **16** | `filter_text = "#parent/child parent/child child"`, `label_details.detail` counts, `KEYWORD` | `issues/16:43-49` |
| **21** | **`textEdit` with explicit range for all completions** | `issues/21:48` |

**They never compete.** The dispatcher returns exactly one list from exactly one completer
(`dispatch:99-125`, `novel:185-189` — *"one region classifier → one completer → one list; never union
of region-candidate lists"*), and `ContextKind` C1–C17 are mutually exclusive by construction
(`dispatch:205-212`). There is therefore **no cross-context ranking problem at all** — only an
intra-list one, once per context. The "four schemes" are four *per-context comparators* behind one
`sortText` grammar.

The second finding (§2.1) is that `sortText` is a **tiebreak**, not a rank: VS Code lets a better
fuzzy match cross a tier boundary. So the honest statement of what each ticket bought is:

- **19's layer prefix orders ties, and its real layer-conflict work is the `FieldKey` dedup**
  (`issues/19:44`) — higher layers suppress lower, so a key never appears twice and the layer byte
  can never be contradicted by a duplicate. Layer ordering between *different* keys yields to fuzzy
  quality, which is the desired behaviour (you want the key you are typing to win).
- **23's config order survives intact**: `- [` admits a single status list, the filter word is
  empty or a short prefix of status names, and prefix matches of the same length score identically
  ⇒ `sortText` is the tiebreak ⇒ insertion order holds. (This is exactly VS Code's `wordLen === 0`
  comment path when nothing is typed.)
- **15's fuzzy score is a *server-side selection* signal**, not a display order — it decides which
  candidates survive the cap and what rank they get (§4.1), then the client applies *its own*
  matcher. Two matchers cannot be reconciled; they can only be made consistent in intent.
- **20's relevance sort is a comparator + a cap**, and is the one scheme that genuinely needs
  `isIncomplete` (§4.5).

### 3.2 Decisions

| # | Decision | Evidence |
| :--- | :--- | :--- |
| **R1** | **One `sortText` grammar for every context** (§4.1). Never omit `sortText`. | `capability-negotiation:395-397` (only order guarantee surviving every client); gopls `:196-201` + LSP #348; RA `to_proto.rs:477-495`; `suggest.ts:250-253,339-356` (mixed presence demotes to label order) |
| **R2** | **Never emit an unpadded numeric rank.** `{:04}` or a fixed-width hex/alnum tier, ASCII only, no locale-dependent characters, no negative-sign dependence. | RA comment *"Zero pad the string to ensure values can be properly sorted by the client"* (`to_proto.rs:488-493`); rumdl `{:04}` (`:29822`); TS fixed-width tiers (`completions.go:272-280`); pyrefly `{:04}`; **anti-pattern** mdoxide `score.to_string()` (`mdoxide-src:4835,4820`) |
| **R3** | **The server emits items already sorted ascending by `sortText`, then truncates to the cap.** Rank-then-truncate, never the reverse. | rumdl *"Collect … so we can rank before truncating"* (`:29796`); RA `res.sort_by(…); res.truncate(limit)` (`to_proto.rs:283-285`); pyrefly `is_sorted_by_key((&sort_text, &label))` test |
| **R4** | **Ranking policy is per-context; the dispatcher must not re-rank.** One list, one comparator, defined by the owning ticket. | `dispatch:99-125,205-212`; `novel:185-189` |
| **R5** | **Server-side fuzzy (`fuzzy-matcher`) is used for *selection and intra-group rank*, never to claim cross-client display order.** Document that display order among differently-matching items is the client's. | `capability-negotiation:387-394`; §2.1 consequence 1; gopls/TS/RA all hand the client a score-shaped `sortText` and accept client re-ranking |
| **R6** | **Always set `filterText`, and co-design it with `textEdit.range.start`.** Never leave it to the label. | §1.1 R-SHARED-FILTER-WORD; rumdl `:29927-29929`; mdoxide `completion_filter_text` (`:3552-3554`); §2.1 consequence 2; zk's `InsertText`-without-`filterText` tag path is the counter-example to avoid (`zk-src:4864-4868`) |
| **R7** | **`preselect` at most one item, and only on a *positively identified* best candidate** — exact typed match (mdoxide `:4520-4526`), type-checker/top relevance (RA `to_proto.rs:481-484`, TS `completions.go:2439`), or the single config-ordered item in a closed list (23). Never "whatever is first" (gopls `:204` is the weak pattern). | [S1:804-812]; `capability-negotiation:414` (`preselectSupport` true in VS Code/Neovim, absent in Zed/Helix ⇒ harmless to set) |
| **R8** | **`isIncomplete` is a *semantic* flag, not a tuning knob**: `true` iff the next keystroke could yield items the current response does not contain (capped-away, or not-yet-answerable). Otherwise `false`. | §4.5; rumdl `:29811,29952`; Marksman `:11785`; TS `completions.go:1948`; pyrefly threshold tests |
| **R9** | **One cap policy, one number** (§4.6). | §4.6 |

---

## 4. The unified ranking policy (the recommendation)

### 4.1 `sortText` grammar

```rust
// ASCII only. Fixed width prefix. Lowercased label tail.
fn sort_text(group: char, rank: u32, label: &str) -> String {
    format!("{group}{rank:04}{}", label.to_lowercase())
}
```

| Segment | Width | Meaning | Default |
| :--- | :-: | :--- | :--- |
| `group` | 1 char | **semantic group that must dominate relevance** — 19's layer, 20's relevance class, rumdl's dir/file. | `'0'` |
| `rank` | 4 digits, zero-padded | **position within the group under the context's comparator** (declaration order, fuzzy order, reference count, recency index). | `0000` |
| `label` | rest | lowercase label — deterministic final tiebreak and self-description; makes `sortText` meaningful if a client drops items and re-sorts. | — |

Why this shape and not `rank` alone (a global index):

- It matches every well-behaved precedent (rumdl `{:04}`+label, TS tier+name, pyrefly
  base+`{:04}`+label, MS md LS tier+label, RA hex score) and avoids mdoxide's unpadded bug (R2).
- `group` is the only place a ticket's *semantic* ordering lives, so 19's literal
  `"sortText prefixed by layer number (0/1/2)"` (`issues/19:48`) is satisfied verbatim by
  `group = layer`.
- `rank` is assigned **after** the context comparator runs and **before** truncation, so `sortText`
  is monotonically non-decreasing across the emitted array (R3) and pyrefly's
  `(sort_text, label)` sortedness invariant holds.
- `{:04}` caps at 9 999; every context caps ≤100 (§4.6), so no overflow path exists.

**Order of operations per request** (the contract the shared completion-generation function
implements — decomposition `:50`):

```
1. classify (unit 1)  → ContextKind + replace range + filter word
2. generate candidates for that one context
3. dedup              → context-specific key (19: canonical FieldKey; 16: tag path; 15: link target)
4. context comparator → group, then rank, then label        ← the four locked schemes live here
5. assign sort_text   → group + {:04} rank + lowercase label
6. truncate to cap    (rank-then-truncate, R3)
7. build items        → label / labelDetails / kind / textEdit / filterText / preselect / sort_text
8. is_incomplete      → produced > cap  (§4.5)
```

Steps 4–8 are O(n log n) over ≤cap+1 items and belong inside ticket 33's ≤20 ms handler budget
(`latency:18`); steps 1–2 are the ones `latency:202-221` requires to be precomputed.

### 4.2 Per-context comparators (`group` / `rank` semantics)

| Ctx | Context | `group` | `rank` within group | Notes |
| :-- | :--- | :--- | :--- | :--- |
| C1 | wikilink target | `0` | server `fuzzy-matcher` score order (desc), tie → path proximity | 15 (`issues/15:64,239`) |
| C2 | wikilink heading | `0` | same-doc headings first (`0`), cross-doc (`1`) as `group`; then slug alpha | 15 slugger (`issues/15:73`) |
| C3 | md-link target/anchor | `0` | as C1 | ownership per unit 2 (`dispatch:192`) |
| C4 | body tag | `0` | reference count desc, then path alpha | 16; counts already surfaced (`issues/16:49`) |
| C5 | frontmatter tag value | `0` | same as C4 | 16 — **must be identical ordering to C4** (same entity) |
| C6 | frontmatter key | layer `0`/`1`/`2` | alphabetical within layer | **19's demand, verbatim** (`issues/19:48`) |
| C7 | frontmatter value | `0` | schema enum order → alpha | 20 |
| C8 | inline-field key | `0` | same as C6 (same key universe) | 19 |
| C9 | task status | `0` | **config insertion index** | 23 (`issues/23:24`) |
| C10 | task date value | `0` | recency → ISO alpha | 23 + 19 predicate |
| C11 | daily-note date | `0` | date proximity to cursor date | 18 |
| C12 | footnote ref | `0` | definition order in document | 17 |
| C13 | callout type | `0` | mdoxide's enum order (canonical first) | 17 |
| C14 | query DSL token | token class (`0` field / `1` op / `2` value) | name alpha | 21 |
| C15 | template helper member | probe kind (`0` function / `1` property / `2` value) | name alpha | 22 |
| C16 | template include name | `0` | local dir first, then global; alpha | 22 (`loader.rs:85-99` precedence) |
| C17 | file-field / fileClass value | **relevance class**: `0` recently opened, `1` same folder, `2` rest | index / alpha | 20 (`issues/20:37`) verbatim |

Uniformity claim: every row reduces to *(semantic group that must dominate) → (intra-group relevance
rank) → (alphabetical)*, which is rumdl's `{:04}`+label, TS's tier+name, pyrefly's
base+`{:04}`+label and MS md LS's tier+label compressed into one grammar.

### 4.3 `filterText` rules

1. **Always set it** (R6). Absent ⇒ client matches `label`, and every badge/qualification in `label`
   becomes a filter hazard (§2.1 consequence 2).
2. **`filterText = (text from `textEdit.range.start` to the name) + (name to insert), plus any
   alias forms the ticket requires.** The first character must equal the first character of the
   replace range's content.
3. Per context:

| Ctx | `textEdit.range.start` | `filterText` | Source |
| :-- | :--- | :--- | :--- |
| C4 body tag | at the typed `#` | `"#parent/child parent/child child"` | `issues/16:43-44` (locked; compound = prefix/path/leaf forms) |
| C5 frontmatter tag | at the first char of the bare word (**no `#`**) | `parent/child parent/child child` — **the `#…` form must be dropped**, because the range excludes `#` | R-SHARED-FILTER-WORD; `issues/16:46-47` |
| C6/C8 keys | at the first char of the key token | **raw `FieldKey`** (no badge) | `novel:162-165` |
| C1/C2/C3 | at the typed `[`/`#` that opens the target | `preceding + inserted text`, e.g. `[[note` / `[[note#head` | rumdl `:29927-29929`; mdoxide `:3552-3554` |
| C9 task status | **after `[`** (`issues/23:24`) | status name only, no `[` | R-SHARED-FILTER-WORD |
| C17 file field | at the first char of the value token | full replacement text | rumdl `:29927-29929` |

4. `filterText` is what makes `label` safe to carry badges/counts (see §5), so **`filterText` never
   contains a badge.**

### 4.4 `label` / `labelDetails` / `detail`

Spec guidance: *"If label details are provided, the label itself should be an unqualified name"*
[S1:762-767]. 19 wants badges in the completion (`issues/19:44`), 16 wants counts in
`label_details.detail` (`issues/16:49`), and `labelDetailsSupport` is absent in Helix
(`capability-negotiation:418,450-454`).

**Decision:**

| Content | When `labelDetailsSupport` | When absent |
| :--- | :--- | :--- |
| name | `label` = unqualified name | `label` = unqualified name |
| count (`47 references`) | `labelDetails.detail` | `detail` (always-available field, `ls-types` `src/completion.rs:445`) |
| provenance badge (`⟨global⟩`, `⟨inferred⟩`, `Unresolved`) | `labelDetails.description` | appended to `label` |

- Precedent for exactly this split: Markdown Oxide puts counts in `labelDetails.detail`
  (`mdoxide-src:5375-5379`) and provenance in `labelDetails.description` (`mdoxide-src:4503-4509`).
- `description` is the spec's slot for "fully qualified names or file paths" [S1:714-716] — a
  provenance marker is the closest sanctioned use, and it keeps `label` unqualified as [S1:762-767]
  asks.
- **Amendment to 19**: badges move from "in the completion" to *"in `labelDetails.description` when
  supported, else appended to `label`"*. Rationale: preserves 19's user-visible outcome, satisfies
  the spec's unqualified-label guidance, and degrades correctly on Helix rather than vanishing.
- **Correction to 16**: `label_details.detail` matches **Markdown Oxide** (`mdoxide-src:5375`) but
  **not zk** — zk puts counts in the top-level `detail` field and sets no `labelDetails`
  (`zk-src:4867`). The decision stands; the citation in `issues/16:49` is wrong (§10).
- `documentation` is *not* part of this unit (unit 4 / resolve). `detail` must not be used as a
  resolve placeholder — it is now a live fallback slot.

### 4.5 `isIncomplete`

**Rule (R8): `is_incomplete = (candidates_after_dedup > cap)`.** Nothing else.

| Situation | Value | Why |
| :--- | :-: | :--- |
| closed/small set fully returned (statuses, callout types, schema keys, footnote defs) | `false` | nothing more to ask for |
| open set under the cap | `false` | client filters the shipped list; **zero extra round trips** |
| open set truncated at the cap | `true` | the next keystroke may surface an item this response cut |
| segment-changing input where the server must re-list (rumdl's directories) | `true` always | precedent `rumdl-src:29952-29954` — **no Traces context is like this in v1**; flagged as a pattern to adopt only if a "drill into" context appears |
| candidate source temporarily unavailable (22's degraded mode) | `true` | `dispatch:251-254`: declining context must not look final |

**Amendment to 20** (`issues/20:37` says `is_incomplete: true` unconditionally): make it
cap-conditional. Evidence: rumdl `matches.len() > MAX_ITEMS` (`:29811`), Marksman
`>= maxCompletions` (`:11785`), TS `hasUnresolvedAutoImports` (`completions.go:1948`), pyrefly's
threshold tests. Cost of unconditional `true`: with `contextSupport` on, every keystroke becomes a
fresh `textDocument/completion` round trip through `concurrency_level(1)` (`issues/12:20-36`), which
is precisely the head-of-line blocking `latency:223-233` warns about — and it is *wrong* (the
response would be identical). 20's requirement that the client keep asking for a *capped* list is
fully preserved by the conditional form.

Note the interaction with unit 1: `isIncomplete`-driven re-requests arrive as
`TriggerForIncompleteCompletions` (3.18 changelog `docs/refs/lsp_spec.md:867`), and `dispatch:245-250`
already routes them through the same classifier, so **nothing in this rule requires reading
`context`**.

### 4.6 Caps

| Decision | Value | Evidence |
| :--- | :--- | :--- |
| **One completion cap for Traces, all contexts** | **`COMPLETION_CAP = 100`** | 20's locked number (`issues/20:37`); serialization, not scoring, is what blows the budget — `latency:193` S8 *"thousands of items ≈ out of budget → cap, don't optimize"*, `crates:586-588` |
| Eviction | **rank-then-truncate**, by `sortText` ascending, after dedup | rumdl `:29796,29812`; RA `to_proto.rs:283-285` |
| Cap source | constant in config (`completion.cap`), not per-context magic numbers | one number to reason about; Marksman makes it configurable (`marksman:5532`) |
| `isIncomplete` | `= produced > COMPLETION_CAP` | §4.5 |
| What is *not* a completion cap | rust-analyzer's **128** is its `workspace/symbol` default (`research/23:442`), and **ticket 16 contains no cap at all** — see §10 correction 3 | — |

Ecosystem caps for calibration: rumdl 50, Marksman 50 (configurable), Markdown Oxide 20, RA
(config `limit`), 20's 100. 100 is at the top of the range but is the one Traces already locked, and
it sits inside the serialization budget at ~100 B/item ≈ 10 KB JSON (`latency:19`, `:193`).

---

## 5. Per-context item contract (fields set / unset)

`✓` = set on every item; `—` = never set; `⊗` = set only under the stated condition.

| Field | C1–C3 link/anchor | C4–C5 tags | C6–C8 keys/fields | C9–C10 tasks | C11–C13 date/footnote/callout | C14–C16 query/template | C17 file fields |
| :--- | :--: | :--: | :--: | :--: | :--: | :--: | :--: |
| `label` | ✓ name | ✓ tag path | ✓ unqualified `FieldKey` | ✓ status/emoji name | ✓ name | ✓ member/name | ✓ value |
| `labelDetails` | — | `.detail` count ⊗ | `.detail` count ⊗, `.description` badge ⊗ | — | — | — | — |
| `detail` | path fallback ⊗ | count fallback ⊗ | count fallback ⊗ | — | — | signature ⊗ | fileClass name ⊗ |
| `kind` | `Reference` 18 | `Keyword` 14 | `Field` 5 | `Enum` 13 | see §7 | see §7 | see §7 |
| `preselect` | ⊗ exact typed target (mdoxide `:4520-4526`) | — | — | ✓ config-first item (23) | — | — | ⊗ exact typed value |
| `sortText` | ✓ grammar §4.1 | ✓ | ✓ (group = layer) | ✓ (rank = config index) | ✓ | ✓ | ✓ (group = relevance class) |
| `filterText` | ✓ R6 | ✓ compound (§4.3) | ✓ raw key | ✓ status name | ✓ raw token | ✓ raw token | ✓ full replacement |
| `insertText` | — | — | — | — | — | — | — |
| `insertTextFormat` | ⊗ unit 5 only | ⊗ unit 5 only | — | ⊗ stage-2 (§8) | ⊗ unit 5 only | ⊗ unit 5 only | — |
| `textEdit` | ✓ | ✓ | ✓ | ✓ | ✓ (spanning, 23) | ✓ | ✓ |
| `additionalTextEdits` | — | — | — | — | — | — | ⊗ only if a truly independent edit appears (none planned) |
| `commitCharacters` | — | — | — | — | — | — | — |
| `command` | — | — | — | ⊗ stage-2 (§8) | — | — | ⊗ drill-down only |
| `documentation` | unit 4 | unit 4 | unit 4 | — | — | unit 4 | unit 4 |
| `tags` / `deprecated` | — | — | — | — | — | — | — |
| `data` | unit 4 | unit 4 | unit 4 | — | — | unit 4 | unit 4 |

Uniform invariants (they are the answer to decomposition `:51`'s "per-context item-shape table"):

1. **Exactly one `textEdit`, always with an explicit range** (21, `issues/21:48`), and
   `insertText` omitted (spec ignores it anyway [S1:869-871] — omitting saves bytes and removes an
   ambiguity for clients that disagree with the spec).
2. **`sortText` and `filterText` on 100% of items**, in the §4.1 grammar / §4.3 pairing.
3. **No `commitCharacters`, ever** (§8).
4. **No `itemDefaults` / `textEditText` / `applyKind`** — unavailable through `ls-types`
   (`capability-negotiation:484-490`, `crates:486-488`).
5. **No `insertTextMode`, no `insertText`, no `deprecated`.**

---

## 6. `textEdit` vs `additional_text_edits` vs `insertText` — decision table

| Edit shape | Example | Vehicle | Notes |
| :--- | :--- | :--- | :--- |
| Replace one token on one line, insert text contains no tab stops | `#pro` → `#projects/active`; `field` → `field_name`; `status` after `[` | **`CompletionTextEdit::Edit(TextEdit)`** with explicit range | `ls-types` `src/completion.rs:266-269`; capability-gated `InsertReplaceEdit` *not* used in v1 (§6.1) |
| Replace a token **and** leave the cursor inside inserted structure | `[[ta` → `[[target\|]]` | **one `textEdit` whose `newText` is a snippet**, `insertTextFormat = Snippet` | [S1:845-852]: `insertTextFormat` *does* apply to `textEdit.newText`; unit 5 owns the snippet bodies |
| Replace an already-paired region (auto-closed `]]`, `"`) | `[[foo|]]` with cursor inside | **one `textEdit` whose range extends past the cursor** to cover the closing token | range still "contains the request position" [S1:864-866]; zk's `newTextEditForLink` does the same (`zk-digest:4184-4260`) — zk additionally pushes the *notebook-wide* edits to `additionalTextEdits` |
| Spanning edit on one line (emoji slot, `[due:: ` value) | `📅 ` slot → ISO date | **one `textEdit` spanning the slot** | `issues/23:27,29`; needs `byte_to_utf16_cu` (ticket 11/19 prerequisite, `issues/23:29`) |
| Multi-token rewrite of the whole link including surrounding punctuation | full-path replacement (16 body) | **one `textEdit`** from the opening token through the cursor | `issues/16:43` |
| An edit genuinely unrelated to the cursor (file-top import-equivalent) | *none planned in Traces v1* | `additionalTextEdits` | [S1:917-923]: must not overlap the main edit or each other; **`insertTextFormat` does not apply** [S1:851-854] ⇒ never a snippet. Keep the reserved slot, do not populate it |
| Rely on the client's word-guessing | — | **never** | `insertText` mode [S1:13]; zk tag items are the counter-example (`zk-src:4866`) and would diverge per client on `#`/`[` word-boundary rules |
| Post-insert side effect | re-open the widget (23 stage-2) | `command` ⊗ (§8) or `isIncomplete` session | §8 |

### 6.1 `InsertReplaceEdit` — defer

`insertReplaceSupport` is advertised by VS Code, Neovim, nvim-cmp, Zed and Helix
(`capability-negotiation:415`), so it is *available*. It is not *needed* for v1: its only benefit is
"insert without eating the tail after the cursor", and every Traces context either (a) replaces
exactly the typed token, or (b) deliberately replaces the tail (paired `]]`). Introduce it only when
a concrete context wants "accept but keep what follows the cursor". Recorded as open question Q3.

---

## 7. `CompletionItemKind` taxonomy

Two constraints already on the table: the 1–18 cliff (`capability-negotiation:94-113`) and 16's
locked `KEYWORD` (`issues/16:48`).

**Decision: Traces emits only kinds in 1–18.** Not as a clamp applied per client, but as the
*source* set. Rationale: the clamp-or-accept choice is otherwise a per-client behavioural fork
(`capability-negotiation:108-113`), Helix does not advertise `valueSet` (`:420`), and every kind
Traces actually needs fits. One small match in the client, zero divergence between Helix and VS Code.
The two casualties are `FOLDER`=19 and `EVENT`=23 — both have acceptable 1–18 substitutes below.

| Ctx | Kind | Value | Basis |
| :-- | :--- | :-: | :--- |
| C1/C2/C3 link target, heading anchor, link target | `Reference` | 18 | Marksman all-Reference (`marksman:4791-5078`), zk notes (`zk-src:4959`), mdoxide headings/blocks (`mdoxide-src:4492`) |
| C4/C5 tags (body + YAML) | `Keyword` | 14 | locked (`issues/16:48`); mdoxide tags (`mdoxide-src:5371`) — **mdoxide only; zk sets no kind on tag items** (§10) |
| C6 frontmatter key, C8 inline-field key | `Field` | 5 | `capability-negotiation:103` (`research/20:365`, `research/21:96`) |
| C7 frontmatter/enum value | `Value` | 12 | mdoxide/`Value` value completion (`msmd:5817`); clamp target for `ENUM_MEMBER` (`capability-negotiation:106`) |
| C9 task status | `Enum` | 13 | it *is* an enum member of a configured value set; 13 ≤ 18 |
| C10 task date value | `Value` | 12 | |
| C11 daily note | `Reference` | 18 | **instead of mdoxide's `EVENT`=23** (`mdoxide-src:4499`) — avoids the 1–18 divergence; daily notes are link targets here |
| C12 footnote ref | `Reference` | 18 | mdoxide footnotes (`mdoxide-src:3763`) |
| C13 callout type | `Keyword` | 14 | mdoxide uses `SNIPPET`=15 (`mdoxide-src:3645`), which is also ≤18 and defensible; `Keyword` chosen because Traces v1 inserts a literal type token, not a multi-line snippet body. **Q4** |
| C14 query DSL token | token class: field `Field` 5 / operator `Keyword` 14 / value `Value` 12 | | `research/21` |
| C15 template helper member | probe-derived: function `Function` 3, property `Property` 10, value `Value` 12; unknown root ⇒ **decline, no item** | | 22's D5 + `issues/22:35` (never emit a degraded item) |
| C16 template include/extends name | `File` 17 | | it resolves to a file (`TemplateLoader::find`) |
| C17 file-field / fileClass value | class → `Class` 7; enumerated option → `Value` 12; free string → `Text` 1; file path → `File` 17 | | `research/20:365-366` — **`ENUM_MEMBER`=20 is deliberately avoided** |

`completionItemKind.valueSet` handling then reduces to: nothing to clamp, for every row above.

---

## 8. `commitCharacters`, `command`, `tags`, `insertTextMode`

- **`commitCharacters`: do not set, in any context, ever.** Four independent reasons: (i) **zero
  precedent** — not one server in the corpus sets them; the only digest hits are
  `all_commit_characters: None` in server *capabilities* (`marksman:11154`, `mdoxide:4999`);
  (ii) in text-edit mode a commit character would be typed *into* an active replacement range,
  exactly the interaction [S1:925-929] leaves to client interpretation; (iii) Traces' trigger
  characters (`[`, `#`, `:`, `.`, …) are already registered server-side, so the client re-asks on
  them anyway and a commit character would only duplicate a trigger (`dispatch:216-232`); (iv) Zed
  and Helix do not advertise `commitCharactersSupport` (`capability-negotiation:416`) ⇒ setting them
  would create a per-client behavioural fork for no benefit.
- **`command`:** set **only** for "re-open the suggest widget after accepting a *prefix* item", and
  only where no portable alternative exists. Precedent is exactly this use: rumdl
  `editor.action.triggerSuggest` on directories (`rumdl-src:29941-29946`) and MS md LS on
  directories (`msmd:5915`). **Portability caveat**: `command` is a raw client command id, not an
  LSP-namespaced request; Neovim's core LSP client does not execute completion-item commands. So the
  portable route for 23's stage-2 emoji flow (`issues/23:30`) is an `isIncomplete` session plus the
  space that follows `📅 ` as a trigger — with `command` as a best-effort extra. **Q5.**
- **`tags`/`deprecated`:** unused. `deprecated` has no Traces concept; `CompletionItemTag.Deprecated`
  would be the modern spelling anyway (`capability-negotiation:417`).
- **`insertTextMode`:** leave unset — the client default comes from
  `completion.insertTextMode` (`capability-negotiation:419`); no Traces edit has indentation
  semantics.

---

## 9. Seams (what this unit hands off, and what it must not decide)

1. **Unit 4 (resolve split).** Everything immutable is already fixed here: `sortText`, `filterText`,
   `insertText`, `textEdit` must be present at `completion` time and never change on resolve
   [S1:4]. `documentation`, `detail`-as-preview and `data` are the only sensible resolve payloads —
   but note §4.4 has now made `detail` a *live* field (labelDetails fallback), so unit 4 must not
   reserve it. `resolveSupport.properties` per client: `capability-negotiation:412`.
2. **Unit 5 (snippets).** The *vehicle* is decided here (§6: snippet inside the single main
   `textEdit.newText`); the snippet *bodies*, tab-stop layouts and the `[[x|]]` cursor placement are
   unit 5's. Also unit 5's: whether `insertTextFormat` may be sent when `snippetSupport` is absent
   (answer implied by `capability-negotiation`: no — it must degrade to `PlainText`, and §6 rows
   that need tabs must then be re-cut as plain text).
3. **Unit 6 (latency).** Steps 1–2 of §4.1 are the memoizable stages; steps 4–8 are per-request and
   must fit the S-scoring/serialization rows (`latency:176-200`). The cap is a *budget* mechanism
   (`latency:193`), so if the budget tightens, the cap is the first knob.
4. **Unit 8 (capability negotiation).** Declared gates, to be wired there: `labelDetailsSupport`
   (§4.4), `completionItemKind.valueSet` (§7 — now a no-op), `snippetSupport` (§6), `preselectSupport`
   (harmless to ignore — R7), `insertReplaceSupport` (Q3), `resolveSupport` (unit 4).
5. **Unit 1 (dispatcher).** This unit consumes `ContextKind` + prefix/replace range
   (`dispatch:260-261`) and adds one requirement back: **the classifier must also return the replace
   range, not just the context**, because §4.3's R-SHARED-FILTER-WORD makes `range.start` part of the
   item contract. If the dispatcher only returns a `ContextKind`, the item builder must re-derive the
   range — wasteful and error-prone.
6. **Not decided here:** trigger set (unit 2), rumdl coordination (ticket 32), `isIncomplete`
   *re-trigger* routing (unit 1 `:245-250`), workspace-symbol caps (ticket 27/23 — a different
   response type entirely).

---

## 10. Corrections / proposed amendments to sibling tickets

1. **`issues/16:49` — zk citation is wrong.** zk's tag completion sets top-level `Detail:
   "3 notes"`, no `labelDetails`, no `kind` (`zk-src:4864-4868`). Markdown Oxide *does* use
   `label_details.detail = "{n} references"` (`mdoxide-src:5375-5379`). The decision (counts in
   `labelDetails.detail`) stands on the Markdown Oxide precedent + §4.4's fallback.
2. **`issues/16:48` — "KEYWORD standard in Markdown Oxide and zk"**: true for Markdown Oxide tags
   (`mdoxide-src:5371`); **zk sets no kind on tag items at all** and uses `Reference` only on notes
   (`zk-src:4959`). Rest the decision on Markdown Oxide.
3. **`issues/23:52` — "cap 128 (16 precedent)" is a mis-attribution.** Ticket 16 contains no cap of
   any kind. 128 is rust-analyzer's `workspace/symbol` default (`research/23:442`), a different
   response. Real completion caps: rumdl 50, Marksman 50, Markdown Oxide 20, Traces 100 (20).
4. **`issues/20:37` — `is_incomplete: true` → cap-conditional.** §4.5. Preserves 20's intent ("keep
   asking while results may exist") without a guaranteed-waste round trip per keystroke under
   `concurrency_level(1)`.
5. **`issues/19:48` — layer `sortText` prefixes: keep, with a sharpened rationale.** They are the
   *tiebreak*, not a tier that beats fuzzy match (§3.1); the layer guarantee actually rests on the
   `FieldKey` dedup, and the badge placement moves to `labelDetails.description` per §4.4.
6. **`issues/21:48` — "textEdit for all" is right but incomplete**: it must be paired with
   R-SHARED-FILTER-WORD, otherwise `filterText` and `range.start` drift and items silently vanish
   (rumdl `:29927-29929`, §1.1).
7. **Phantom capability `documentationMarkdown`** does not exist (`capability-negotiation:490`) —
   recorded here only so unit 4 does not re-derive it.

---

## 11. Ready-to-paste input lines for ticket 24

```
INPUT from 24 / completion-item-shape-and-ranking (research/24-completion-item-shape-and-ranking.md)

S-1  One item shape for every context: exactly one CompletionTextEdit with an explicit range,
     insertText omitted, sortText + filterText on 100% of items, no commitCharacters, no
     insertTextMode, no tags/deprecated, no itemDefaults/textEditText/applyKind (ls-types gap).
     [issues/21:48; S1:869-871,845-854; capability-negotiation:484-490]

S-2  sortText grammar: format!("{group}{rank:04}{}", group_char, rank, label.to_lowercase()).
     group = the context's semantic group (19's layer 0|1|2; 20's relevance class; task 0);
     rank = position under the owning ticket's comparator, assigned AFTER ranking and BEFORE
     truncation. Never emit an unpadded number (RA to_proto.rs:488-493; rumdl:29822; mdoxide
     anti-pattern src:4835). Server emits the array already sorted by sortText ascending.
     [issues/19:48, 20:37, 23:24, 15:64; S1:819-823]

S-3  Ranking is per-context, never cross-context: the dispatcher returns ONE list from ONE
     completer (C1..C17 are mutually exclusive), so the four locked schemes are four comparators
     behind one grammar, not four competing orders.
     [dispatch:99-125,205-212; novel:185-189]

S-4  Client reality: VS Code orders by fuzzy score DESC, then sortText ASC (lowercased), then
     label, then kind; sortText never outranks a better fuzzy match and decides EVERYTHING only
     when the filter word is empty (completionModel.ts:172-178,244-259; suggest.ts:339-356).
     Therefore server fuzzy-matcher output is a SELECTION + intra-group rank signal, not a
     promised display order. [S1:8; capability-negotiation:387-394]

S-5  filterText is co-designed with textEdit.range.start: filterText must match
     range.start..cursor (R-SHARED-FILTER-WORD). Consequences: C4 body tag keeps
     "#parent/child parent/child child"; C5 YAML tag MUST drop the leading "#…" form because its
     range excludes '#'; C6/C8 keys = raw FieldKey (no badge); C9 = status name only (range is
     after '['); C1-C3 = preceding + inserted text. Wrong filterText DELETES the item
     (completionModel.ts:198-205). Always set filterText. [issues/16:43-47; rumdl-src:29927-29929;
     mdoxide-src:3552-3554; novel:162-165]

S-6  label / labelDetails: label = unqualified name (S1:762-767); counts in labelDetails.detail,
     provenance badge in labelDetails.description; when labelDetailsSupport is absent, counts
     fall back to `detail` and the badge appends to `label`. AMENDS 19 (badge placement).
     [issues/19:44, 16:49; mdoxide-src:4503-4509,5375-5379; capability-negotiation:418]

S-7  preselect: at most one item, only on a positively identified best candidate (exact typed
     match, top relevance, or the single config-ordered item). 23's "preselect top" is satisfied
     by config order + preselect on item 0. Never "whatever happens to be first".
     [S1:804-812; issues/23:24; mdoxide-src:4520-4526; RA to_proto.rs:481-484; TS completions.go:2439]

S-8  isIncomplete = (candidates_after_dedup > cap), nothing else. AMENDS 20 (unconditional true).
     Evidence: rumdl-src:29811, Marksman:11785, TS completions.go:1948, pyrefly threshold tests.
     Re-trigger routing stays unit 1's (dispatch:245-250); contextSupport not required.
     [issues/20:37, 12:20-36; latency:223-233]

S-9  Cap: one constant COMPLETION_CAP = 100 for all contexts; rank-then-truncate by sortText
     ascending after dedup. 128 is rust-analyzer's workspace/symbol default, NOT a completion
     precedent, and ticket 16 states no cap. CORRECTS 23:52.
     [issues/20:37; research/23:442; rumdl-src:29796,29810-29812; RA to_proto.rs:283-285;
     latency:193]

S-10 Kind: Traces emits ONLY CompletionItemKind 1..18 — full C1..C17 table in §7. Avoids the
     valueSet clamp fork entirely (FOLDER=19 and EVENT=23 replaced by FILE=17 / REFERENCE=18).
     [capability-negotiation:94-113,420,450-454]

S-11 commitCharacters: never set. Zero precedent across the whole corpus (only
     all_commit_characters: None in server capabilities); would fork behaviour against Zed/Helix
     which do not advertise the flag; Traces' own trigger chars already re-request.
     [S1:925-929; marksman:11154; mdoxide:4999; capability-negotiation:416]

S-12 Edits: one main textEdit per item; snippets live in textEdit.newText (insertTextFormat DOES
     apply there, does NOT apply to additionalTextEdits); additionalTextEdits reserved but
     unpopulated in v1; InsertReplaceEdit deferred (Q3). Snippet bodies = unit 5.
     [S1:845-854,917-923,864-866; issues/21:48; zk-digest:4184-4260]

S-13 command: only for "re-open suggest after accepting a prefix item", as a best-effort extra
     behind a portable isIncomplete-session route (23 stage-2). Precedent: rumdl-src:29941-29946,
     msmd:5915. [issues/23:30; S1:931-935]

S-14 Dispatcher contract addition: classification must return the REPLACE RANGE alongside
     ContextKind, not ContextKind alone — range.start is part of the item contract (S-5).
     [dispatch:260-261; S1:15]

S-15 Seams: resolve payloads = documentation + data (NOT detail — detail is now a live fallback);
     snippet bodies = unit 5; capability gates = unit 8; cap is the latency budget's first knob.
     [S1:4; §4.4; latency:193]
```

---

## 12. Open questions

- **Q1 — Is a client's fuzzy score crossing our `group` tiers acceptable for *every* context?**
  It is explicitly acceptable for C6/C8 (19: dedup already resolved the real conflict) and for C9
  (single closed list). It is *least* obviously acceptable for C17, where 20 wants recency to beat
  alphabetical and a long-shot prefix match could jump a class. Options: (a) accept (client freedom
  is the spec's stated design, [S1:8]); (b) server-filter C17 hard enough that only same-class
  candidates ship, then `isIncomplete = capped`. Recommendation: (a), because (b) costs a round trip
  per keystroke (§4.5) — but this is the one place a grilling should push.
- **Q2 — Does the `rank` segment need to be *meaningful* outside the server?** If a client drops
  items and re-sorts, `group` still orders correctly but `rank` gaps appear. Harmless (they only
  preserve relative order), yet worth stating in the implementation spec so nobody "fixes" it by
  renumbering client-side.
- **Q3 — `InsertReplaceEdit`**: introduce now or only when a context wants "accept but keep the
  tail"? Needs a concrete case before spending the branch (§6.1).
- **Q4 — C13 callout kind**: `Keyword` 14 (literal token) vs mdoxide's `Snippet` 15 (which would
  also switch the row to `insertTextFormat = Snippet` and drag unit 5 in).
- **Q5 — C9/C10 stage-2 delivery**: portable `isIncomplete` session vs `command:
  editor.action.triggerSuggest`. Evidence favours `isIncomplete` (Neovim does not execute
  completion-item commands) — confirm with the 23 grilling.
- **Q6 — Should `COMPLETION_CAP` be per-context after all?** 100 for C17 (20's number) vs a lower
  cap for C1/C4 vault-wide sets would tighten serialization, but breaks the "one knob" property of
  §4.6. Deferred until `latency`'s S-serialization measurement exists (`latency:263-270`).
- **Q7 — Unverified**: whether nvim-cmp's own ranking (as opposed to VS Code's) treats `sortText`
  as tertiary or primary. It changes nothing for Traces (S-1/S-2 hold either way) but would sharpen
  S-4's wording. **QUESTION** — not load-bearing.

---

## Sources

**Spec** — [S1] <https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.18/language/completion.md> (fetched 2026-10-06). Changelog dating: `docs/refs/lsp_spec.md:749-867`.

**Clients / servers (primary source, fetched 2026-10-06 unless from local digest)**

- VS Code: `src/vs/editor/contrib/suggest/browser/suggest.ts` (`:96-97,250-253,291-331,339-384`),
  `src/vs/editor/contrib/suggest/browser/completionModel.ts` (`:140,152-155,172-228,235,244-259`),
  `src/vs/editor/contrib/suggest/browser/wordDistance.ts` (`:45-90`),
  `src/vs/editor/common/config/editorOptions.ts` (`:5170,5227-5231`) —
  <https://github.com/microsoft/vscode>
- gopls: `gopls/internal/server/completion.go:69-71,96,194-205` —
  <https://github.com/golang/tools>
- TypeScript (Go rewrite): `tsc/internal/ls/completions.go:269-292,1948,2439,5040-5110,4855-4935` —
  <https://github.com/microsoft/TypeScript>
- rust-analyzer: `crates/rust-analyzer/src/lsp/to_proto.rs:251-285,477-495`,
  `crates/rust-analyzer/src/handlers/request.rs:1175-1201` —
  <https://github.com/rust-lang/rust-analyzer>
- pyrefly: `pyrefly/lib/lsp/wasm/completion.rs:96-140`,
  `pyrefly/lib/test/lsp/lsp_interaction/completion.rs` —
  <https://github.com/facebook/pyrefly/blob/b1e40b2d/pyrefly/lib/lsp/wasm/completion.rs>
- LSP issue #348 (`SortText` ranking), cited by gopls' own comment:
  <https://github.com/microsoft/language-server-protocol/issues/348>

**Local digests** — `mdoxide-src:3552-3554,4470-4530,4787-4837,5191-5198,5330-5390,5540-5640`;
`marksman:5113-5165,5421,11154,11766-11786`; `rumdl-src:29796-29830,29905-29955`;
`msmd:5517-5520,5764-5817,5907-5915`; `zk-src:4856-5010`, `zk-digest:4184-4260,10375-10468`.

**Crate on disk** — `ls-types 0.0.6` `src/completion.rs:266-269` (`CompletionTextEdit`),
`:425-566` (`CompletionItem`), `:567-580` (`CompletionItemLabelDetails`) — all fields this unit
needs are expressible except `textEditText`/`itemDefaults` (`capability-negotiation:484-490`).

**Siblings & tickets** — [24-part-decomposition](24-part-decomposition.md) §3;
[capability-negotiation](24-completion-capability-negotiation.md) §1/§1.2/§5/§6/§8;
[latency](24-completion-latency-architecture.md) §1/§4/§5/§6/§7;
[novel](24-novel-embedded-context-precedent.md) §4/§5;
[dispatch](24-completion-dispatch-architecture.md) §5-§8;
`issues/15`, `issues/16`, `issues/19`, `issues/20`, `issues/21`, `issues/22`, `issues/23`,
`issues/24`, `issues/33`; `research/23:442,500-501`.
