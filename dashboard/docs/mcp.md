---
title: MCP server
description: Connect Claude Code, Cursor and other AI agents to your fleet.
---

Smith runs a remote [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server. Connect an AI agent to it to look up devices, ship releases, follow deployments and troubleshoot devices by asking in plain language.

The agent signs in with your Smith account, so it can only do what your role allows. Your server URL is:

```text
{{MCP_URL}}
```

## Claude Code

Add the Smith MCP server:

```bash
claude mcp add --transport http --scope user smith {{MCP_URL}}
```

`--scope user` makes Smith available in Claude Code wherever you run it. Without it, Claude Code only adds the server for the directory you ran the command in.

Then sign in. Start Claude Code, run `/mcp`, select **smith** and choose **Authenticate**. Your browser opens the Smith login page, and the tools are ready once you sign in.

To check the connection, ask Claude something like "Which devices are offline?".

## Cursor

[Install in Cursor]({{CURSOR_INSTALL_URL}})

Select **Install in Cursor** to add the server. To configure it manually, add the following to `~/.cursor/mcp.json`:

```json title="~/.cursor/mcp.json"
{
  "mcpServers": {
    "smith": {
      "url": "{{MCP_URL}}"
    }
  }
}
```

Cursor asks you to sign in the first time it connects. See [Cursor's MCP documentation](https://docs.cursor.com/context/model-context-protocol) for more.

## VS Code

[Install in VS Code]({{VSCODE_INSTALL_URL}})

Select **Install in VS Code** to add the server. To configure it manually, add the following to `.vscode/mcp.json` in your workspace:

```json title=".vscode/mcp.json"
{
  "servers": {
    "smith": {
      "type": "http",
      "url": "{{MCP_URL}}"
    }
  }
}
```

See the [VS Code MCP documentation](https://code.visualstudio.com/docs/copilot/chat/mcp-servers) for more.

## Claude Desktop and claude.ai

Go to **Settings → Connectors**, select **Add custom connector**, and enter the server URL. Claude asks you to sign in when you connect.

Custom connectors connect from Anthropic's cloud, so the API has to be reachable from the internet. A `localhost` server only works with local clients such as Claude Code.

## Codex

Add the server to `~/.codex/config.toml`:

```toml title="~/.codex/config.toml"
[mcp_servers.smith]
url = "{{MCP_URL}}"
```

Then sign in:

```bash
codex mcp login smith
```

## Other clients

MCP is an open protocol, so any client that supports remote (streamable HTTP) servers with OAuth works. Use the server URL above; the client finds the Smith login page by itself.

## Manage access

Signing in over MCP uses the same Smith account and role as the dashboard. To disconnect a client, remove the server from it, for example `claude mcp remove smith`.

Auth0 administrators can revoke a client's access for a user under **User Management → Users → Authorized Applications** in the Auth0 dashboard.

## Tools

The server exposes the following tools.

| Area | Tool | Description |
|---|---|---|
| **Devices** | `list_devices` | List devices, filtered by labels, online state, release or search terms |
| | `get_device` | Full details for one device |
| **Releases** | `list_distributions` | List distributions |
| | `list_releases` | List releases, newest first |
| | `get_release` | A release with its packages and deployment status |
| | `draft_release` | Draft a release by bumping the version of an existing one (patch, minor or major, optionally as a release candidate) |
| | `publish_release` | Publish a draft release |
| | `set_release_lts` | Mark or unmark a release as LTS |
| | `yank_release` | Withdraw a release |
| | `promote_release_candidate` | Turn a release candidate into a regular release |
| **Deployments** | `deploy_release` | Deploy a release to canary devices, chosen by labels or ids |
| | `get_deployment` | Canary progress and unhealthy services for a deployment |
| | `confirm_full_rollout` | Roll a deployment out to the whole fleet |
| **Troubleshooting** | `get_device_logs` | Fetch journal logs from a device |
| | `get_service_status` | `systemctl status` for a unit on devices |
| | `restart_service` | `systemctl restart` for a unit on devices |
| | `restart_devices` | Reboot devices |
| | `get_command_results` | Results of queued device commands |
| **Recipes** | `list_recipes` | List saved command recipes |
| | `trigger_recipe` | Run a saved recipe against devices |

Permissions match the dashboard. For example, `get_service_status` and `restart_service` run shell commands on the device, so they need the `commands:freeform` permission. Tunnels, file access, free-form commands and deleting devices are not available over MCP.

### Things to ask

- "Which devices are offline?"
- "Draft a patch release, publish it and deploy it to the canary devices."
- "How is the latest deployment going? Roll it out to everyone if the canaries are healthy."
- "Get the last hour of `smithd` logs from this device."

## Confirm actions

Tools that change your fleet, such as `deploy_release`, `confirm_full_rollout`, `yank_release`, the restart tools and `trigger_recipe`, are marked as destructive, so clients ask for your confirmation before running them. Keep those confirmations on.

Be careful when the same agent also reads untrusted content, such as device logs, web pages or other MCP servers. Text in them can try to steer the agent into running tools you didn't ask for (prompt injection).

## Set up the server

This section is for administrators running Smith. The MCP server uses the same Auth0 API as the dashboard (`AUTH0_AUDIENCE`), so there is nothing to deploy or configure in Smith itself. `AUTH0_AUDIENCE` has to be the public URL of the API, for example `https://api.example.com`; otherwise the MCP server stays off.

MCP clients log in differently from the dashboard, so the Auth0 tenant needs a one-time change:

1. **In Auth0, go to Settings → Advanced** and enable:
   - **Resource Parameter Compatibility Profile**, because MCP clients identify the API with the `resource` parameter instead of `audience`.
   - **Include Issuer in Authorization Responses**.
   - **Client ID Metadata Document registration**, so MCP clients such as Claude Code register themselves instead of needing an Auth0 application each. If a client fails at registration, also enable **OIDC Dynamic Application Registration**.
2. **Go to Authentication**, open the connection your team logs in with, and enable **Promote Connection to Domain Level**, so the login page offers it to MCP clients.

The API serves the OAuth protected resource metadata at `/.well-known/oauth-protected-resource`, which points clients at `AUTH0_ISSUER`.
