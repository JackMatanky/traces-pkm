#!/bin/bash
# Shared size-gate helpers for the mise clean/sweep tasks.
# Not a mise task: lives outside .mise/tasks/ and declares no MISE header, so
# mise never discovers it. Sourced from task bodies via MISE_PROJECT_ROOT
# (tasks run with the invocation cwd, not the config root — see
# docs/refs/mise_tasks/01_overview.md).

# require_nonneg_int <task> <flag> <value>
# Validate <value> as a non-negative decimal integer; exit 2 with the task's
# historical message shape otherwise. `08` etc. are accepted (validation is
# textual); base-10 interpretation happens in gib_to_kib.
require_nonneg_int() {
    local task="${1}" flag="${2}" val="${3}"
    if [[ ! "${val}" =~ ^[0-9]+$ ]]; then
        printf "%s: %s must be a non-negative integer, got '%s'\n" \
            "${task}" "${flag}" "${val}" >&2
        exit 2
    fi
}

# dir_size_kib <dir> — print the directory's size in KiB.
dir_size_kib() {
    du -sk "${1}" | cut -f1
}

# gib_to_kib <gib> — print the size in KiB. 10# forces base 10; a leading
# zero would otherwise make bash read the value as octal.
gib_to_kib() {
    printf '%s' "$((10#${1} * 1024 * 1024))"
}
