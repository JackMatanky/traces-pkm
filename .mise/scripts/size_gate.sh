#!/bin/bash
# Helpers for the size thresholds used by the `clean` and `sweep` tasks:
# validate a size flag, measure a directory in KiB, and convert GiB to KiB.
#
# Not a task: mise only discovers tasks under `.mise/tasks/`. Task `run` bodies
# `source` this file via `MISE_PROJECT_ROOT` so the path does not depend on the
# current directory.

#######################################
# Validate a size flag value as a non-negative decimal integer.
#
# The check is textual: leading zeros such as `"08"` pass, and `gib_to_kib`
# later reads them as base 10.
#
# Globals:
#   None
# Arguments:
#   `task`  Calling task's name, prefixed to the error message.
#   `flag`  Flag being validated, named in the error message.
#   `val`   Value to check.
# Outputs:
#   On failure, one error line on `STDERR`.
# Returns:
#   `0` when `val` is all digits; otherwise exits `2`, ending calling task.
#######################################
require_nonneg_int() {
  local task="${1}" flag="${2}" val="${3}"
  if [[ ! "${val}" =~ ^[0-9]+$ ]]; then
    printf "%s: %s must be a non-negative integer, got '%s'\n" \
      "${task}" "${flag}" "${val}" >&2
    exit 2
  fi
}

#######################################
# Measure a directory's disk usage.
#
# Globals:
#   None
# Arguments:
#   `dir`  Path of the directory to measure.
# Outputs:
#   Writes `dir`'s disk usage in KiB (1024-byte blocks) to `stdout`.
# Returns:
#   Status of the `du`/`cut` pipeline: if `dir` is missing, `stdout` is empty
#   and status is nonzero only when caller set `pipefail`.
#######################################
dir_size_kib() {
  du -sk "${1}" | cut -f1
}

#######################################
# Convert a whole number of GiB to KiB (multiply by 1024*1024).
#
# The `10#` prefix forces base-10 parsing: without it a leading zero triggers
# octal parsing, and digits 8-9 make the expansion fail.
#
# Globals:
#   None
# Arguments:
#   `gib`  Integer GiB value; pre-validated with `require_nonneg_int`.
# Outputs:
#   Writes the equivalent size in KiB to `stdout`.
# Returns:
#   `0` on success; a non-base-10 value makes arithmetic fail with nonzero
#   status.
gib_to_kib() {
  printf '%s' "$((10#${1} * 1024 * 1024))"
}

#######################################
# Format a size in KiB as a human-readable GiB float string.
# Arguments:
#   kib_val: Non-negative integer size in KiB.
# Outputs:
#   Writes formatted float with 1 decimal place (e.g. "4.2") to STDOUT.
#######################################
format_kib_to_gib() {
  awk "BEGIN { printf \"%.1f\", ${1} / 1024 / 1024 }"
}

#######################################
# Evaluate whether a directory's size exceeds a GiB threshold.
#
# Validates inputs, measures directory usage, and outputs a diagnostic message
# if size is at or below the threshold.
#
# Arguments:
#   task_label: Calling task's name for diagnostic logging.
#   dir_path: Path of the directory to measure.
#   threshold_gib: Threshold integer GiB value to check against.
# Outputs:
#   Diagnostic log on STDOUT when directory does not exist or size is below
#   threshold.
# Returns:
#   0 if directory exists and size strictly exceeds threshold.
#   1 if directory does not exist or size is at or below threshold.
#   Exits 2 if threshold_gib is not a non-negative integer.
#######################################
eval_dir_size_gate() {
  local task_label="${1}" dir_path="${2}" threshold_gib="${3}"
  require_nonneg_int "${task_label}" "--threshold-gib" "${threshold_gib}"
  if [[ ! -d "${dir_path}" ]]; then
    printf '%s: directory %s does not exist, skipping\n' \
      "${task_label}" "${dir_path}"
    return 1
  fi
  local dir_kib limit_kib dir_gib
  dir_kib="$(dir_size_kib "${dir_path}")"
  limit_kib="$(gib_to_kib "${threshold_gib}")"
  if (( dir_kib <= limit_kib )); then
    dir_gib="$(format_kib_to_gib "${dir_kib}")"
    printf '%s: %s is %s GiB; at or below %s GiB threshold, skipping\n' \
      "${task_label}" "${dir_path}" "${dir_gib}" "${threshold_gib}"
    return 1
  fi
  return 0
}
