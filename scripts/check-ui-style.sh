#!/bin/bash

# UI Style Guidelines Checker
# Run this manually or via pre-commit hook to validate UI code

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Determine which files to check
if [ "$1" == "--staged" ]; then
    FILES=$(git diff --cached --name-only --diff-filter=ACM | grep -E '^src/ui/.*\.rs$' || true)
    MODE="staged"
elif [ -n "$1" ]; then
    FILES="$@"
    MODE="specified"
else
    FILES=$(find src/ui -name "*.rs" -type f)
    MODE="all"
fi

if [ -z "$FILES" ]; then
    echo -e "${GREEN}No UI files to check.${NC}"
    exit 0
fi

echo -e "${YELLOW}Checking UI style guidelines ($MODE files)...${NC}"
echo ""

VIOLATIONS=""
VIOLATION_COUNT=0

for file in $FILES; do
    # Skip theme.rs - it's allowed to define colors
    if [[ "$file" == *"theme.rs" ]]; then
        continue
    fi

    # Get file content (staged or current)
    if [ "$MODE" == "staged" ]; then
        CONTENT=$(git show ":$file" 2>/dev/null || cat "$file")
    else
        CONTENT=$(cat "$file")
    fi

    # Check for Color:: usages that aren't Color::Black or in imports
    RAW_COLORS=$(echo "$CONTENT" | grep -n 'Color::' | grep -v 'Color::Black' | grep -v '^[0-9]*:use' | grep -v '// Color' || true)

    if [ -n "$RAW_COLORS" ]; then
        VIOLATIONS="$VIOLATIONS\n${RED}$file:${NC}\n$RAW_COLORS\n"
        VIOLATION_COUNT=$((VIOLATION_COUNT + $(echo "$RAW_COLORS" | wc -l)))
    fi
done

if [ -n "$VIOLATIONS" ]; then
    echo -e "${RED}Found $VIOLATION_COUNT potential style violation(s):${NC}"
    echo -e "$VIOLATIONS"
    echo ""
    echo -e "${YELLOW}Quick fix reference:${NC}"
    echo "  Color::Cyan      -> theme::ACCENT_PRIMARY"
    echo "  Color::DarkGray  -> theme::TEXT_MUTED"
    echo "  Color::White     -> theme::TEXT_PRIMARY"
    echo "  Color::Green     -> theme::ACCENT_SUCCESS"
    echo "  Color::Yellow    -> theme::ACCENT_WARNING"
    echo "  Color::Red       -> theme::ACCENT_ERROR"
    echo "  Color::Magenta   -> theme::ACCENT_SPECIAL"
    echo "  Color::Gray      -> theme::TEXT_MUTED"
    echo "  Color::Blue      -> theme::ACCENT_PRIMARY"
    echo ""
    echo "  Color::Rgb(30,30,30)   -> theme::BG_CARD"
    echo "  Color::Rgb(45,45,45)   -> theme::BG_SELECTED"
    echo "  Color::Rgb(38,38,38)   -> theme::BG_SECONDARY"
    echo "  Color::Rgb(60,60,60)   -> theme::BORDER_INACTIVE"
    echo ""
    echo -e "See ${YELLOW}docs/UI_STYLE_GUIDELINES.md${NC} for full documentation."
    echo ""

    # If running with --strict, exit with error
    if [ "$STRICT" == "1" ] || [ "$2" == "--strict" ]; then
        echo -e "${RED}Style check FAILED.${NC}"
        exit 1
    fi

    # Otherwise, prompt for Claude analysis
    echo -e "${YELLOW}Run Claude analysis to check if violations are acceptable? [y/N]${NC}"
    read -r response
    if [[ "$response" =~ ^[Yy]$ ]]; then
        echo ""
        echo "Running Claude analysis..."

        CLAUDE_PROMPT="Review these UI code violations against style guidelines.

RULES:
1. Never use hardcoded Color:: values except Color::Black (for contrast)
2. All colors must come from theme:: constants
3. Color passed as function parameter is acceptable

VIOLATIONS:
$(echo -e "$VIOLATIONS")

Respond with:
VERDICT: APPROVE (acceptable exceptions) or REJECT (needs fixing)
REASON: Brief explanation"

        echo "$CLAUDE_PROMPT" | claude --print 2>/dev/null || echo "Could not run Claude. Please review manually."
    fi
else
    echo -e "${GREEN}No style violations found!${NC}"
fi
