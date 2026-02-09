# A3S Search - Justfile

default:
    @just --list

# AI-powered commit message
cz:
    @bash .scripts/generate-commit-message.sh

# ============================================================================
# Build
# ============================================================================

# Build the project
build:
    cargo build -p a3s-search

# Build with headless browser support
build-headless:
    cargo build -p a3s-search --features headless

# Build release
release:
    cargo build -p a3s-search --release

# ============================================================================
# Test (unified command with progress display)
# ============================================================================

# Run all tests with progress display and module breakdown
test:
    #!/usr/bin/env bash
    set -e

    # Colors
    BOLD='\033[1m'
    GREEN='\033[0;32m'
    BLUE='\033[0;34m'
    CYAN='\033[0;36m'
    YELLOW='\033[0;33m'
    RED='\033[0;31m'
    DIM='\033[2m'
    RESET='\033[0m'

    # Counters
    TOTAL_PASSED=0
    TOTAL_FAILED=0
    TOTAL_IGNORED=0

    print_header() {
        echo ""
        echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
        echo -e "${BOLD}  $1${RESET}"
        echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
    }

    # Extract module test counts from cargo test output
    extract_module_counts() {
        local output="$1"
        echo "$output" | grep -E "^test .+::.+ \.\.\. ok$" | \
            sed 's/^test \([^:]*\)::.*/\1/' | \
            sort | uniq -c | sort -rn | \
            while read count module; do
                printf "      ${DIM}%-20s %3d tests${RESET}\n" "$module" "$count"
            done
    }

    print_header "🧪 A3S Search Test Suite"
    echo ""
    echo -ne "${CYAN}▶${RESET} ${BOLD}a3s-search${RESET} "

    # Run tests and capture output
    if OUTPUT=$(cargo test -p a3s-search --lib 2>&1); then
        TEST_EXIT=0
    else
        TEST_EXIT=1
    fi

    # Extract test results
    RESULT_LINE=$(echo "$OUTPUT" | grep -E "^test result:" | tail -1)
    if [ -n "$RESULT_LINE" ]; then
        PASSED=$(echo "$RESULT_LINE" | grep -oE '[0-9]+ passed' | grep -oE '[0-9]+' || echo "0")
        FAILED=$(echo "$RESULT_LINE" | grep -oE '[0-9]+ failed' | grep -oE '[0-9]+' || echo "0")
        IGNORED=$(echo "$RESULT_LINE" | grep -oE '[0-9]+ ignored' | grep -oE '[0-9]+' || echo "0")

        TOTAL_PASSED=$((TOTAL_PASSED + PASSED))
        TOTAL_FAILED=$((TOTAL_FAILED + FAILED))
        TOTAL_IGNORED=$((TOTAL_IGNORED + IGNORED))

        if [ "$FAILED" -gt 0 ]; then
            echo -e "${RED}✗${RESET} ${DIM}$PASSED passed, $FAILED failed${RESET}"
            echo "$OUTPUT" | grep -E "^test .* FAILED$" | sed 's/^/    /'
        else
            echo -e "${GREEN}✓${RESET} ${DIM}$PASSED passed${RESET}"
            # Show module breakdown for crates with many tests
            if [ "$PASSED" -gt 10 ]; then
                extract_module_counts "$OUTPUT"
            fi
        fi
    else
        # No tests found or compilation error
        if echo "$OUTPUT" | grep -q "error\[E"; then
            echo -e "${RED}✗${RESET} ${DIM}compile error${RESET}"
            echo "$OUTPUT" | grep -E "^error" | head -3 | sed 's/^/    /'
        elif [ "$TEST_EXIT" -ne 0 ]; then
            echo -e "${RED}✗${RESET} ${DIM}failed${RESET}"
        else
            echo -e "${YELLOW}○${RESET} ${DIM}no tests${RESET}"
        fi
    fi

    # Summary
    echo ""
    echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"

    if [ "$TOTAL_FAILED" -gt 0 ]; then
        echo -e "  ${RED}${BOLD}✗ FAILED${RESET}  ${GREEN}$TOTAL_PASSED passed${RESET}  ${RED}$TOTAL_FAILED failed${RESET}  ${YELLOW}$TOTAL_IGNORED ignored${RESET}"
        echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
        exit 1
    else
        echo -e "  ${GREEN}${BOLD}✓ PASSED${RESET}  ${GREEN}$TOTAL_PASSED passed${RESET}  ${YELLOW}$TOTAL_IGNORED ignored${RESET}"
        echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
    fi
    echo ""

