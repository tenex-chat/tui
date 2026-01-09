#!/bin/bash

# Setup git hooks for this repository

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(dirname "$SCRIPT_DIR")"
HOOKS_DIR="$REPO_DIR/.git/hooks"

echo "Setting up git hooks..."

# Create pre-commit hook
cat > "$HOOKS_DIR/pre-commit" << 'HOOK_EOF'
#!/bin/bash

# Pre-commit hook to validate UI style guidelines
# Triggers Claude to check for violations in staged UI files

set -e

# Get list of staged .rs files in src/ui/
STAGED_UI_FILES=$(git diff --cached --name-only --diff-filter=ACM | grep -E '^src/ui/.*\.rs$' || true)

# Skip if no UI files are staged
if [ -z "$STAGED_UI_FILES" ]; then
    exit 0
fi

echo "Checking UI style guidelines..."

# Quick check: Look for raw Color:: usages (except Color::Black)
VIOLATIONS=""
for file in $STAGED_UI_FILES; do
    # Skip theme.rs - it's allowed to define colors
    if [[ "$file" == "src/ui/theme.rs" ]]; then
        continue
    fi

    # Check for Color:: usages that aren't Color::Black
    RAW_COLORS=$(git show ":$file" 2>/dev/null | grep -n 'Color::' | grep -v 'Color::Black' | grep -v 'use.*Color' | grep -v '// Color' || true)

    if [ -n "$RAW_COLORS" ]; then
        VIOLATIONS="$VIOLATIONS\n$file:\n$RAW_COLORS"
    fi
done

# If quick check finds violations, run Claude for detailed analysis
if [ -n "$VIOLATIONS" ]; then
    echo ""
    echo "Potential style violations found:"
    echo -e "$VIOLATIONS"
    echo ""
    echo "Running Claude for detailed analysis..."
    echo ""

    CLAUDE_PROMPT="You are reviewing UI code for style guideline violations.

RULES (from docs/UI_STYLE_GUIDELINES.md):
1. Never use hardcoded Color:: values except Color::Black
2. All colors must come from theme:: constants (e.g., theme::ACCENT_PRIMARY, theme::TEXT_MUTED)
3. Color passed as function parameter is acceptable

FILES WITH VIOLATIONS:
$STAGED_UI_FILES

VIOLATIONS DETECTED:
$VIOLATIONS

Analyze and respond:
VERDICT: APPROVE (acceptable exceptions like Color::Black or parameter) or REJECT (needs fixing)
REASON: Brief explanation
FIXES NEEDED: (if rejected) List specific changes"

    RESULT=$(echo "$CLAUDE_PROMPT" | claude --print 2>/dev/null || echo "VERDICT: REJECT
REASON: Could not run Claude analysis. Manual review required.")

    echo "$RESULT"
    echo ""

    if echo "$RESULT" | grep -q "VERDICT: REJECT"; then
        echo "Style check FAILED. Please fix violations."
        echo ""
        echo "Quick reference:"
        echo "  Color::Cyan      -> theme::ACCENT_PRIMARY"
        echo "  Color::DarkGray  -> theme::TEXT_MUTED"
        echo "  Color::White     -> theme::TEXT_PRIMARY"
        echo "  Color::Green     -> theme::ACCENT_SUCCESS"
        echo "  Color::Yellow    -> theme::ACCENT_WARNING"
        echo "  Color::Red       -> theme::ACCENT_ERROR"
        echo "  Color::Magenta   -> theme::ACCENT_SPECIAL"
        echo ""
        echo "See docs/UI_STYLE_GUIDELINES.md"
        exit 1
    fi

    echo "Style check PASSED."
fi

exit 0
HOOK_EOF

chmod +x "$HOOKS_DIR/pre-commit"
echo "Installed pre-commit hook."

echo ""
echo "Git hooks setup complete!"
echo ""
echo "The pre-commit hook will:"
echo "  - Check staged UI files for style guideline violations"
echo "  - Run Claude analysis on potential violations"
echo "  - Block commits that violate the UI style guidelines"
echo ""
echo "To skip the hook (not recommended): git commit --no-verify"
