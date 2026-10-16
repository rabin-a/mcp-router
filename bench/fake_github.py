"""Fake GitHub MCP server returning realistic, bulky REST-shaped JSON."""
import json, sys, os
def user(login, i):
    b = f"https://api.github.com/users/{login}"
    return {"login": login, "id": 1000 + i, "node_id": f"MDQ6VXNlcj{i}", "avatar_url": f"https://avatars.githubusercontent.com/u/{1000+i}?v=4",
            "gravatar_id": "", "url": b, "html_url": f"https://github.com/{login}", "followers_url": f"{b}/followers",
            "following_url": f"{b}/following{{/other_user}}", "gists_url": f"{b}/gists{{/gist_id}}", "starred_url": f"{b}/starred{{/owner}}{{/repo}}",
            "subscriptions_url": f"{b}/subscriptions", "organizations_url": f"{b}/orgs", "repos_url": f"{b}/repos", "events_url": f"{b}/events{{/privacy}}",
            "received_events_url": f"{b}/received_events", "type": "User", "user_view_type": "public", "site_admin": False}
def issue(n):
    r = "https://api.github.com/repos/acme/web"
    labels = [{"id": 77 + n, "node_id": f"LA_kw{n}", "url": f"{r}/labels/{'bug' if n % 3 == 0 else 'enhancement'}",
               "name": "bug" if n % 3 == 0 else "enhancement", "color": "d73a4a", "default": True, "description": None}]
    return {"url": f"{r}/issues/{n}", "repository_url": r, "labels_url": f"{r}/issues/{n}/labels{{/name}}", "comments_url": f"{r}/issues/{n}/comments",
            "events_url": f"{r}/issues/{n}/events", "html_url": f"https://github.com/acme/web/issues/{n}", "id": 2000000 + n, "node_id": f"I_kwDOAbc{n}",
            "number": n, "title": f"{'Crash' if n % 3 == 0 else 'Improve'} in module {n}: {'checkout fails on submit' if n % 3 == 0 else 'polish settings page'}",
            "user": user(f"dev{n % 5}", n), "labels": labels, "state": "open", "locked": False, "assignee": None, "assignees": [], "milestone": None,
            "comments": n % 4, "created_at": f"2026-09-{n:02d}T10:00:00Z", "updated_at": f"2026-10-{n % 15 + 1:02d}T10:00:00Z", "closed_at": None,
            "author_association": "MEMBER", "type": None, "active_lock_reason": None, "sub_issues_summary": {"total": 0, "completed": 0, "percent_completed": 0},
            "body": f"Steps to reproduce issue {n}.", "closed_by": None,
            "reactions": {"url": f"{r}/issues/{n}/reactions", "total_count": 0, "+1": 0, "-1": 0, "laugh": 0, "hooray": 0, "confused": 0, "heart": 0, "rocket": 0, "eyes": 0},
            "timeline_url": f"{r}/issues/{n}/timeline", "performed_via_github_app": None, "state_reason": None}
TOOLS = [{"name": "list_issues", "description": "List open issues in a GitHub repository.",
          "inputSchema": {"type": "object", "properties": {"repo": {"type": "string", "description": "owner/name"}}, "required": ["repo"]}}]
for line in sys.stdin:
    msg = json.loads(line)
    if "id" not in msg: continue
    m, i = msg["method"], msg["id"]
    if m == "initialize": res = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake-github", "version": "1"}}
    elif m == "tools/list": res = {"tools": TOOLS}
    elif m == "tools/call":
        if True: open(os.environ.get("FAKE_LOG", os.path.join(os.path.dirname(os.path.abspath(__file__)), "work", "calls.log")), "a").write(json.dumps({"tool": "github." + msg["params"]["name"], "args": msg["params"].get("arguments", {})}) + "\n")
        res = {"content": [{"type": "text", "text": json.dumps([issue(n) for n in range(1, 21)], indent=2)}]}
    else: res = {}
    print(json.dumps({"jsonrpc": "2.0", "id": i, "result": res}), flush=True)