# Run tests with headless feature
test-headless:
    #!/usr/bin/env bash
    set -e

    # Colors
    BOLD='\033[1m'
    GREEN='\033[0;32m'
    BLUE='\033[0;34m'
    CYAN='\033[0;36m'
    YELLOW='\033[0;33m'
    RED='\033[0;31m'
    DIM='\033[2m'
    RESET='\033[0m'

    TOTAL_PASSED=0
    TOTAL_FAILED=0
    TOTAL_IGNORED=0

    print_header() {
        echo ""
        echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
        echo -e "${BOLD}  $1${RESET}"
        echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
    }

    extract_module_counts() {
        local output="$1"
        echo "$output" | grep -E "^test .+::.+ \.\.\. ok$" | \
            sed 's/^test \([^:]*\)::.*/\1/' | \
            sort | uniq -c | sort -rn | \
            while read count module; do
                printf "      ${DIM}%-20s %3d tests${RESET}\n" "$module" "$count"
            done
    }

    print_header "🧪 A3S Search Test Suite (headless)"
    echo ""
    echo -ne "${CYAN}▶${RESET} ${BOLD}a3s-search${RESET} ${DIM}(--features headless)${RESET} "

    if OUTPUT=$(cargo test -p a3s-search --features headless --lib 2>&1); then
        TEST_EXIT=0
    else
        TEST_EXIT=1
    fi

    RESULT_LINE=$(echo "$OUTPUT" | grep -E "^test result:" | tail -1)
    if [ -n "$RESULT_LINE" ]; then
        PASSED=$(echo "$RESULT_LINE" | grep -oE '[0-9]+ passed' | grep -oE '[0-9]+' || echo "0")
        FAILED=$(echo "$RESULT_LINE" | grep -oE '[0-9]+ failed' | grep -oE '[0-9]+' || echo "0")
        IGNORED=$(echo "$RESULT_LINE" | grep -oE '[0-9]+ ignored' | grep -oE '[0-9]+' || echo "0")

        TOTAL_PASSED=$((TOTAL_PASSED + PASSED))
        TOTAL_FAILED=$((TOTAL_FAILED + FAILED))
        TOTAL_IGNORED=$((TOTAL_IGNORED + IGNORED))

        if [ "$FAILED" -gt 0 ]; then
            echo -e "${RED}✗${RESET} ${DIM}$PASSED passed, $FAILED failed${RESET}"
        else
            echo -e "${GREEN}✓${RESET} ${DIM}$PASSED passed${RESET}"
            if [ "$PASSED" -gt 10 ]; then
                extract_module_counts "$OUTPUT"
            fi
        fi
    else
        if echo "$OUTPUT" | grep -q "error\[E"; then
            echo -e "${RED}✗${RESET} ${DIM}compile error${RESET}"
        elif [ "$TEST_EXIT" -ne 0 ]; then
            echo -e "${RED}✗${RESET} ${DIM}failed${RESET}"
        else
            echo -e "${YELLOW}○${RESET} ${DIM}no tests${RESET}"
        fi
    fi

    echo ""
    echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
    if [ "$TOTAL_FAILED" -gt 0 ]; then
        echo -e "  ${RED}${BOLD}✗ FAILED${RESET}  ${GREEN}$TOTAL_PASSED passed${RESET}  ${RED}$TOTAL_FAILED failed${RESET}  ${YELLOW}$TOTAL_IGNORED ignored${RESET}"
    else
        echo -e "  ${GREEN}${BOLD}✓ PASSED${RESET}  ${GREEN}$TOTAL_PASSED passed${RESET}  ${YELLOW}$TOTAL_IGNORED ignored${RESET}"
    fi
    echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
    echo ""

# Run tests without progress (raw cargo output)
test-raw:
    cargo test -p a3s-search --lib

