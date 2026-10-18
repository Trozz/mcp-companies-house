# Getting started

This page takes you from nothing to a running server: a Companies House API key, a signing key for the server, the server itself in Docker or as a binary, and a check that it answers. You need a machine that can run Docker or a single binary. For claude.ai, the server also needs a public HTTPS address with a publicly trusted certificate (see [TLS and public access](tls.md)).

## 1. Get a Companies House API key

Everyone who uses the server needs a REST API key of their own. Keys are free. The server has no key of its own and needs none to start, but you need yours to connect a client.

1. Register at the [Companies House Developer Hub](https://developer.company-information.service.gov.uk/get-started/) and sign in.
2. Open **Your applications** and create an application. Choose **live** for a server on the real register, or **test** for a server on the [sandbox](#4-or-use-the-sandbox).
3. In the application choose **Create new key** and pick **REST**. Stream keys and web OAuth clients are for other Companies House APIs and do not work here.
4. If you restrict the key to IP addresses or domains, include the address the server calls Companies House from. Otherwise Companies House refuses the key with HTTP 403.

You can try the key against Companies House directly. This is the same request the server makes to check a key when someone signs in:

```bash
curl -s -o /dev/null -w '%{http_code}\n' -u "$COMPANIES_HOUSE_API_KEY:" \
  'https://api.company-information.service.gov.uk/search/companies?q=companies+house&items_per_page=2'
```

`200` means the key works on the live register, `401` that it was rejected (for example a key from a test application), and `403` that its IP or domain restrictions refused this machine. For a sandbox key, use the host `api-sandbox.company-information.service.gov.uk`.

## 2. Choose a signing key for the server

The server needs one secret of its own, `OAUTH_SIGNING_KEY`. It signs the OAuth tokens and encrypts the API keys sealed inside them. It must be at least 32 bytes. Generate it once:

```bash
openssl rand -hex 32
```

- Keep it stable. Changing it signs everyone out.
- Keep it secret. Anyone holding it could open the API keys sealed in tokens they had captured. If it ever leaks, change it and ask everyone to replace their Companies House key.
- Store it in a secret manager or in a file only you can read. Any setting can also be read from a file: append `_FILE` to its name, for example `OAUTH_SIGNING_KEY_FILE=/run/secrets/oauth_signing_key`. A value set directly wins over the file.

The only other required setting is `PUBLIC_URL`, the address clients use to reach the server, for example `https://companies-house.example.com`. It must be `https` (`http` is accepted only for `localhost`) and have no path.

There is no admin password and no shared token. Setting `MCP_AUTH_TOKEN` stops the server at startup with `MCP_AUTH_TOKEN cannot be used when people sign in with their own Companies House API key`.

## 3. Run it

### With Docker

```bash
docker run -d --name mcp-companies-house -p 8443:8443 \
  -e PUBLIC_URL=https://companies-house.example.com \
  -e OAUTH_SIGNING_KEY="$OAUTH_SIGNING_KEY" \
  -v /path/to/certs:/certs:ro \
  -e TLS_CERT_PATH=/certs/fullchain.pem \
  -e TLS_KEY_PATH=/certs/privkey.pem \
  ghcr.io/trozz/mcp-companies-house:latest
```

`$OAUTH_SIGNING_KEY` holds the value you generated in step 2. Do not generate a new one on every start, or each restart signs everyone out.

- The container runs as UID 65532, so the certificate files must be readable by that user. The server reloads them when they change.
- Without `TLS_CERT_PATH` and `TLS_KEY_PATH`, the server makes an in-memory self-signed certificate instead. That is fine for local use and behind a tunnel or proxy, but claude.ai cannot connect to it directly. [TLS and public access](tls.md) covers the options.
- The image has a built-in health check and listens on port 8443. [Releases](releases.md) lists its tags.

### With Docker Compose and a Cloudflare tunnel

A tunnel gives the server a public HTTPS address with a trusted certificate, without opening a port on your network. Create a tunnel in the Cloudflare dashboard, add a public hostname such as `companies-house.example.com` pointing to `https://mcp-companies-house:8443` with **No TLS Verify** turned on (the server's own certificate is self-signed in this setup), and copy the tunnel token.

`compose.yaml`:

```yaml
services:
  mcp-companies-house:
    image: ghcr.io/trozz/mcp-companies-house:latest
    container_name: mcp-companies-house
    restart: unless-stopped
    env_file: app.env
    ports:
      - "127.0.0.1:8443:8443"
    networks:
      - mcp
  tunnel:
    image: cloudflare/cloudflared:latest
    restart: unless-stopped
    command: tunnel --no-autoupdate run
    environment:
      TUNNEL_TOKEN: ${TUNNEL_TOKEN}
    networks:
      mcp:
        ipv4_address: 172.30.0.10
    depends_on:
      - mcp-companies-house
networks:
  mcp:
    ipam:
      config:
        - subnet: 172.30.0.0/24
```

`app.env` (readable only by you, for example `chmod 600 app.env`):

```bash
PUBLIC_URL=https://companies-house.example.com
OAUTH_SIGNING_KEY=replace-with-the-output-of-openssl-rand-hex-32
TRUSTED_PROXIES=172.30.0.10
```

Start both containers with `TUNNEL_TOKEN=<token> docker compose up -d`.

- Every request reaches the server from the tunnel container. Without `TRUSTED_PROXIES`, the sign-in limits would count everyone as one client, so five rejected keys within a minute would hold up everyone's sign-ins until that minute had passed. With the tunnel's fixed address in `TRUSTED_PROXIES`, the server takes each person's address from the `X-Forwarded-For` header of the tunnel's requests and limits each address separately. A request from the tunnel without that header counts as coming from the tunnel itself. See [Security](security.md#limits-on-key-checks).
- No volume is needed. The server stores nothing, and with every tool only reading there is nothing for an audit log to record.

### With the binary

Download the archive for your platform from [Releases](https://github.com/Trozz/mcp-companies-house/releases): Linux x86_64 and arm64 (static builds) and macOS on Apple silicon. Verify the checksum, extract it and run:

```bash
shasum -a 256 -c mcp-companies-house-<version>-<target>.tar.gz.sha256
tar -xzf mcp-companies-house-<version>-<target>.tar.gz
cd mcp-companies-house-<version>-<target>
PUBLIC_URL=https://companies-house.example.com OAUTH_SIGNING_KEY="$OAUTH_SIGNING_KEY" \
  ./mcp-companies-house
```

The targets are `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` and `aarch64-apple-darwin`. Each archive holds the binary, `README.md` and `LICENSE`.

- `./mcp-companies-house --version` prints the version.
- `./mcp-companies-house --healthcheck` connects to `127.0.0.1` on the `BIND_ADDR` port and exits 0 when the server answers `/healthz`. The container image uses it as its `HEALTHCHECK`. `BIND_ADDR` must therefore listen on loopback; the default `0.0.0.0:8443` does.
- `./mcp-companies-house --help` prints a short usage line.

All configuration comes from environment variables. The [configuration reference](configuration.md) lists every one.

## 4. Or use the sandbox

The Companies House sandbox lets you try the server without touching real data. Add one variable:

```bash
-e COMPANIES_HOUSE_API_URL=https://api-sandbox.company-information.service.gov.uk
```

Everyone then needs a key from a **test** application, and live keys are rejected. The sandbox holds test data, not the real register: you make test companies with the Companies House [test data generator](https://developer.company-information.service.gov.uk/api-testing). It has no filed documents, and some of its endpoints answer with errors. [On the sandbox](usage.md#on-the-sandbox) says what works there.

## 5. Check it is running

```bash
curl -k https://127.0.0.1:8443/healthz
```

prints `ok`. `/healthz` needs no sign-in. `-k` is needed while the server uses its self-signed certificate.

The startup log (JSON by default) shows what is running:

| Message | What it shows |
|---|---|
| `starting mcp-companies-house` | The version |
| `service enabled` | The `companies` service, its URL and its tools |
| `combined endpoint enabled` | The `/mcp` URL and the number of tools (26) |
| `companies house settings` | The Companies House API in use and the Document API (`none` on the sandbox) |
| `listening` | The address the server listens on |

Without certificate files there is also a warning that the server is using a self-signed certificate, with its SHA-256 fingerprint. If the configuration has problems, the server prints each one as `configuration error: ...` and exits.

From another machine, check that `PUBLIC_URL` reaches the server:

```bash
curl -s https://companies-house.example.com/.well-known/oauth-protected-resource/mcp
```

The answer names `https://companies-house.example.com/mcp` as its `resource`. A request to `/mcp` or `/companies` without a key is answered `401`, which is what clients expect before they sign in.

## 6. Choose what runs

- `companies` is the only service today, and it runs by default. `COMPANIES_HOUSE_SERVICES` selects services by name, separated by commas. An unknown name stops startup, and so does a list with no names in it, such as `,`. An empty value counts as unset.
- `/mcp` is served whenever a service runs. `COMPANIES_HOUSE_COMBINED=false` turns it off, and it then answers 404.

Next: [Connecting clients](connecting-clients.md).
