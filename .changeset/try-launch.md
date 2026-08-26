---
aimux: minor
---
New `aimux try <PROVIDER> [-- <cli args…>]`: trial-launch a provider without touching live configs. Each app gets a throwaway config directory selected through its official override env (CLAUDE_CONFIG_DIR / CODEX_HOME / OPENCODE_CONFIG / PI_CODING_AGENT_DIR), the real CLI runs attached to your terminal, and the temp dir is removed when it exits. Official rows report there is nothing to try; exit codes pass through.