# Run tests with verbose output
test-v:
    cargo test -p a3s-search --lib -- --nocapture

# Run specific test
test-one TEST:
    cargo test -p a3s-search {{TEST}} -- --nocapture

# ============================================================================
# Test Subsets
# ============================================================================

# Test engine module
test-engine:
    cargo test -p a3s-search --lib -- engine::tests

# Test query module
test-query:
    cargo test -p a3s-search --lib -- query::tests

# Test result module
test-result:
    cargo test -p a3s-search --lib -- result::tests

# Test search module
test-search:
    cargo test -p a3s-search --lib -- search::tests

# Test aggregator module
test-aggregator:
    cargo test -p a3s-search --lib -- aggregator::tests

# Test proxy module
test-proxy:
    cargo test -p a3s-search --lib -- proxy::tests

# Test error module
test-error:
    cargo test -p a3s-search --lib -- error::tests

# ============================================================================
# Coverage (requires: cargo install cargo-llvm-cov)
# ============================================================================

# Test with coverage - shows real-time test progress + module coverage
test-cov:
    #!/usr/bin/env bash
    set -e

    # Colors
    BOLD='\033[1m'
    GREEN='\033[0;32m'
    BLUE='\033[0;34m'
    CYAN='\033[0;36m'
    YELLOW='\033[0;33m'
    RED='\033[0;31m'
    DIM='\033[2m'
    RESET='\033[0m'

    # Clear line and move cursor
    CLEAR_LINE='\033[2K'

    # Resolve LLVM tools
    SYSROOT="$(rustup run stable rustc --print sysroot)"
    HOST=$(rustc -vV | grep host | cut -d' ' -f2)
    export LLVM_COV="$SYSROOT/lib/rustlib/$HOST/bin/llvm-cov"
    export LLVM_PROFDATA="$SYSROOT/lib/rustlib/$HOST/bin/llvm-profdata"

    print_header() {
        echo ""
        echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
        echo -e "${BOLD}  $1${RESET}"
        echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
    }

    print_header "🧪 A3S Search Test Suite with Coverage"
    echo ""
    echo -e "${CYAN}▶${RESET} ${BOLD}a3s-search${RESET}"
    echo ""

    # Temp files for tracking
    tmp_dir="/tmp/test_cov_search_$$"
    mkdir -p "$tmp_dir"
    touch "$tmp_dir/module_counts"

    # Run tests with coverage
    {
        cargo llvm-cov -p a3s-search --lib 2>&1
    } | {
        total_passed=0
        total_failed=0

        while IFS= read -r line; do
            # Check if it's a test result line
            if [[ "$line" =~ ^test\ ([a-z_]+)::.*\.\.\.\ (ok|FAILED)$ ]]; then
                module="${BASH_REMATCH[1]}"
                result="${BASH_REMATCH[2]}"

                if [ "$result" = "ok" ]; then
                    total_passed=$((total_passed + 1))
                    count=$(grep "^${module} " "$tmp_dir/module_counts" 2>/dev/null | awk '{print $2}' || echo "0")
                    count=$((count + 1))
                    grep -v "^${module} " "$tmp_dir/module_counts" > "$tmp_dir/module_counts.tmp" 2>/dev/null || true
                    echo "$module $count" >> "$tmp_dir/module_counts.tmp"
                    mv "$tmp_dir/module_counts.tmp" "$tmp_dir/module_counts"
                else
                    total_failed=$((total_failed + 1))
                fi

                echo -ne "\r${CLEAR_LINE}      ${DIM}Running:${RESET} ${module}::... ${GREEN}${total_passed}${RESET} passed"
                [ "$total_failed" -gt 0 ] && echo -ne " ${RED}${total_failed}${RESET} failed"

            elif [[ "$line" =~ ^[[:space:]]*Compiling ]]; then
                echo -ne "\r${CLEAR_LINE}      ${DIM}Compiling...${RESET}"
            elif [[ "$line" =~ ^[[:space:]]*Running ]]; then
                echo -ne "\r${CLEAR_LINE}      ${DIM}Running tests...${RESET}"
            elif [[ "$line" =~ ^[a-z_]+.*\.rs[[:space:]] ]]; then
                echo "$line" >> "$tmp_dir/coverage_lines"
            elif [[ "$line" =~ ^TOTAL ]]; then
                echo "$line" >> "$tmp_dir/total_line"
            fi
        done

        echo "$total_passed" > "$tmp_dir/total_passed"
        echo "$total_failed" > "$tmp_dir/total_failed"
    }

    # Clear progress line
    echo -ne "\r${CLEAR_LINE}"

    # Read results
    total_passed=$(cat "$tmp_dir/total_passed" 2>/dev/null || echo "0")
    total_failed=$(cat "$tmp_dir/total_failed" 2>/dev/null || echo "0")

    # Show final test result
    if [ "$total_failed" -gt 0 ]; then
        echo -e "      ${RED}✗${RESET} ${total_passed} passed, ${RED}${total_failed} failed${RESET}"
    else
        echo -e "      ${GREEN}✓${RESET} ${total_passed} tests passed"
    fi
    echo ""

    # Parse coverage data and aggregate by module
    if [ -f "$tmp_dir/coverage_lines" ]; then
        awk '
        {
            file=$1; lines=$8; missed=$9
            n = split(file, parts, "/")
            if (n > 1) {
                module = parts[1]
            } else {
                gsub(/\.rs$/, "", file)
                module = file
            }
            total_lines[module] += lines
            total_missed[module] += missed
        }
        END {
            for (m in total_lines) {
                if (total_lines[m] > 0) {
                    covered = total_lines[m] - total_missed[m]
                    pct = (covered / total_lines[m]) * 100
                    printf "%s %.1f %d\n", m, pct, total_lines[m]
                }
            }
        }' "$tmp_dir/coverage_lines" | sort -t' ' -k2 -rn > "$tmp_dir/cov_agg"

        # Display coverage results with test counts
        echo -e "      ${BOLD}Module               Tests   Coverage${RESET}"
        echo -e "      ${DIM}──────────────────────────────────────────────${RESET}"

        while read module pct lines; do
            tests=$(grep "^${module} " "$tmp_dir/module_counts" 2>/dev/null | awk '{print $2}' || echo "0")
            [ -z "$tests" ] && tests=0

            num=${pct%.*}
            if [ "$num" -ge 90 ]; then
                cov_color="${GREEN}${pct}%${RESET}"
            elif [ "$num" -ge 70 ]; then
                cov_color="${YELLOW}${pct}%${RESET}"
            else
                cov_color="${RED}${pct}%${RESET}"
            fi
            echo -e "      $(printf '%-18s' "$module") $(printf '%4d' "$tests")   ${cov_color} ${DIM}($lines lines)${RESET}"
        done < "$tmp_dir/cov_agg"

        # Print total
        if [ -f "$tmp_dir/total_line" ]; then
            total_cov=$(cat "$tmp_dir/total_line" | awk '{print $4}' | tr -d '%')
            total_lines=$(cat "$tmp_dir/total_line" | awk '{print $8}')
            echo -e "      ${DIM}──────────────────────────────────────────────${RESET}"

            num=${total_cov%.*}
            if [ "$num" -ge 90 ]; then
                cov_color="${GREEN}${BOLD}${total_cov}%${RESET}"
            elif [ "$num" -ge 70 ]; then
                cov_color="${YELLOW}${BOLD}${total_cov}%${RESET}"
            else
                cov_color="${RED}${BOLD}${total_cov}%${RESET}"
            fi
            echo -e "      ${BOLD}$(printf '%-18s' "TOTAL") $(printf '%4d' "$total_passed")${RESET}   ${cov_color} ${DIM}($total_lines lines)${RESET}"
        fi
    fi

    # Cleanup
    rm -rf "$tmp_dir"
    echo ""
    echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
    echo ""

