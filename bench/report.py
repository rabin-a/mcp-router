"""Markdown tables from results-final.jsonl: per task, each config's mean tokens/turns/time and pass rate."""
import json, sys, collections, statistics as st
rows = [json.loads(l) for l in open(sys.argv[1])]
order = sys.argv[2].split(",") if len(sys.argv) > 2 else sorted({r["cfg"] for r in rows})
g = collections.defaultdict(list)
for r in rows: g[(r["task"], r["cfg"])].append(r)
for task in ["plain", "single", "github", "multi"]:
    base = g.get((task, "direct"))
    base_tok = st.mean(r["ctx_in"] for r in base) if base else None
    print(f"\n### {task}\n\n| setup | tokens (mean) | vs direct | turns | time s | correct |\n|---|---|---|---|---|---|")
    for cfg in order:
        rs = g.get((task, cfg))
        if not rs: continue
        tok = st.mean(r["ctx_in"] for r in rs)
        rel = f"{(tok / base_tok - 1) * 100:+.0f}%" if base_tok and cfg != "direct" else "—"
        print(f"| {cfg} | {tok/1000:.1f}k | {rel} | {st.mean(r['turns'] or 0 for r in rs):.1f} | {st.mean(r['wall_s'] for r in rs):.0f} | {sum(r['correct'] for r in rs)}/{len(rs)} |")
