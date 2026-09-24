#!/usr/bin/env python3
"""Render the GitHub Release body for a tag from Conventional Commit history.

Usage: release-notes.py <version> <previous-tag|""> [<head-rev>]

Commits in <previous-tag>..<head-rev> (merges excluded) are grouped by type;
`type!:` or a `BREAKING CHANGE:` footer lifts a commit into its own section.
Hand-written highlights in release/notes/<version>.md, when present, are
placed above the generated list. Output goes to stdout as Markdown.
"""

import os
import re
import subprocess
import sys

SUBJECT_RE = re.compile(r"^(?P<type>[a-zA-Z-]+)(?:\((?P<scope>[^)]*)\))?(?P<bang>!)?:\s*(?P<desc>.+)$")
BREAKING_RE = re.compile(r"^BREAKING[ -]CHANGE:", re.MULTILINE)
# `chore(release): prepare vX.Y.Z` only bumps version strings.
SKIP_RE = re.compile(r"^chore\(release\):")

# (heading, types); order is the render order. Anything unmatched lands in 其他.
PRIMARY = [
    ("新功能", {"feat"}),
    ("修复", {"fix"}),
    ("性能", {"perf"}),
    ("文档", {"docs"}),
]
# Rendered folded: useful for auditing, noise for most readers.
INTERNAL = [
    ("重构", {"refactor"}),
    ("测试", {"test"}),
    ("构建与 CI", {"build", "ci"}),
    ("杂项", {"chore", "style", "i18n", "revert"}),
]
# GitHub caps release bodies at 125000 characters.
BODY_LIMIT = 120000


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def load_commits(rev_range):
    raw = git("log", "--no-merges", "--reverse", "--format=%H%x1f%an%x1f%s%x1f%b%x1e", rev_range)
    commits = []
    for record in raw.split("\x1e"):
        record = record.strip("\n")
        if not record:
            continue
        sha, author, subject, body = record.split("\x1f", 3)
        if SKIP_RE.match(subject):
            continue
        m = SUBJECT_RE.match(subject)
        if m:
            ctype = m["type"].lower()
            scope = (m["scope"] or "").strip()
            desc = m["desc"].strip()
            breaking = bool(m["bang"])
        else:
            ctype, scope, desc, breaking = "", "", subject.strip(), False
        breaking = breaking or bool(BREAKING_RE.search(body))
        commits.append(
            {"sha": sha, "author": author, "type": ctype, "scope": scope, "desc": desc, "breaking": breaking}
        )
    return commits


def line(c, repo_url):
    scope = f"**{c['scope']}**: " if c["scope"] else ""
    sha = c["sha"]
    link = f"[`{sha[:7]}`]({repo_url}/commit/{sha})" if repo_url else f"`{sha[:7]}`"
    return f"- {scope}{c['desc']} ({link})"


def section(title, commits, repo_url, level="###"):
    # Group by scope so related changes sit together; commits stay oldest-first.
    ordered = sorted(commits, key=lambda c: (c["scope"] == "", c["scope"]))
    return [f"{level} {title}", "", *(line(c, repo_url) for c in ordered), ""]


def render(version, prev, head, repo_url, include_internal=True):
    out = []

    highlights = f"release/notes/{version}.md"
    if os.path.isfile(highlights):
        with open(highlights, encoding="utf-8") as f:
            text = f.read().strip()
        if text:
            out += [text, ""]

    if not prev:
        out += ["首个发布，无上一版本可比较。", ""]
        return "\n".join(out)

    commits = load_commits(f"{prev}..{head}")
    if not commits:
        out += [f"自 `{prev}` 以来没有新的提交。", ""]
        return "\n".join(out)

    breaking = [c for c in commits if c["breaking"]]
    if breaking:
        out += section("⚠️ 不兼容变更", breaking, repo_url)
    rest = [c for c in commits if not c["breaking"]]

    claimed = set()
    for title, types in PRIMARY:
        group = [c for c in rest if c["type"] in types]
        if group:
            out += section(title, group, repo_url)
        claimed |= types

    internal_types = set().union(*(types for _, types in INTERNAL))
    other = [c for c in rest if c["type"] not in claimed and c["type"] not in internal_types]
    if other:
        out += section("其他", other, repo_url)

    internal = [c for c in rest if c["type"] in internal_types]
    if internal and include_internal:
        out += [f"<details><summary>内部改动（{len(internal)}）</summary>", ""]
        for title, types in INTERNAL:
            group = [c for c in internal if c["type"] in types]
            if group:
                out += section(title, group, repo_url, level="####")
        out += ["</details>", ""]

    authors = sorted({c["author"] for c in commits if not c["author"].endswith("[bot]")})
    if authors:
        out += ["**贡献者**：" + "、".join(authors), ""]

    if repo_url:
        out += [f"**完整变更**：[{prev}...{version}]({repo_url}/compare/{prev}...{version})", ""]
    return "\n".join(out)


def main():
    if len(sys.argv) not in (3, 4):
        sys.exit(__doc__)
    version, prev = sys.argv[1], sys.argv[2]
    head = sys.argv[3] if len(sys.argv) == 4 else "HEAD"
    server = os.environ.get("GITHUB_SERVER_URL", "https://github.com")
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    repo_url = f"{server}/{repo}" if repo else ""

    body = render(version, prev, head, repo_url)
    if len(body) > BODY_LIMIT:
        body = render(version, prev, head, repo_url, include_internal=False)
    sys.stdout.write(body)


if __name__ == "__main__":
    main()