# Test with coverage (headless feature)
test-cov-headless:
    #!/usr/bin/env bash
    set -e
    SYSROOT="$(rustup run stable rustc --print sysroot)"
    HOST=$(rustc -vV | grep host | cut -d' ' -f2)
    export LLVM_COV="$SYSROOT/lib/rustlib/$HOST/bin/llvm-cov"
    export LLVM_PROFDATA="$SYSROOT/lib/rustlib/$HOST/bin/llvm-profdata"
    cargo llvm-cov -p a3s-search --features headless --lib --summary-only

# Coverage with pretty terminal output
cov:
    #!/usr/bin/env bash
    set -e
    SYSROOT="$(rustup run stable rustc --print sysroot)"
    HOST=$(rustc -vV | grep host | cut -d' ' -f2)
    export LLVM_COV="$SYSROOT/lib/rustlib/$HOST/bin/llvm-cov"
    export LLVM_PROFDATA="$SYSROOT/lib/rustlib/$HOST/bin/llvm-profdata"
    COV_FILE="/tmp/a3s-search-coverage.lcov"
    echo "┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓"
    echo "┃                    🧪 Running Tests with Coverage                     ┃"
    echo "┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛"
    cargo llvm-cov -p a3s-search --lib --lcov --output-path "$COV_FILE" 2>&1 | grep -E "^test result"
    echo ""
    echo "┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓"
    echo "┃                         📊 Coverage Report                            ┃"
    echo "┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛"
    lcov --summary "$COV_FILE" 2>&1
    rm -f "$COV_FILE"

