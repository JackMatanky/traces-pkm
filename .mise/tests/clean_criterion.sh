#!/bin/bash
# Regression tests: `mise run clean --criterion` must keep each benchmark's
# new/ and base/ runs (design: docs/superpowers/specs/2026-09-22-criterion-clean-preserve-runs-design.md).
# Red-green: fails against a full rm -rf.
# Usage: bash .mise/tests/clean_criterion.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "${ROOT}"

pass_n=0
fail() { printf 'not ok: %s\n' "$1" >&2; exit 1; }
ok() { pass_n=$((pass_n + 1)); printf 'ok %d: %s\n' "${pass_n}" "$1"; }

SBROOT="$(mktemp -d "${TMPDIR:-/tmp}/clean_crit.XXXXXX")"
trap 'rm -rf "${SBROOT}"' EXIT

# build_fixture <target-root> — criterion tree with two protected runs,
# a named + a custom baseline, change/, report/ at id and group level,
# one group with NO keep dirs at all, a dangling symlink victim (pins the
# wc-open failure tolerance in victim_kib), a sibling file cargo clean
# must never touch, and CACHEDIR.TAG (case 7's cargo clean
# --dry-run exits 101 without it).
build_fixture() {
  local t="$1"
  mkdir -p \
    "${t}/criterion/GroupA/1kb/new" \
    "${t}/criterion/GroupA/1kb/base" \
    "${t}/criterion/GroupA/1kb/main-old" \
    "${t}/criterion/GroupA/1kb/my-snapshot" \
    "${t}/criterion/GroupA/1kb/change" \
    "${t}/criterion/GroupA/1kb/report/both" \
    "${t}/criterion/GroupA/report" \
    "${t}/criterion/GroupB/2kb/main-only" \
    "${t}/criterion/GroupB/2kb/report" \
    "${t}/criterion/Weird Group (Note)/xkb/main-y" \
    "${t}/debug"
  printf 'LATEST-NEW\n'  > "${t}/criterion/GroupA/1kb/new/estimates.json"
  printf 'LATEST-NEW2\n' > "${t}/criterion/GroupA/1kb/new/sample.json"
  printf 'SECOND-BASE\n' > "${t}/criterion/GroupA/1kb/base/estimates.json"
  printf 'OLD\n'         > "${t}/criterion/GroupA/1kb/main-old/estimates.json"
  printf 'SNAP\n'        > "${t}/criterion/GroupA/1kb/my-snapshot/estimates.json"
  printf 'CHG\n'         > "${t}/criterion/GroupA/1kb/change/estimates.json"
  printf '<html>\n'      > "${t}/criterion/GroupA/1kb/report/index.html"
  printf '<svg>\n'       > "${t}/criterion/GroupA/1kb/report/both/violin.svg"
  printf '<ghtml>\n'     > "${t}/criterion/GroupA/report/index.html"
  printf 'ORPHAN-OLD\n'  > "${t}/criterion/GroupB/2kb/main-only/estimates.json"
  printf '<bhtml>\n'     > "${t}/criterion/GroupB/2kb/report/index.html"
  printf 'WEIRD\n'       > "${t}/criterion/Weird Group (Note)/xkb/main-y/estimates.json"
  printf 'keep\n'        > "${t}/debug/keepme.o"
  printf 'Signature: 8a477f597d28d172789f06886806bc55\n\ncargo\n' > "${t}/CACHEDIR.TAG"
  ln -s target-does-not-exist.json "${t}/criterion/GroupA/1kb/snap-link"
}

# Manifests hash sorted (cksum path) pairs — byte-identity across a run.
tree_manifest() { find "$1" -type f -exec cksum {} + | sort | cksum | tr -d ' \n'; }
keep_manifest() { find "$1/criterion/GroupA/1kb/new" "$1/criterion/GroupA/1kb/base" -type f -exec cksum {} + | sort | cksum | tr -d ' \n'; }
# Extract "N.M." from a report line so dry-run vs real numbers can be compared.
numstrip() { printf '%s' "$1" | grep -oE '(would remove|removed) [0-9]+ file\(s\) \([0-9]+ KiB\)' | grep -oE '[0-9]+' | tr '\n' '.'; }

# --- Case 1: surgical default keeps runs, deletes everything else (RED target)
T="$SBROOT/c1"; build_fixture "$T"
before_keep="$(keep_manifest "$T")"
out1="$(mise run -y clean --criterion --target-dir "$T" 2>&1)" || fail "case1: exit nonzero: $out1"
[[ -d "$T/criterion/GroupA/1kb/new" ]]  || fail "case1: new/ was deleted"
[[ -d "$T/criterion/GroupA/1kb/base" ]] || fail "case1: base/ was deleted"
[[ "$before_keep" == "$(keep_manifest "$T")" ]] || fail "case1: protected content changed"
for gone in "GroupA/1kb/main-old" "GroupA/1kb/my-snapshot" "GroupA/1kb/change" "GroupA/1kb/report" "GroupA/report" "GroupB" "Weird Group (Note)"; do
  [[ ! -e "$T/criterion/$gone" ]] || fail "case1: should be removed: $gone"
done
[[ -f "$T/debug/keepme.o" ]] || fail "case1: sibling of criterion touched"
[[ ! -L "$T/criterion/GroupA/1kb/snap-link" ]] || fail "case1: symlink victim survived"
printf '%s' "$out1" | grep -Eq 'kept 2 run dir\(s\), removed 10 file\(s\) \([0-9]+ KiB\)' || fail "case1: bad report line: $out1"
# GroupA/1kb itself and its ancestor chain must survive as keepers' parents:
[[ -d "$T/criterion/GroupA/1kb" && -d "$T/criterion/GroupA" ]] || fail "case1: ancestor dirs pruned too far"
ok "surgical default keeps new/+base/, removes 10 files"

