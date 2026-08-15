#!/usr/bin/env bash
# ------------------------------------------------------------
# watch-and-commit.sh
#   • Monitors the repository every minute
#   • Stages any changes (including new files)
#   • Creates a commit with an emoji prefix that matches the type of change
#   • Runs for up to 60 minutes (configurable)
#
#   Usage:   ./watch-and-commit.sh
# ------------------------------------------------------------

# ------------------- Configuration -----------------------------
MAX_MINUTES=60          # total runtime (minutes)
SLEEP_SEC=60            # pause between checks (seconds)
# BRANCH="main"         # branch to work on (change if needed)
# -----------------------------------------------------------

# Emoji / commit-type mapping (customise if you want)
declare -A COMMIT_TYPES=(
    ["feat"]="🚀"
    ["fix"]="🐛"
    ["docs"]="📝"
    ["test"]="✅"
    ["chore"]="🔧"
    ["refactor"]="🔧"
    ["perf"]="⚡"
    ["style"]="🎨"
    ["build"]="🔨"
    ["ci"]="🤖"
    ["revert"]="🔁"
)

# ------------------- Helper Functions ---------------------------

# Helper: detect the type of change from the list of changed files
detect_change_type() {
    local files=$1
    local type="chore"
    local emoji="🔧"
    if [[ -z $files ]]; then
        echo "🔧 chore"
        return
    fi
    for f in $files; do
        case "$f" in
            *.md|*.mdx|*.txt|README*|*.adoc)
                type="docs"
                emoji="📝"
                ;;
            *.rs)
                type="feat"
                emoji="🚀"
                ;;
            *test*|*spec*|*mod.rs*)
                type="test"
                emoji="✅"
                ;;
            *setup*|*ci*|*.yml|*.yaml|*.json)
                type="ci"
                emoji="🤖"
                ;;
            *)
                type="chore"
                emoji="🔧"
                ;;
        esac
    done
    echo "$emoji $type"
}

# ------------------- Main Loop ---------------------------

echo "🚀 Starting repository watcher (max ${MAX_MINUTES} minutes)…"
elapsed=0
while (( elapsed < MAX_MINUTES )); do
    clear

    echo "⏱  Checking for changes (iteration $((elapsed/60+1)))…"
    changed=$(git status --porcelain)

    if [[ -n $changed ]]; then
        echo "🔎 Changes detected! Preparing commit…"

        # Stage all changes
        git add -A > /dev/null

        # Get the list of changed files
        files_to_commit=$(git diff --name-only --cached)

        # Get the emoji and type
        read -r emoji type <<< "$(detect_change_type "$files_to_commit")"

        # Generate commit message using aichat with correct syntax
        commit_message=$(echo "Generate a commit message with the emoji prefix '$emoji' for the following changes:" | aichat -m "gpt-3.5-turbo" -- -p "You are a helpful assistant that writes concise commit messages for git commits. Use the provided emoji as the first character of the commit message and generate a concise commit message for the following changes: $(git diff --cached --name-only)")
        
        echo "✅ Creating commit: \"$commit_message\""
        git commit -m "$commit_message" --no-verify > /dev/null
        echo "✅ Commit created successfully."
    else
        echo "✅ No changes detected – sleeping..."
    fi

    # Wait the configured interval
    sleep "$SLEEP_SEC"

    # Update elapsed time
    (( elapsed+=SLEEP_SEC ))
done

echo "🎉 Finished watching after ${MAX_MINUTES} minutes."