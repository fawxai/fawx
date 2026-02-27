# Health Check & Hardening: Costs and Risks

> Issue: [#248](https://github.com/abbudjoe/fawx/issues/248)  
> Audience: developers and operators running Fawx health checks

## Overview

Fawx health checks and hardening routines may invoke LLM API calls to validate configuration, test connectivity, and verify agent functionality. This document discloses the associated costs and risks.

---

## Token Costs

### What consumes tokens

| Check Type | API Calls | Estimated Tokens |
|---|---|---|
| API key validation | 1 chat request per provider | ~50-100 tokens |
| Agent smoke test | 1-2 chat requests | ~200-500 tokens |
| Tool execution test | 1 chat + 1-3 tool loop iterations | ~500-1500 tokens |
| Full health check suite | All of the above | ~1000-3000 tokens |

### Cost estimates (approximate)

At typical API pricing (verify current rates with your provider):

- **Single health check run:** $0.001 – $0.01 depending on model and provider
- **Automated periodic checks (hourly):** $0.02 – $0.25/day
- **One-time setup validation:** $0.01 – $0.05

These are estimates — actual costs depend on the configured model (Haiku/Sonnet/GPT-4o-mini/etc.), prompt length, and response complexity.

**Example:** 10 health checks per day with the cheapest action model (Haiku-class):
- ~100 tokens/check × 10 checks = 1,000 tokens/day
- At approximate Haiku-class pricing (~$0.25/MTok input, ~$1.25/MTok output — verify current rates with your provider)
- Estimated cost: ~$0.001–$0.002/day, or under $1/year

### What does NOT consume tokens

- Accessibility service status checks (local device query)
- ADB connectivity checks (local)
- Overlay permission checks (local)
- Network reachability pings (HTTP HEAD, no LLM)
- Local LLM checks (Ollama/llama.cpp — no API cost)

---

## Risks

### Rate limiting

Frequent health checks against cloud providers may trigger rate limits, especially on free-tier or low-quota accounts. If you run automated health checks:

- Space checks at least 5 minutes apart
- Use the cheapest available model (e.g., Haiku, GPT-4o-mini)
- Monitor your provider dashboard for usage spikes

### Key exposure during validation

Health check API calls use your configured API keys. The same security considerations apply as for normal Fawx usage:

- Keys are stored in Android Keystore (encrypted at rest)
- Validation requests are minimal (`"ping"` message)
- No user data is sent during health checks

### False positives

Health checks test basic connectivity and model availability. They do NOT guarantee:

- Correct model behavior for your use case
- Sufficient quota for extended usage
- Billing account health

---

## Recommendations

1. **Run health checks sparingly** — once at setup, then only when diagnosing issues
2. **Use the action model** (cheaper/faster) for validation, not the chat model
3. **Monitor provider dashboards** if running automated periodic checks
4. **Local LLM users** have zero API cost for health checks — prefer local validation when possible
