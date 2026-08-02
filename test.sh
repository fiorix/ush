#!/bin/bash
set -euo pipefail

# End-to-end tests for ush.
# Usage: ./test.sh

USH=./target/debug/ush
PASS=0
FAIL=0
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

# ── Assertions ────────────────────────────────────────────────────────

assert_eq() {
    local got="$1" want="$2"
    if [ "$got" = "$want" ]; then
        return 0
    fi
    printf "    assert_eq failed:\n      got:  %s\n      want: %s\n" "$got" "$want" >&2
    return 1
}

assert_contains() {
    local haystack="$1" needle="$2"
    if printf '%s' "$haystack" | grep -qF "$needle"; then
        return 0
    fi
    printf "    assert_contains failed:\n      needle: %s\n      not in: %s\n" "$needle" "$haystack" >&2
    return 1
}

assert_line_count() {
    local output="$1" want="$2"
    local got
    got=$(printf '%s\n' "$output" | grep -c .)
    if [ "$got" -eq "$want" ]; then
        return 0
    fi
    printf "    assert_line_count failed: got %s, want %s\n" "$got" "$want" >&2
    return 1
}

# ── Test runner ───────────────────────────────────────────────────────

run_test() {
    local name="$1"
    shift
    if "$@" 2>/dev/null; then
        PASS=$((PASS + 1))
        printf "  PASS  %s\n" "$name"
    else
        FAIL=$((FAIL + 1))
        printf "  FAIL  %s\n" "$name" >&2
    fi
}

# ── Tests ─────────────────────────────────────────────────────────────

test_basic_exec() {
    local out
    out=$(printf 'hello\nworld\n' | "$USH" exec --batch -- echo {})
    assert_line_count "$out" 2
    assert_contains "$out" '"target":"hello"'
    assert_contains "$out" '"target":"world"'
    assert_contains "$out" '"exit_status":0'
}

test_parallel_exec() {
    local out
    out=$(seq 1 10 | "$USH" exec --batch -p 4 -- echo {})
    assert_line_count "$out" 10
}

test_timeout_sigterm() {
    local out
    out=$(printf 'test\n' | "$USH" exec --batch -t 2s -- sleep 10)
    assert_contains "$out" '"signal: terminated"'
    assert_contains "$out" '"exit_status":-1'
}

test_exit_code() {
    local out
    out=$(printf 'test\n' | "$USH" exec --batch -- false)
    assert_contains "$out" '"exit_status":'
    # false exits with 1
    assert_contains "$out" '"error":"exit status 1"'
}

test_stdout_head() {
    local out
    out=$(printf 'test\n' | "$USH" exec --batch --stdout_bytes=5 --head -- echo hello_world)
    assert_contains "$out" '"stdout":"hello[...]"'
}

test_stdout_tail() {
    local out
    out=$(printf 'test\n' | "$USH" exec --batch --stdout_bytes=5 -- echo hello_world)
    assert_contains "$out" '"stdout":"[...]orld'
}

test_streaming_chunks() {
    local out
    out=$(printf 't\n' | "$USH" exec --chunk_size=4 --stdout_bytes=16 --head -- sh -c 'printf "%0.sx" $(seq 1 20)')
    local chunk_count
    chunk_count=$(printf '%s\n' "$out" | grep -c '"type":"stdout_chunk"')
    assert_eq "$chunk_count" "4"
    assert_contains "$out" '"stdout_truncated":true'
}

test_exclude_file() {
    printf 'skip_me\n' > "$TMPDIR/exclude.txt"
    local out
    out=$(printf 'keep\nskip_me\n' | "$USH" exec -e "$TMPDIR/exclude.txt" -- echo {})
    assert_line_count "$out" 1
    assert_contains "$out" '"target":"keep"'
}

test_comments_skipped() {
    local out
    out=$(printf '# this is a comment\nreal_target\n' | "$USH" exec -- echo {})
    assert_line_count "$out" 1
    assert_contains "$out" '"target":"real_target"'
}

test_empty_input() {
    local out
    out=$(printf '' | "$USH" exec -- echo {})
    assert_eq "$out" ""
}

test_freq_stdout() {
    local out
    out=$(printf 'a\nb\nc\n' | "$USH" exec -- echo hello | "$USH" freq stdout)
    assert_contains "$out" "100.00"
    assert_contains "$out" "hello"
}

test_freq_exitstatus() {
    local out
    out=$(printf 'a\nb\n' | "$USH" exec -- true | "$USH" freq exitstatus)
    assert_contains "$out" "100.00"
}

test_freq_json() {
    local out
    out=$(printf 'a\nb\n' | "$USH" exec -- echo hi | "$USH" freq stdout --json)
    assert_contains "$out" '"freq":'
    assert_contains "$out" '"targets":'
}

test_verbose_flag() {
    local stderr_out
    stderr_out=$(printf 'test\n' | "$USH" -v exec -- echo {} 2>&1 1>/dev/null)
    assert_contains "$stderr_out" "[verbose]"
}

test_version_flag() {
    local out
    out=$("$USH" --version)
    assert_contains "$out" "ush v"
}

test_missing_command() {
    if "$USH" exec 2>/dev/null; then
        printf "    expected non-zero exit\n" >&2
        return 1
    fi
    return 0
}

# ── Main ──────────────────────────────────────────────────────────────

printf "Building ush...\n"
cargo build --quiet 2>&1

printf "\nRunning end-to-end tests:\n"
run_test "basic_exec"        test_basic_exec
run_test "parallel_exec"     test_parallel_exec
run_test "timeout_sigterm"   test_timeout_sigterm
run_test "exit_code"         test_exit_code
run_test "stdout_head"       test_stdout_head
run_test "stdout_tail"       test_stdout_tail
run_test "streaming_chunks"  test_streaming_chunks
run_test "exclude_file"      test_exclude_file
run_test "comments_skipped"  test_comments_skipped
run_test "empty_input"       test_empty_input
run_test "freq_stdout"       test_freq_stdout
run_test "freq_exitstatus"   test_freq_exitstatus
run_test "freq_json"         test_freq_json
run_test "verbose_flag"      test_verbose_flag
run_test "version_flag"      test_version_flag
run_test "missing_command"   test_missing_command

printf "\nResults: %d passed, %d failed\n" "$PASS" "$FAIL"
if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
