#!/usr/bin/env python3
"""Inline LLM code review for GitHub PRs against a self-hosted OpenAI-compatible endpoint.

Flow:
  1. Fetch the PR's changed files (with unified-diff patches) from the GitHub API.
  2. Parse each patch into a line-numbered view and record which RIGHT-side
     line numbers are actually part of the diff (only those are commentable).
  3. Ask the LLM for findings as JSON: {summary, comments:[{path,line,severity,body}]}.
  4. Validate every finding's (path, line) against the real diff positions.
  5. Post ONE review (event=COMMENT) with the valid findings as inline comments;
     anything that doesn't map to a diff line is folded into the summary.

Only the Python standard library is used, so the runner needs no pip install.
"""
import json
import os
import re
import sys
import urllib.error
import urllib.request

GITHUB_API = "https://api.github.com"
HUNK_RE = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@")

SYSTEM_PROMPT = (
    "You are a senior code reviewer for the Hermit unikernel: no_std Rust kernel "
    "code, cross-arch (x86_64/aarch64/riscv64), with heavy unsafe, inline asm and "
    "MMIO. Review the numbered diff below and report only concrete, actionable "
    "problems: correctness bugs, unsound unsafe, broken invariants, missing error "
    "handling, concurrency/ordering issues, real footguns. Be conservative about "
    "unsafe and asm - flag only clear problems, not style, and skip nits. Always "
    "respond in English.\n\n"
    "Respond with ONLY a JSON object, no prose and no markdown fences, shaped like:\n"
    '{"summary": string, "comments": [{"path": string, "line": number, '
    '"severity": "high"|"medium"|"low", "body": string}]}\n'
    "Each 'line' MUST be a RIGHT-side line number shown in the numbered diff for "
    "that exact 'path'. If you have no line-specific findings, return an empty "
    "comments array and put overall remarks in 'summary'."
)

# Keep the payload within a small local model's context window.
DIFF_CHAR_BUDGET = 60000


def require_env(name):
    """Return a required env var or exit(1) with a clear message."""
    val = os.environ.get(name, "").strip()
    if not val:
        sys.exit(
            f"ERROR: required environment variable '{name}' is not set or empty.\n"
            f"  - Check the `env:` block in the workflow.\n"
            f"  - If it comes from a secret/variable, confirm that secret exists and is "
            f"spelled exactly the same (secrets are empty for pull_request runs from forks)."
        )
    return val


def gh_request(method, url, token, data=None):
    headers = {
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "Content-Type": "application/json",
    }
    body = json.dumps(data).encode() if data is not None else None
    req = urllib.request.Request(url, data=body, headers=headers, method=method)
    with urllib.request.urlopen(req, timeout=60) as resp:
        raw = resp.read().decode()
    return json.loads(raw) if raw else {}


def get_pr_files(repo, pr, token):
    files, page = [], 1
    while True:
        url = f"{GITHUB_API}/repos/{repo}/pulls/{pr}/files?per_page=100&page={page}"
        batch = gh_request("GET", url, token)
        if not batch:
            break
        files.extend(batch)
        if len(batch) < 100:
            break
        page += 1
    return files


def parse_patch(patch):
    """Return (numbered_text, valid_right_lines).

    valid_right_lines is the set of new-file line numbers that appear in the
    diff (added or context lines) and are therefore commentable on side=RIGHT.
    """
    out, valid, new_line = [], set(), None
    for raw in patch.splitlines():
        m = HUNK_RE.match(raw)
        if m:
            new_line = int(m.group(1))
            out.append(raw)
            continue
        if new_line is None:
            continue  # file header noise before the first hunk
        tag, content = raw[:1], raw[1:]
        if tag == "+":
            out.append(f"{new_line:>6} + {content}")
            valid.add(new_line)
            new_line += 1
        elif tag == " ":
            out.append(f"{new_line:>6}   {content}")
            valid.add(new_line)
            new_line += 1
        elif tag == "-":
            out.append(f"     - - {content}")  # deleted: no RIGHT line number
        # "\ No newline at end of file" and anything else is ignored
    return "\n".join(out), valid


def completions_url(base):
    return base.rstrip("/") + "/chat/completions"


def call_llm(base, model, api_key, system, user):
    payload = {
        "model": model,
        "temperature": 0.1,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
    }
    headers = {"Content-Type": "application/json"}
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"
    req = urllib.request.Request(
        completions_url(base), data=json.dumps(payload).encode(),
        headers=headers, method="POST",
    )
    with urllib.request.urlopen(req, timeout=600) as resp:
        body = resp.read().decode()
    try:
        data = json.loads(body)
    except json.JSONDecodeError:
        raise RuntimeError(f"endpoint returned non-JSON response: {body[:500]}")
    # Some OpenAI-compatible servers answer HTTP 200 with an error object.
    if not data.get("choices"):
        raise RuntimeError(f"response has no 'choices' (endpoint error / wrong model?): {body[:500]}")
    return data["choices"][0]["message"]["content"]