# Coverage for specific module
cov-module MOD:
    #!/usr/bin/env bash
    SYSROOT="$(rustup run stable rustc --print sysroot)"
    HOST=$(rustc -vV | grep host | cut -d' ' -f2)
    export LLVM_COV="$SYSROOT/lib/rustlib/$HOST/bin/llvm-cov"
    export LLVM_PROFDATA="$SYSROOT/lib/rustlib/$HOST/bin/llvm-profdata"
    cargo llvm-cov -p a3s-search --lib -- {{MOD}}::

# Coverage with HTML report (opens in browser)
cov-html:
    #!/usr/bin/env bash
    SYSROOT="$(rustup run stable rustc --print sysroot)"
    HOST=$(rustc -vV | grep host | cut -d' ' -f2)
    export LLVM_COV="$SYSROOT/lib/rustlib/$HOST/bin/llvm-cov"
    export LLVM_PROFDATA="$SYSROOT/lib/rustlib/$HOST/bin/llvm-profdata"
    cargo llvm-cov -p a3s-search --lib --html --open

# Coverage with detailed file-by-file table
cov-table:
    #!/usr/bin/env bash
    SYSROOT="$(rustup run stable rustc --print sysroot)"
    HOST=$(rustc -vV | grep host | cut -d' ' -f2)
    export LLVM_COV="$SYSROOT/lib/rustlib/$HOST/bin/llvm-cov"
    export LLVM_PROFDATA="$SYSROOT/lib/rustlib/$HOST/bin/llvm-profdata"
    cargo llvm-cov -p a3s-search --lib

# Coverage for CI (generates lcov.info)
cov-ci:
    #!/usr/bin/env bash
    SYSROOT="$(rustup run stable rustc --print sysroot)"
    HOST=$(rustc -vV | grep host | cut -d' ' -f2)
    export LLVM_COV="$SYSROOT/lib/rustlib/$HOST/bin/llvm-cov"
    export LLVM_PROFDATA="$SYSROOT/lib/rustlib/$HOST/bin/llvm-profdata"
    cargo llvm-cov -p a3s-search --lib --lcov --output-path lcov.info

# ============================================================================
# Code Quality
# ============================================================================

# Format code
fmt:
    cargo fmt -p a3s-search

# Check formatting
fmt-check:
    cargo fmt -p a3s-search -- --check

# Lint (clippy)
lint:
    cargo clippy -p a3s-search --all-targets -- -D warnings

# CI checks (fmt + lint + test)
ci:
    cargo fmt -p a3s-search -- --check
    cargo clippy -p a3s-search --all-targets -- -D warnings
    cargo test -p a3s-search --lib

# ============================================================================
# Utilities
# ============================================================================

# Clean build artifacts
clean:
    cargo clean

# Check project (fast compile check)
check:
    cargo check -p a3s-search

# Watch and rebuild
watch:
    cargo watch -x "build -p a3s-search"

# Generate docs
doc:
    cargo doc -p a3s-search --no-deps --open

# Update dependencies
update:
    cargo update

# Show current version
version:
    @grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/'
