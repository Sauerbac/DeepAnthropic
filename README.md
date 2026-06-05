# cc-router

A ~250-line local reverse proxy that fronts **one blended Claude Code session** and routes
each request by its `model` field:

| Model in request | Upstream | Auth | Sent as |
|---|---|---|---|
| `…opus…` | `api.anthropic.com` (your Pro plan) | `Bearer sk-ant-oat01…` + `anthropic-beta: oauth…` | unchanged |
| `…sonnet…` (and default) | `api.deepseek.com/anthropic` | `x-api-key` | `deepseek-v4-pro` |
| `…haiku…` (small/fast slot) | `api.deepseek.com/anthropic` | `x-api-key` | `deepseek-v4-flash` |

Both upstreams speak the Anthropic wire format, so there is **no body translation** — the proxy
only swaps the auth header, rewrites the model name for DeepSeek, and streams the SSE response
straight back.

## Setup

1. **Get your Opus token** (one-time):
   ```powershell
   claude setup-token
   ```
   Copy the `sk-ant-oat01-…` value.

2. **Configure**:
   ```powershell
   Copy-Item config.toml.example config.toml
   ```
   Edit `config.toml` → set `anthropic.oauth_token` and `deepseek.api_key`.

3. **Run**:
   ```powershell
   ./start.ps1
   ```
   This builds (first run), starts the router, and launches Claude Code pointed at it.

## Daily use

- Default tier is **Sonnet → `deepseek-v4-pro`** (cheap, no quota worry).
- Type `/model opus` to escalate to your **real Pro-plan Opus**; `/model sonnet` to drop back.
- The small/fast background slot (Haiku) auto-routes to `deepseek-v4-flash`.
- No restart needed when switching — routing follows the model string live.
- When Pro Opus quota runs dry (~45 msgs / 5h), just `/model sonnet` and keep going.

## Manual launch (without start.ps1)

```powershell
./target/release/cc-router.exe        # terminal 1 (reads ./config.toml)

$env:ANTHROPIC_BASE_URL  = "http://127.0.0.1:8788"   # terminal 2
$env:ANTHROPIC_AUTH_TOKEN = "dummy-local-token"
claude
```

## Notes & caveats

- **ToS**: a proxy sits in front of your subscription-OAuth traffic. Informed choice.
- `config.toml` holds two secrets and is gitignored — keep it that way.
- The `oauth_beta` value in config is the first knob to check if Opus passthrough ever returns 401.
- DeepSeek's Anthropic endpoint ignores `disable_parallel_tool_use`; harmless for Claude Code.
- The setup-token is long-lived/self-refreshing; regenerate ~yearly if Opus starts 401ing.

## Smoke test

With placeholder tokens, all three tiers should reach their upstream and return `401`
(proving routing + plumbing). The router log prints the routing decision per request:

```
POST claude-opus-4-6   -> anthropic:opus-passthrough
POST claude-sonnet-4-6 -> deepseek-v4-pro
POST claude-haiku-4-5  -> deepseek-v4-flash
```