def extract_json(text):
    text = text.strip()
    if text.startswith("```"):
        text = re.sub(r"^```[a-zA-Z]*\n?", "", text)
        text = re.sub(r"\n?```$", "", text).strip()
    start, end = text.find("{"), text.rfind("}")
    if start == -1 or end == -1:
        raise ValueError("no JSON object in model output")
    return json.loads(text[start : end + 1])


def post_review(repo, pr, token, sha, body, comments):
    url = f"{GITHUB_API}/repos/{repo}/pulls/{pr}/reviews"
    payload = {"commit_id": sha, "event": "COMMENT", "body": body, "comments": comments}
    try:
        gh_request("POST", url, token, payload)
        print(f"Posted review with {len(comments)} inline comment(s).")
    except urllib.error.HTTPError as e:
        detail = e.read().decode()
        print(f"Review POST failed ({e.code}): {detail}", file=sys.stderr)
        if e.code == 403:
            sys.exit(
                "ERROR: 403 when posting the review. The token lacks 'pull-requests: "
                "write' - this happens for pull_request runs from forks. Trigger the "
                "review on the base repo (or via pull_request_target with care)."
            )
        # A single bad line rejects the whole batch - degrade to summary only.
        payload.pop("comments", None)
        gh_request("POST", url, token, payload)
        print("Posted summary-only review as fallback.")


def main():
    # Fail loudly and clearly if the environment isn't wired up.
    token = require_env("GITHUB_TOKEN")
    repo = require_env("GITHUB_REPOSITORY")
    base = require_env("LLM_API_BASE")
    event_path = require_env("GITHUB_EVENT_PATH")
    model = os.environ.get("LLM_MODEL", "").strip() or "default"
    api_key = os.environ.get("LLM_API_KEY", "").strip()

    with open(event_path) as fh:
        event = json.load(fh)
    if "pull_request" not in event:
        sys.exit("ERROR: no pull_request in the event payload - trigger this on `pull_request`.")
    pr = event["pull_request"]["number"]
    head_sha = event["pull_request"]["head"]["sha"]
    print(f"Reviewing {repo} PR #{pr} @ {head_sha[:8]} - LLM endpoint: {base}")

    try:
        files = get_pr_files(repo, pr, token)
    except urllib.error.HTTPError as e:
        sys.exit(
            f"ERROR: GitHub API returned {e.code} when listing PR files. "
            f"Usually a token/permission issue (fork PRs get a read-only token)."
        )
    print(f"{len(files)} changed file(s) from the API.")

    valid_by_path, sections, skipped, budget = {}, [], [], DIFF_CHAR_BUDGET
    for f in files:
        patch, path = f.get("patch"), f["filename"]
        if not patch:  # binary / rename-only / too large to diff
            continue
        numbered, valid = parse_patch(patch)
        if not valid:
            continue
        block = f"### {path}\n{numbered}"
        if len(block) > budget:
            skipped.append(path)
            continue
        budget -= len(block)
        sections.append(block)
        valid_by_path[path] = valid

    if not sections:
        print("No reviewable diff - nothing to do.")
        return
    print(f"{len(sections)} file(s) with reviewable diff; calling {completions_url(base)} (model={model})...")

    try:
        raw = call_llm(base, model, api_key, SYSTEM_PROMPT, "\n\n".join(sections))
    except urllib.error.HTTPError as e:
        detail = e.read().decode()[:500]
        sys.exit(f"ERROR: LLM endpoint returned {e.code}: {detail}")
    except urllib.error.URLError as e:
        sys.exit(
            f"ERROR: could not reach the LLM endpoint at {base} ({e.reason}).\n"
            f"  - Check host, port and that the path ends in /v1."
        )
    except (RuntimeError, KeyError, IndexError) as e:
        sys.exit(f"ERROR: unexpected response from the LLM endpoint - {e}")

    try:
        result = extract_json(raw)
    except (ValueError, json.JSONDecodeError) as e:
        print(f"Could not parse model output as JSON: {e}", file=sys.stderr)
        post_review(
            repo, pr, token, head_sha,
            "## LLM review\n\nThe model did not return parseable JSON; raw "
            f"output below.\n\n```\n{raw[:4000]}\n```",
            [],
        )
        return

    inline, dropped = [], []
    for c in result.get("comments", []):
        path, line = c.get("path"), c.get("line")
        text = (c.get("body") or "").strip()
        if not text:
            continue
        if path in valid_by_path and isinstance(line, int) and line in valid_by_path[path]:
            sev = c.get("severity", "")
            prefix = f"**[{sev}]** " if sev else ""
            inline.append({"path": path, "line": line, "side": "RIGHT", "body": prefix + text})
        else:
            dropped.append(c)

    body = "## LLM review\n\n" + (result.get("summary") or "").strip()
    if skipped:
        body += f"\n\n_Skipped (diff too large for context budget): {', '.join(skipped)}._"
    if dropped:
        body += "\n\n<details><summary>Findings without a valid diff line</summary>\n\n"
        for c in dropped:
            body += f"- `{c.get('path')}:{c.get('line')}` - {(c.get('body') or '').strip()}\n"
        body += "\n</details>"

    post_review(repo, pr, token, head_sha, body, inline)


if __name__ == "__main__":
    main()
