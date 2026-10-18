# Connecting clients

This page shows how to add the server to claude.ai, Claude Desktop, the Claude mobile apps, Claude Code and other MCP clients, and how to sign out or change your key. Everyone connects with their own Companies House REST API key (see [Getting started](getting-started.md#1-get-a-companies-house-api-key)), either on the server's sign-in page or as the bearer token.

| Endpoint | Connector URL |
|---|---|
| Companies | `https://companies-house.example.com/companies` |
| All services | `https://companies-house.example.com/mcp` |

Add one connector for `/mcp`, or one per service. They offer the same tools and prompts; on `/mcp` each name starts with its service, such as `companies_get_company` (see [Combined endpoint](combined.md)). Use one or the other, not both: with both, every tool appears twice.

## claude.ai, Claude Desktop and Claude mobile

These connect from Anthropic's servers, so the server must be reachable from the internet with a publicly trusted certificate (see [TLS and public access](tls.md)).

1. In claude.ai open **Settings**, then **Connectors**, then **Add custom connector**.
2. Enter the connector URL, for example `https://companies-house.example.com/mcp`.
3. A sign-in page from your server opens. It says which service you are connecting to (for `/mcp`, every service on the server), the client's name as the client gave it, and where you will be sent afterwards.
4. Enter your own Companies House REST API key and choose **Authorize**.

The server checks the key with one request to Companies House, then seals it into the connector's OAuth tokens; it stores nothing. Access tokens last one hour and refresh tokens 30 days, and the client refreshes its access token itself. Every sign-in ends 90 days after it was made, however often it was refreshed, and the connector then asks you to connect again. Connectors added in claude.ai are also available in Claude Desktop and the Claude mobile apps.

If the sign-in page does not accept the key, it says why:

| Message | Meaning |
|---|---|
| That is not a Companies House API key. | The text cannot be a key (keys are 8 to 64 letters, digits, dashes and underscores). It was not sent to Companies House |
| That Companies House API key was not accepted. | Companies House rejected it: a mistyped key, a stream key, or a key for the other environment (a test application's key on a live server, or the other way round) |
| Companies House refused your API key from this server (HTTP 403). Check the key's IP or domain restrictions in the Developer Hub | The key is restricted to addresses that do not include the server |
| Your Companies House API key could not be checked right now: ... | Your key's rate limit is used up, or Companies House did not answer. The rest of the message says which. Try again later |

After 5 rejected attempts within a minute from one address, the server refuses further attempts from it until those rejections are more than a minute old. [Troubleshooting](troubleshooting.md#the-sign-in-page-refuses) lists every message the page can show, with its fix, and [Security](security.md#limits-on-key-checks) explains the limits.

## Claude Code

Claude Code can send a header, so it can use your key directly as the bearer token:

```bash
claude mcp add --transport http ch https://companies-house.example.com/mcp \
  --header "Authorization: Bearer $COMPANIES_HOUSE_API_KEY"
```

or for the service's own endpoint:

```bash
claude mcp add --transport http companies-house https://companies-house.example.com/companies \
  --header "Authorization: Bearer $COMPANIES_HOUSE_API_KEY"
```

`$COMPANIES_HOUSE_API_KEY` is your own key on your own machine. The shell puts the key's value into the command, so Claude Code is given the key itself, not the variable's name. The server checks the key with Companies House the first time it sees it and remembers the answer for up to 10 minutes (1 minute for a rejected key). Every Companies House request still uses the key, so a key you revoke at Companies House stops working at once.

Without `--header`, Claude Code uses the sign-in page instead: add the server with only its URL, then run `/mcp` in Claude Code to sign in.

Claude Code also works against a local self-signed certificate if you trust it (see [TLS and public access](tls.md)).

## Other MCP clients

Any client that speaks the MCP Streamable HTTP transport works. It either completes the OAuth flow or sends `Authorization: Bearer <your-api-key>`.

- **OAuth.** The server supports dynamic client registration (public clients only) and the authorization code flow with PKCE (S256). A request without a token gets a `401` whose `WWW-Authenticate` header points to the endpoint's resource metadata, for example `https://companies-house.example.com/.well-known/oauth-protected-resource/companies`, where discovery starts. The authorization server's metadata is at `/.well-known/oauth-authorization-server`.
- **Tokens belong to one endpoint.** A token issued for `/companies` is refused on `/mcp`, and the other way round. A client names the endpoint it signs in for with the `resource` parameter. A client that does not, as with clients written before the June 2025 MCP authorization specification, is signed in for `/companies`, so point such a client at `/companies`.
- **Bearer key.** A key sent as the bearer token works on both endpoints. Only values shaped like a Companies House key are ever sent to Companies House; anything else is refused without being checked.

## Tool names and their length

The longest tool name is `list_persons_with_significant_control`, 37 characters, or 47 as `companies_list_persons_with_significant_control` on `/mcp`. Some clients and model providers limit a tool's full name, including any prefix the client adds, to 64 characters. Keep the name you give the server in your client short, such as `ch` or `companies-house`.

## Signing out or changing your key

- **Change your key:** remove or disconnect the connector in your client and add it again with the new key. In Claude Code, `claude mcp remove ch` and then add it again.
- **End every sign-in made with a key:** revoke or delete the key in the Developer Hub. Each request uses the key, so it stops working at once, and tools answer `auth: Companies House rejected your API key. Sign in again ...`.
- The server cannot sign out one client on its own. Whoever runs it can sign everyone out by changing `OAUTH_SIGNING_KEY`.

Each MCP session belongs to the key that opened it, and other people cannot use it. One key can hold up to 50 sessions on each endpoint. If you open more, for example in many windows or clients at once, your least recently used session is closed.

## Icons

The service sends its own icon in `serverInfo`, and `/favicon.ico` serves the project icon. Some clients, including claude.ai at the time of writing, show a generic icon for custom connectors regardless.

Next: [Using it](usage.md).
