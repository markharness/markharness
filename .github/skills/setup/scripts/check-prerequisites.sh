#!/usr/bin/env bash
# Setup — Prerequisite Check (macOS / Linux)
# This script checks for required development tools and reports their status.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

all_good=true

check_tool() {
    local name="$1"
    local cmd="$2"
    local min_major="$3"

    if command -v "$cmd" &> /dev/null; then
        version_output=$("$cmd" --version 2>&1 || true)
        version=$(echo "$version_output" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)
        if [ -n "$version" ]; then
            major=$(echo "$version" | cut -d. -f1)
            if [ "$major" -ge "$min_major" ]; then
                echo -e "  ${GREEN}✅ $name v$version${NC}"
            else
                echo -e "  ${YELLOW}⚠️  $name v$version (minimum v${min_major}+ required)${NC}"
                all_good=false
            fi
        else
            echo -e "  ${YELLOW}⚠️  $name (installed, version unknown)${NC}"
        fi
    else
        echo -e "  ${RED}❌ $name — not found${NC}"
        all_good=false
    fi
}

echo ""
echo "Checking prerequisites..."
echo ""

check_tool "Rust (rustc)"     "rustc" 1
check_tool "cargo"            "cargo" 1
check_tool "Git"              "git"  2
check_tool "GitHub CLI (gh)"  "gh"   2

echo ""
if [ "$all_good" = true ]; then
    echo -e "${GREEN}All prerequisites are installed.${NC}"
else
    echo -e "${YELLOW}Some prerequisites are missing or outdated. See above.${NC}"
fi
