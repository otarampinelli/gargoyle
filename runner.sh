#!/bin/bash
set -e

GARGOYLE_TMP=$(mktemp -d)
trap "rm -rf $GARGOYLE_TMP" EXIT

if git diff main...HEAD > /dev/null 2>&1; then
  git diff main...HEAD | head -3000 > "$GARGOYLE_TMP/diff.patch"
  git log main..HEAD --oneline > "$GARGOYLE_TMP/log.txt"
elif git rev-parse HEAD > /dev/null 2>&1; then
  git diff HEAD | head -3000 > "$GARGOYLE_TMP/diff.patch"
  git log HEAD --oneline > "$GARGOYLE_TMP/log.txt"
else
  git diff | head -3000 > "$GARGOYLE_TMP/diff.patch"
  git log --oneline | head -10 > "$GARGOYLE_TMP/log.txt"
fi

if [ ! -s "$GARGOYLE_TMP/diff.patch" ]; then
  echo "No changes to review."
  exit 0
fi

if [ ! -d ".gargoyle/checks" ] || [ -z "$(find .gargoyle/checks -name '*.md' -type f)" ]; then
  echo "No checks configured in .gargoyle/checks/."
  exit 0
fi

find .gargoyle/checks -name '*.md' -type f | sort | while read check_file; do
  name=$(sed -n '/^name: /s/^name: //p' "$check_file" | head -1)
  description=$(sed -n '/^description: /s/^description: //p' "$check_file" | head -1)
  path=$(realpath "$check_file")
  echo "$path|$name|$description"
done > "$GARGOYLE_TMP/checks-meta.txt"

echo "✅ Temp dir: $GARGOYLE_TMP"
echo "✅ Checks found: $(wc -l < "$GARGOYLE_TMP/checks-meta.txt")"
echo ""
echo "Pass to skill:"
echo "  export GARGOYLE_TMP=$GARGOYLE_TMP"
echo "  export GARGOYLE_DIFF=$GARGOYLE_TMP/diff.patch"
echo "  export GARGOYLE_LOG=$GARGOYLE_TMP/log.txt"
echo "  export GARGOYLE_CHECKS_META=$GARGOYLE_TMP/checks-meta.txt"
