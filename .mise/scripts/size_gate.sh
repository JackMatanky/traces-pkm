#!/bin/bash
# Helpers for the size thresholds used by the clean and sweep tasks: validate a
# size flag, measure a directory in KiB, and convert GiB to KiB.
#
# Not a task: mise only discovers tasks under .mise/tasks/. Task run bodies
# `source` this file via MISE_PROJECT_ROOT so the path does not depend on the
# current directory.

#######################################
# Validate a size flag value as a non-negative decimal integer.
#
# The check is textual: leading zeros such as "08" pass, and gib_to_kib later
# reads them as base 10.
#
# Globals:
#   None
# Arguments:
#   task  Calling task's name, prefixed to the error message.
#   flag  Flag being validated, named in the error message.
#   val   Value to check.
# Outputs:
#   On failure, one error line on STDERR.
# Returns:
#   0 when val is all digits; otherwise exits 2, ending the calling task.
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
#   dir  Path of the directory to measure.
# Outputs:
#   Writes dir's disk usage in KiB (1024-byte blocks) to stdout.
# Returns:
#   Status of the `du`/`cut` pipeline: if dir is missing, stdout is empty and the
#   status is nonzero only when the caller set pipefail.
#######################################
dir_size_kib() {
    du -sk "${1}" | cut -f1
}

#######################################
# Convert a whole number of GiB to KiB (multiply by 1024*1024).
#
# The 10# prefix forces base-10 parsing: without it a leading zero triggers
# octal parsing, and digits 8-9 make the expansion fail.
#
# Globals:
#   None
# Arguments:
#   gib  Integer GiB value; callers pre-validate it with require_nonneg_int.
# Outputs:
#   Writes the equivalent size in KiB to stdout.
# Returns:
#   0 on success; a non-base-10 value makes the arithmetic fail with a nonzero
#   status.
#######################################
gib_to_kib() {
    printf '%s' "$((10#${1} * 1024 * 1024))"
}
