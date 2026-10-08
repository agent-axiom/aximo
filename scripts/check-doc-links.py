#!/usr/bin/env python3
"""Check relative file links in repository Markdown (no network required)."""
import re
from pathlib import Path
from urllib.parse import unquote, urlsplit

root = Path(__file__).resolve().parents[1]
errors = []
for document in [root / "README.md", root / "SECURITY.md", *sorted((root / "docs").rglob("*.md"))]:
    # Exclude fenced examples so sample Markdown is not treated as navigation.
    text = re.sub(r"```.*?```", "", document.read_text(), flags=re.S)
    for target in re.findall(r"!?\[[^\]]*\]\(([^\s)]+)(?:\s+[^)]*)?\)", text):
        link = urlsplit(target)
        if link.scheme or link.netloc or not link.path:
            continue
        destination = document.parent / unquote(link.path)
        if not destination.exists():
            errors.append(f"{document.relative_to(root)}: missing {target}")
if errors:
    raise SystemExit("\n".join(errors))
print("All relative Markdown file links resolve.")
