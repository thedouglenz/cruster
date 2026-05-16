#!/usr/bin/env bash
# install.sh — copy cruster's agentskills into the right place for
# your AI coding agent.
#
# Usage:
#   ./install.sh                          # default: --agent generic
#   ./install.sh --agent claude
#   ./install.sh --agent cursor
#   ./install.sh --agent generic
#   ./install.sh --agent claude --dry-run
#   ./install.sh --agent claude --force   # overwrite existing skill dirs

set -euo pipefail

AGENT="generic"
DRY_RUN=0
FORCE=0

usage() {
  cat <<'EOF'
Install cruster's agentskills.

Options:
  --agent <claude|cursor|generic>   Where to install (default: generic).
                                    claude  -> ~/.claude/skills/
                                    cursor  -> ./.cursor/skills/
                                    generic -> ~/.agentskills/
  --dry-run                          Print what would happen, do nothing.
  --force                            Replace existing skill dirs (default: skip).
  -h, --help                         This help.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --agent)   AGENT="$2"; shift 2 ;;
    --dry-run) DRY_RUN=1;  shift ;;
    --force)   FORCE=1;    shift ;;
    -h|--help) usage; exit 0 ;;
    *)         echo "unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

case "$AGENT" in
  claude)  TARGET="$HOME/.claude/skills" ;;
  cursor)  TARGET="$PWD/.cursor/skills" ;;
  generic) TARGET="$HOME/.agentskills" ;;
  *) echo "unknown agent: $AGENT (expected: claude, cursor, generic)" >&2; exit 2 ;;
esac

# Resolve repo root from the script's location (allow being invoked from anywhere).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE_DIR="$SCRIPT_DIR/skills"

if [[ ! -d "$SOURCE_DIR" ]]; then
  echo "error: source skills dir not found at $SOURCE_DIR" >&2
  exit 1
fi

echo "agent:  $AGENT"
echo "source: $SOURCE_DIR"
echo "target: $TARGET"
echo "force:  $FORCE"
echo "mode:   $([[ $DRY_RUN -eq 1 ]] && echo DRY-RUN || echo APPLY)"
echo

if [[ $DRY_RUN -eq 0 ]]; then
  mkdir -p "$TARGET"
fi

# Iterate skills/cruster-*/ — only directories.
shopt -s nullglob
for skill_dir in "$SOURCE_DIR"/cruster-*/; do
  skill_name=$(basename "$skill_dir")
  dest="$TARGET/$skill_name"

  if [[ -e "$dest" && $FORCE -eq 0 ]]; then
    echo "skip $skill_name (already at $dest; --force to replace)"
    continue
  fi

  if [[ $DRY_RUN -eq 1 ]]; then
    echo "would install $skill_name -> $dest"
  else
    rm -rf "$dest"
    cp -R "$skill_dir" "$dest"
    echo "installed $skill_name -> $dest"
  fi
done

echo
echo "done."
