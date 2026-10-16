# MCP token-saving proxies, measured in Claude Code

**Draft. Results from 2026-10-16.**

Many MCP proxies claim 85–99% token savings. Those figures compare against
loading every tool schema up front. Claude Code already defers MCP tools by
default with its built-in tool search, so the question that matters to a
Claude Code user is different: compared with plugging the servers in directly,
does a proxy reduce the tokens a real task consumes, and does the task still
get done?

This benchmark measures exactly that. It runs headless Claude Code against the
same 441-tool fleet, once through each proxy and once connected directly.

## Setup

- **Client:** Claude Code `claude -p`, model Sonnet, `--strict-mcp-config`, 2 runs
  per task and setup, order shuffled.
- **Fleet:** 12 fake MCP servers (11 services × 40 tools, plus a GitHub-style
  `list_issues` that returns 20 realistic REST issues, 55 KB of pretty-printed
  JSON). The fake servers log every call, and a run counts as correct only if
  the log shows the work was done, not just if Claude's reply looks right.
- **Proxies:** each one runs with its default settings and fronts the same
  12 servers.
- **Tokens:** total input tokens per session (uncached + cache writes + cache
  reads), the amount that counts against usage.

| Task | What it needs |
|---|---|
| plain | "What is 2+2?" No tool call; measures fixed per-request overhead |
| single | Create one Jira issue |
| github | List 20 issues (55 KB result), report which are bugs |
| multi | List GitHub bugs, then create a Jira issue for each (1 + 6 calls) |

## Results

Change in tokens versus direct MCP, with correct runs out of 2:

| Setup | plain | single | github | multi | Correct (MCP tasks) |
|---|---|---|---|---|---|
| direct (built-in tool search) | 33.8k | 102.6k | 171.6k | 252.6k | 6/6 |
| **mcp-router (this repo)** | **−15%** | **−13%** | **−44%** | **−30%** | **6/6** |
| KGT24k/mcp-tool-search | −17% | +76% | +63% | +87% | 6/6 |
| maltejk/lazy-mcp | −17% | +34% | +50% | +73% | 6/6 |
| RaiAnsar/mcp-gateway | −16% | +77% | +62% | +78% | 6/6 |
| abdullah1854/MCPGateway | −15% | +138% | +152% | gave up (0/2) | 4/6 |
| dhruv1n30/mcp-tool-router | −17% | failed (0/2) | failed (0/2) | failed (0/2) | 0/6 |
| kira-autonoma/mcp-context-proxy | −0% | wrong server (0/2) | truncated (0/2) | truncated (0/2) | 0/6 |

Per-task tables with turns and wall time are in
[`bench/results/report.md`](bench/results/report.md), and raw runs are in
[`bench/results/results-final.jsonl`](bench/results/results-final.jsonl).

## Findings

1. **Every proxy cuts the fixed overhead, but that's the easy part.** Hiding
   441 tool names saves about 5k tokens per request (−15–17% on `plain`).
2. **Most proxies cost more tokens than they save once a tool is called:
   +34% to +152%.** The extra discovery round trips (search, fetch the schema,
   call) each re-read the whole context, which is about 28k tokens or more.
   The four other working proxies needed 1.5–2.5× the turns of direct MCP.
3. **The proxy's own tools get deferred too.** Claude Code defers MCP tools
   behind its ToolSearch, including the proxy's meta-tools, which adds a turn
   before every routed call. None of the proxies tested set
   `_meta["anthropic/alwaysLoad"]`. mcp-router sets it on its meta-tools. In our
   earlier test, that change plus a "call it now" hint in search results took
   routed tasks from 5 turns to 3.
4. **Response size matters more than tool-list size.** Claude Code saves MCP
   results over 50,000 characters to a file that Claude then has to read back.
   mcp-router strips noise from JSON results (`*_url` fields, nulls, empty
   values, pretty-print whitespace), so the 55 KB result shrank to 18 KB and stayed
   inline: −44% on `github` and 2 fewer turns. The other proxies passed the result
   through unchanged or made it bigger.
5. **Shrinking results by truncating them breaks tasks.** mcp-context-proxy keeps
   the first 3 array items by default, so Claude never saw most issues. It also
   routes duplicate tool names last-wins, which sent "create Jira issue" to
   Sentry.
6. **Code mode helped the multi-step task.** mcp-router's `run_script` took the
   GitHub → 6 Jira task from 12 turns to 6. MCPGateway also has code execution,
   but in this run Claude gave up on that task.

## Caveats

- The fleet is synthetic. Results depend on the shape of real tools and real
  responses.
- With 2 runs per cell, differences under about 10% are noise.
- Each proxy ran with its defaults. Some have options, such as semantic search,
  summarization or lite modes, that could change their results.
- **dhruv1n30/mcp-tool-router:** its search returns results over plain JSON-RPC,
  but returned `[]` inside Claude Code every time. The cause wasn't identified,
  so it may be a setup problem on our side.
- **RaiAnsar/mcp-gateway** needed `mcp<2` pinned to start.
- **MCPGateway** search returns nothing for multi-word queries.
- mcp-router is written by the author of this benchmark. The harness, fake
  servers and raw data are included so anyone can check the numbers.

## Reproduce

```sh
cd bench
python3 make_fleet.py /path/to/mcp-router
# add fleet-<name>.json for any proxy you want to compare (Claude Code mcpServers format)
python3 mcp_bench.py "$PWD" 2 direct ours <name> ...
python3 report.py results-final.jsonl direct,ours,<name>
```
