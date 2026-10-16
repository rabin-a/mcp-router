"""Fake MCP server: realistic-looking tools for one domain, canned results. Usage: fake_mcp.py <domain>"""
import json, sys, os
DOMAINS = {
 "github": ("repository", ["issue", "pull_request", "branch", "release", "workflow_run", "comment", "label", "milestone"]),
 "jira": ("project", ["issue", "sprint", "epic", "board", "worklog", "comment", "version", "component"]),
 "slack": ("workspace", ["message", "channel", "reaction", "user", "file", "reminder", "bookmark", "thread"]),
 "gmail": ("mailbox", ["email", "draft", "label", "thread", "filter", "attachment", "contact", "signature"]),
 "calendar": ("calendar", ["event", "attendee", "reminder", "room", "availability", "invitation", "series", "note"]),
 "stripe": ("account", ["customer", "invoice", "subscription", "refund", "payment_intent", "product", "price", "coupon"]),
 "notion": ("workspace", ["page", "database", "block", "comment", "property", "template", "view", "member"]),
 "shopify": ("store", ["product", "order", "collection", "discount", "inventory_level", "fulfillment", "customer", "variant"]),
 "linear": ("team", ["issue", "cycle", "project", "label", "comment", "roadmap", "attachment", "workflow_state"]),
 "drive": ("drive", ["file", "folder", "permission", "revision", "comment", "shortcut", "shared_drive", "export"]),
 "hubspot": ("portal", ["contact", "company", "deal", "ticket", "note", "task", "pipeline", "owner"]),
 "sentry": ("organization", ["issue", "event", "release", "project", "alert_rule", "team", "deploy", "replay"]),
}
VERBS = [("create", "Create a new {n} in the {c}. Returns the created {n} with its id and URL."),
         ("get", "Get a single {n} by id, including all fields and metadata."),
         ("list", "List {n}s in the {c}, with filtering, sorting and cursor pagination."),
         ("update", "Update fields on an existing {n}. Only provided fields are changed."),
         ("delete", "Permanently delete a {n}. This cannot be undone.")]
domain = sys.argv[1]
container, nouns = DOMAINS[domain]
def schema(verb, noun):
    props = {f"{container}_id": {"type": "string", "description": f"The {container} identifier. Use the {container} list tool to find it if unknown."}}
    if verb in ("get", "update", "delete"):
        props[f"{noun}_id"] = {"type": "string", "description": f"Identifier of the {noun.replace('_', ' ')} to operate on."}
    if verb in ("create", "update"):
        props["title"] = {"type": "string", "description": f"Title or name of the {noun.replace('_', ' ')}. Plain text, max 255 characters."}
        props["body"] = {"type": "string", "description": "Optional longer description. Markdown is supported where the service supports it."}
        props["metadata"] = {"type": "object", "description": "Optional key/value metadata to attach.", "additionalProperties": {"type": "string"}}
    if verb == "list":
        props["query"] = {"type": "string", "description": "Optional free-text filter applied to titles and bodies."}
        props["limit"] = {"type": "integer", "description": "Maximum results to return (1-100, default 25)."}
        props["cursor"] = {"type": "string", "description": "Pagination cursor from a previous response."}
        props["sort"] = {"type": "string", "enum": ["created", "updated", "name"], "description": "Sort order for results."}
    req = [f"{container}_id"] + ([f"{noun}_id"] if verb in ("get", "update", "delete") else []) + (["title"] if verb == "create" else [])
    return {"type": "object", "properties": props, "required": req, "$schema": "http://json-schema.org/draft-07/schema#", "additionalProperties": False}
TOOLS = [{"name": f"{v}_{n}", "description": f"[{domain}] " + d.format(n=n.replace('_', ' '), c=container), "inputSchema": schema(v, n)}
         for n in nouns for v, d in VERBS]
for line in sys.stdin:
    msg = json.loads(line)
    if "id" not in msg: continue
    m, i = msg["method"], msg["id"]
    if m == "initialize":
        res = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": f"fake-{domain}", "version": "1"}}
    elif m == "tools/list":
        res = {"tools": TOOLS}
    elif m == "tools/call":
        name, args = msg["params"]["name"], msg["params"].get("arguments", {})
        if True: open(os.environ.get("FAKE_LOG", os.path.join(os.path.dirname(os.path.abspath(__file__)), "work", "calls.log")), "a").write(json.dumps({"tool": f"{domain}.{name}", "args": args}) + "\n")
        res = {"content": [{"type": "text", "text": json.dumps({"ok": True, "tool": f"{domain}.{name}", "id": f"{domain.upper()}-4821", "received": args})}]}
    else:
        res = {}
    print(json.dumps({"jsonrpc": "2.0", "id": i, "result": res}), flush=True)
