"""mcp-bench: run the same tasks through Claude Code against different MCP setups.

usage: mcp_bench.py <bench_dir> <reps> <config> [<config>...]
Each <config> is fleet-<name>.json in bench_dir (see make_fleet.py). Run from a copy of
this directory; fake servers log calls to <bench_dir>/work/calls.log. Results append to results-final.jsonl.
Correctness is checked from the fake servers' call log, not from Claude's reply.
"""
import json, os, random, subprocess, sys, time

S, REPS, CONFIGS = sys.argv[1], int(sys.argv[2]), sys.argv[3:]
WORK, LOG = f"{S}/work", f"{S}/work/calls.log"
BUG_TITLES = sorted(f"Crash in module {n}: checkout fails on submit" for n in (3, 6, 9, 12, 15, 18))

def calls():
    return [json.loads(l) for l in open(LOG)] if os.path.exists(LOG) else []

TASKS = {
    "plain": ("What is 2+2? Reply with just the number.",
              lambda reply, log: reply.strip() == "4" and not log),
    "single": ("Using the MCP tools, create a Jira issue in project ENG titled 'Login bug'. Reply with just the returned id.",
               lambda reply, log: any(c["tool"] == "jira.create_issue" and c["args"].get("title") == "Login bug"
                                      and "ENG" in json.dumps(c["args"]) for c in log)),
    "github": ("Using the MCP tools, list open issues in GitHub repo acme/web. Reply with just the issue numbers labeled bug, comma-separated.",
               lambda reply, log: reply.strip().splitlines()[0].replace(" ", "").strip(".") == "3,6,9,12,15,18"
                                  and any(c["tool"] == "github.list_issues" for c in log)),
    "multi": ("Find the open issues labeled bug in GitHub repo acme/web, and create a Jira issue in project ENG for each one, "
              "using the GitHub issue title as the Jira title. Reply with just the number of Jira issues created.",
              lambda reply, log: sorted(c["args"].get("title", "") for c in log if c["tool"] == "jira.create_issue") == BUG_TITLES),
}

plan = [(rep, task, cfg) for rep in range(REPS) for task in TASKS for cfg in CONFIGS]
random.Random(7).shuffle(plan)  # spread prompt-cache warmth across configs
out = open(f"{S}/results-final.jsonl", "a")
for i, (rep, task, cfg) in enumerate(plan, 1):
    mcp = json.load(open(f"{S}/fleet-{cfg}.json"))
    allowed = [f"mcp__{name}" for name in mcp["mcpServers"]]
    if os.path.exists(LOG): os.remove(LOG)
    cmd = ["claude", "-p", TASKS[task][0], "--output-format", "json", "--model", "sonnet",
           "--mcp-config", f"{S}/fleet-{cfg}.json", "--strict-mcp-config", "--allowedTools", *allowed]
    t = time.time()
    try:
        p = subprocess.run(cmd, cwd=WORK, capture_output=True, text=True, timeout=900, env={**os.environ, "FAKE_LOG": LOG})
        r = json.loads(p.stdout)
    except Exception as e:
        r = {"error": f"{type(e).__name__}: {str(e)[-300:]}"}
    u = r.get("usage", {})
    reply = str(r.get("result", r.get("error", "")))
    row = {"cfg": cfg, "task": task, "rep": rep, "turns": r.get("num_turns"),
           "ctx_in": sum(u.get(k, 0) for k in ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"]),
           "output": u.get("output_tokens"), "wall_s": round(time.time() - t, 1),
           "correct": bool(TASKS[task][1](reply, calls())), "n_calls": len(calls()), "reply": reply[:100]}
    print(f"[{i}/{len(plan)}]", json.dumps(row), flush=True)
    out.write(json.dumps(row) + "\n"); out.flush()
