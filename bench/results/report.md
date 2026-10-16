
### plain

| setup | tokens (mean) | vs direct | turns | time s | correct |
|---|---|---|---|---|---|
| direct | 33.8k | — | 1.0 | 3 | 2/2 |
| ours | 28.8k | -15% | 1.0 | 3 | 2/2 |
| tool-search | 28.2k | -17% | 1.0 | 6 | 2/2 |
| tool-router | 28.2k | -17% | 1.0 | 7 | 2/2 |
| lazy-mcp | 28.2k | -17% | 1.0 | 8 | 2/2 |
| rai-gateway | 28.2k | -16% | 1.0 | 4 | 2/2 |
| mcpgateway | 28.8k | -15% | 1.0 | 6 | 2/2 |
| context-proxy | 33.7k | -0% | 1.0 | 3 | 2/2 |

### single

| setup | tokens (mean) | vs direct | turns | time s | correct |
|---|---|---|---|---|---|
| direct | 102.6k | — | 3.0 | 7 | 2/2 |
| ours | 88.8k | -13% | 3.0 | 6 | 2/2 |
| tool-search | 180.8k | +76% | 6.0 | 14 | 2/2 |
| tool-router | 128.4k | +25% | 5.0 | 15 | 0/2 |
| lazy-mcp | 137.1k | +34% | 4.5 | 14 | 2/2 |
| rai-gateway | 182.0k | +77% | 6.0 | 10 | 2/2 |
| mcpgateway | 244.4k | +138% | 7.0 | 16 | 2/2 |
| context-proxy | 101.9k | -1% | 3.0 | 8 | 0/2 |

### github

| setup | tokens (mean) | vs direct | turns | time s | correct |
|---|---|---|---|---|---|
| direct | 171.6k | — | 4.5 | 12 | 2/2 |
| ours | 95.5k | -44% | 3.0 | 7 | 2/2 |
| tool-search | 280.5k | +63% | 8.0 | 22 | 2/2 |
| tool-router | 115.0k | -33% | 4.5 | 14 | 0/2 |
| lazy-mcp | 257.8k | +50% | 7.5 | 27 | 2/2 |
| rai-gateway | 278.1k | +62% | 9.0 | 21 | 2/2 |
| mcpgateway | 432.7k | +152% | 12.0 | 27 | 2/2 |
| context-proxy | 169.8k | -1% | 4.5 | 13 | 0/2 |

### multi

| setup | tokens (mean) | vs direct | turns | time s | correct |
|---|---|---|---|---|---|
| direct | 252.6k | — | 12.0 | 21 | 2/2 |
| ours | 176.9k | -30% | 6.0 | 12 | 2/2 |
| tool-search | 472.8k | +87% | 18.0 | 34 | 2/2 |
| tool-router | 112.4k | -56% | 5.5 | 18 | 0/2 |
| lazy-mcp | 435.8k | +73% | 15.5 | 35 | 2/2 |
| rai-gateway | 448.6k | +78% | 18.5 | 37 | 2/2 |
| mcpgateway | 79.2k | -69% | 3.5 | 12 | 0/2 |
| context-proxy | 210.3k | -17% | 6.5 | 19 | 0/2 |
