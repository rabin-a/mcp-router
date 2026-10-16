"""Write fleet-direct.json (12 fake servers, 441 tools) and fleet-ours.json (same fleet behind mcp-router).

usage: python3 make_fleet.py [path/to/mcp-router binary]
"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROUTER = sys.argv[1] if len(sys.argv) > 1 else "mcp-router"
os.makedirs(f"{HERE}/work", exist_ok=True)
domains = ["jira", "slack", "gmail", "calendar", "stripe", "notion", "shopify", "linear", "drive", "hubspot", "sentry"]
fleet = {"github": {"command": "python3", "args": ["-I", f"{HERE}/fake_github.py"]}}
fleet.update({d: {"command": "python3", "args": ["-I", f"{HERE}/fake_mcp.py", d]} for d in domains})
json.dump({"mcpServers": fleet}, open(f"{HERE}/fleet-direct.json", "w"), indent=1)
toml = "".join(f'[servers.{n}]\ncommand = "{c["command"]}"\nargs = {json.dumps(c["args"])}\n' for n, c in fleet.items())
open(f"{HERE}/fleet-ours.toml", "w").write(toml)
json.dump({"mcpServers": {"router": {"command": ROUTER, "env": {"MCP_ROUTER_CONFIG": f"{HERE}/fleet-ours.toml"}}}},
          open(f"{HERE}/fleet-ours.json", "w"), indent=1)
print("wrote fleet-direct.json and fleet-ours.json; add fleet-<name>.json for any other proxy")
