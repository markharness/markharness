#!/bin/sh
set -eu

repo_root=$(git rev-parse --show-toplevel)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT

git -C "$fixture" init -q
git -C "$fixture" config user.email test@example.com
git -C "$fixture" config user.name Test
git -C "$fixture" checkout -qb main
cp "$repo_root/.githooks/pre-commit" "$fixture/.git/hooks/pre-commit"

if git -C "$fixture" hook run --ignore-missing pre-commit >/dev/null 2>&1; then
  echo "expected pre-commit to reject main" >&2
  exit 1
fi

git -C "$fixture" checkout -qb feature/test
git -C "$fixture" hook run pre-commit
