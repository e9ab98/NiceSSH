#!/usr/bin/env python3
"""Parse GitHub compare API JSON from stdin, emit one markdown line per commit.

Input: JSON from https://api.github.com/repos/{owner}/{repo}/compare/{base}...{head}
Output: markdown bullet list with short SHA + subject, each commit linked to its page.
"""
import json
import sys

d = json.load(sys.stdin)
for c in d.get("commits", []):
    sha = c["sha"][:7]
    msg = c["commit"]["message"].splitlines()[0]
    url = c["html_url"]
    print(f"- [`{sha}`]({url}) {msg}")
