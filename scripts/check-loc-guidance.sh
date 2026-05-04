#!/usr/bin/env bash
set -euo pipefail

soft_limit=300
hard_limit=500

usage() {
  cat <<'EOF'
Usage: scripts/check-loc-guidance.sh [--all]

Reports tracked hand-authored text files that exceed the repository LOC
guidance from AGENTS.md:
  - >300 LOC: guidance warning
  - >500 LOC: hard violation unless locally justified

Generated, vendored, lock, binary, and asset files are skipped. A file over the
hard limit can carry an explicit local justification by including LOC-JUSTIFY in
the file.

Options:
  --all  Print all files over 300 LOC, not only hard violations and summary.
EOF
}

print_all=false
if [[ "${1:-}" == "--all" ]]; then
  print_all=true
elif [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
elif [[ $# -gt 0 ]]; then
  usage >&2
  exit 2
fi

is_exempt_path() {
  local path="$1"
  case "$path" in
    Cargo.lock|Package.resolved|*.xcworkspace/contents.xcworkspacedata) return 0 ;;
    swift-bindings/*) return 0 ;;
    ios-app/Sources/TenexMVP/TenexCore/tenex_core.swift) return 0 ;;
    */TenexCoreFFI/*|*/SourcePackages/*|*/.build/*|*/DerivedData*/*) return 0 ;;
    *.png|*.jpg|*.jpeg|*.gif|*.webp|*.ico|*.pdf|*.mp3|*.wav|*.aiff) return 0 ;;
    *.pbxproj|*.xcuserstate) return 0 ;;
  esac
  return 1
}

is_text_file() {
  local path="$1"
  grep -Iq . "$path"
}

has_local_justification() {
  local path="$1"
  rg -q "LOC-JUSTIFY" "$path"
}

hard_count=0
hard_justified_count=0
soft_count=0

while IFS= read -r -d '' path; do
  if is_exempt_path "$path"; then
    continue
  fi
  if ! is_text_file "$path"; then
    continue
  fi

  lines=$(wc -l < "$path" | tr -d ' ')
  if (( lines <= soft_limit )); then
    continue
  fi

  if (( lines > hard_limit )); then
    if has_local_justification "$path"; then
      ((hard_justified_count += 1))
      if [[ "$print_all" == true ]]; then
        printf "JUSTIFIED\t%s\t%s\n" "$lines" "$path"
      fi
    else
      ((hard_count += 1))
      printf "HARD\t%s\t%s\n" "$lines" "$path"
    fi
  else
    ((soft_count += 1))
    if [[ "$print_all" == true ]]; then
      printf "WARN\t%s\t%s\n" "$lines" "$path"
    fi
  fi
done < <(git ls-files -z)

printf "summary: hard=%s justified_hard=%s warn=%s\n" \
  "$hard_count" "$hard_justified_count" "$soft_count"

if (( hard_count > 0 )); then
  exit 1
fi