# --- Case 1b: idempotent second run
out1b="$(mise run -y clean --criterion --target-dir "$T" 2>&1)" || fail "case1b: exit nonzero"
printf '%s' "$out1b" | grep -Eq 'kept 2 run dir\(s\), removed 0 file\(s\)' || fail "case1b: $out1b"
[[ -d "$T/criterion/GroupA/1kb/new" && -d "$T/criterion/GroupA/1kb/base" ]] || fail "case1b: keep dir vanished"
[[ "$before_keep" == "$(keep_manifest "$T")" ]] || fail "case1b: content drifted"
ok "second surgical run removes 0 files"

# --- Case 2: dry-run leaves tree byte-identical; numbers match the real run
T2="$SBROOT/c2"; build_fixture "$T2"
before2="$(tree_manifest "$T2")"
out2="$(mise run -y clean --criterion --dry-run --target-dir "$T2" 2>&1)" || fail "case2: exit nonzero"
[[ "$before2" == "$(tree_manifest "$T2")" ]] || fail "case2: dry-run modified tree"
printf '%s' "$out2" | grep -Eq 'would keep 2 run dir\(s\), would remove 10 file\(s\) \([0-9]+ KiB\)' || fail "case2: report line: $out2"
printf '%s' "$out2" | grep -q 'main-old' || fail "case2: dry-run must print victim paths"
if printf '%s' "$out2" | grep -Eq '/(new|base)/'; then
  fail "case2: keep dir path leaked into victim list: $out2"
fi
[[ "$(numstrip "$out1")" == "$(numstrip "$out2")" ]] || fail "case2: dry/real number mismatch ($out1 vs $out2)"
ok "dry-run is byte-identical and numerically matches the real run"

# --- Case 3: --force full wipe (rest of target/ untouched)
T3="$SBROOT/c3"; build_fixture "$T3"
out3="$(mise run -y clean --criterion --force --target-dir "$T3" 2>&1)" || fail "case3: exit nonzero"
[[ ! -d "$T3/criterion" ]] || fail "case3: force must wipe criterion"
printf '%s' "$out3" | grep -q 'removed benchmark output' || fail "case3: message: $out3"
[[ -f "$T3/debug/keepme.o" ]] || fail "case3: --criterion must not cargo-clean"
ok "--force wipes the whole criterion tree"

# --- Case 4: --force --dry-run prints full-wipe line, touches nothing
T4="$SBROOT/c4"; build_fixture "$T4"
before4="$(tree_manifest "$T4")"
out4="$(mise run -y clean --criterion --force --dry-run --target-dir "$T4" 2>&1)" || fail "case4: exit nonzero"
[[ "$before4" == "$(tree_manifest "$T4")" ]] || fail "case4: dry force modified tree"
printf '%s' "$out4" | grep -q 'would remove benchmark output' || fail "case4: message: $out4"
ok "--force --dry-run reports without deleting"

# --- Case 5: tree with zero keep dirs falls back to full wipe
T5="$SBROOT/c5"
mkdir -p "$T5/criterion/GroupC/3kb/main-x" "$T5/criterion/GroupC/3kb/report"
printf 'old\n' > "$T5/criterion/GroupC/3kb/main-x/estimates.json"
printf '<h>\n' > "$T5/criterion/GroupC/3kb/report/index.html"
out5="$(mise run -y clean --criterion --target-dir "$T5" 2>&1)" || fail "case5: exit nonzero"
[[ ! -d "$T5/criterion" ]] || fail "case5: no keep dirs => full wipe expected"
printf '%s' "$out5" | grep -q 'removed benchmark output' || fail "case5: full-wipe message: $out5"
ok "zero keep dirs falls back to full wipe"

# --- Case 6: missing criterion dir exits 0 with the historical messages
T6="$SBROOT/c6"; mkdir -p "$T6/target"
out6="$(mise run -y clean --criterion --target-dir "$T6/target" 2>&1)" || fail "case6: real mode nonzero on missing dir"
out6d="$(mise run -y clean --criterion --dry-run --target-dir "$T6/target" 2>&1)" || fail "case6: dry mode nonzero"
printf '%s' "$out6d" | grep -q 'no benchmark output at' || fail "case6: dry message: $out6d"
if printf '%s' "$out6" | grep -q 'no benchmark output'; then
  fail "case6: real mode must be silent on missing dir: $out6"
fi
ok "missing dir: exit 0, dry-run says no benchmark output"

# --- Case 7: default clean (no --criterion) never enters the criterion path
# NB: manifests the criterion subtree only — cargo clean --dry-run legitimately
# writes .rustc_info.json at the target root, which a whole-target manifest
# would flag (probed: that write is cargo's, not the criterion path's).
T7="$SBROOT/c7"; build_fixture "$T7"
before7="$(tree_manifest "$T7/criterion")"
out7="$(mise run -y clean --dry-run --target-dir "$T7" 2>&1)" || fail "case7: exit nonzero"
[[ "$before7" == "$(tree_manifest "$T7/criterion")" ]] || fail "case7: criterion tree modified by default clean"
[[ -f "$T7/debug/keepme.o" ]] || fail "case7: cargo clean deleted sibling (dry-run must not write)"
if printf '%s' "$out7" | grep -Eq 'benchmark output|run dir\(s\)'; then
  fail "case7: criterion path ran without --criterion: $out7"
fi
ok "default clean does not enter the criterion path"

printf 'all %d assertions passed\n' "${pass_n}"
