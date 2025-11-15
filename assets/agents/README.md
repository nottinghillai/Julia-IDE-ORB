# Agent Registry

This directory contains the agent registry system for Julia IDE. Agents are organized into two tiers:

## Source of Truth (Repository)

**Location**: `assets/agents/builtin/`

- Lives in the repository, version-controlled
- Contains built-in agents: `gemini/`, `claude-code/`, `codex/`, `native/`
- Each folder must contain:
  - `agent.toml` (required) - Agent metadata and configuration
  - `system_prompt.md` (optional) - System prompt for the agent
- Never edited by users - read-only reference

## Runtime Storage (User Data)

**Location**: `{data_dir}/agents/`

- Data directory resolution (platform-specific):
  - macOS: `~/Library/Application Support/Julia/agents/`
  - Linux/FreeBSD: `$XDG_DATA_HOME/zed/agents/` (or `~/.local/share/zed/agents/`)
  - Windows: `%LOCALAPPDATA%\Julia\agents\`
- Structure:
  - `index.json` - Manifest tracking source, version, checksums, user modifications
  - `builtin/` - Copied from `assets/agents/builtin/`
  - `custom/` - User-defined agents

## Agent Configuration

### agent.toml

Required file for each agent. Example structure:

```toml
[agent]
id = "gemini"
name = "Gemini CLI"
type = "builtin"
version = "1.0.0"
description = "Google Gemini command-line agent"

[agent.metadata]
icon = "ai-gemini"
telemetry_id = "gemini-cli"
```

### system_prompt.md

Optional markdown file containing the system prompt for the agent.

## Migration and Seeding

On first run:
1. Check if `{data_dir}/agents/index.json` exists
2. If missing, copy all from `assets/agents/builtin/` to `{data_dir}/agents/builtin/`
3. Generate `index.json` with checksums and metadata

On each startup:
1. Compute checksums of files in `assets/agents/builtin/`
2. Compare with checksums in manifest
3. If checksum differs:
   - If `user_modified = false`: Update from assets, update checksum
   - If `user_modified = true`: Skip (preserve user edits), log warning

## ZED_STATELESS Mode

In stateless mode:
- Skip manifest writes
- Skip folder sync
- Log warning: "Agent registry disabled in stateless mode"

## Agent Identification

- Built-in agents: Use enum variant name (e.g., "gemini", "claude_code", "codex", "native")
- Custom agents: Use the `name` field from `ExternalAgent::Custom`

