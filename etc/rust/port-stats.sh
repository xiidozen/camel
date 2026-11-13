#!/usr/bin/env bash
# Updates the Java/Rust share of the code in README.md (between the rust-port-stats markers).
# Counts the non-blank lines and the files of the .java and .rs files known to git (tracked, or
# untracked and not ignored). Run before each push of the rust branch.
set -euo pipefail
cd "$(dirname "$0")/../.."

count() { # ext -> "files lines"
    local files lines
    files=$(git ls-files --cached --others --exclude-standard -z -- "*.$1" | xargs -0 -r ls 2>/dev/null | wc -l)
    lines=$(git ls-files --cached --others --exclude-standard -z -- "*.$1" | xargs -0 -r cat 2>/dev/null | grep -cv '^\s*$' || true)
    echo "$files $lines"
}
read -r java_files java_lines <<<"$(count java)"
read -r rust_files rust_lines <<<"$(count rs)"
pct() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.1f", (a + b) ? 100 * a / (a + b) : 0 }'; }
fmt() { echo "$1" | sed ':a;s/\B[0-9]\{3\}\>/,&/;ta'; }

section=$(cat <<SECTION
<!-- rust-port-stats:start -->
> **Port progress** (non-blank lines of the \`.java\` and \`.rs\` files, updated with each push;
> \`./etc/rust/port-stats.sh\`):
>
> | Language | Lines | Share of lines | Files |
> |---|---:|---:|---:|
> | Rust | $(fmt "$rust_lines") | $(pct "$rust_lines" "$java_lines")% | $(fmt "$rust_files") |
> | Java | $(fmt "$java_lines") | $(pct "$java_lines" "$rust_lines")% | $(fmt "$java_files") |
>
> Last updated: $(date -u +%Y-%m-%d). The Java lines include generated code and the Java tests not
> ported yet (ported Java code and tests are removed as they are ported).
<!-- rust-port-stats:end -->
SECTION
)

python3 - "$section" <<'PY'
import re, sys
section = sys.argv[1]
p = "README.md"
s = open(p).read()
start, end = "<!-- rust-port-stats:start -->", "<!-- rust-port-stats:end -->"
if start in s:
    s = re.sub(re.escape(start) + r".*?" + re.escape(end), lambda _: section, s, count=1, flags=re.S)
else:
    # after the first block quote (the rust branch notice)
    i = s.index("\n\n[![")
    s = s[:i] + "\n>\n" + section + s[i:]
open(p, "w").write(s)
PY
echo "Rust $(pct "$rust_lines" "$java_lines")% ($rust_lines lines, $rust_files files), Java $(pct "$java_lines" "$rust_lines")% ($java_lines lines, $java_files files)"
