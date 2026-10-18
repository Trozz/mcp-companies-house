# TLS and public access

This page explains how the server chooses its certificate, what its self-signed certificate is good for, when it sends HSTS, and three ways to give it a public HTTPS address: a tunnel, a reverse proxy, or certificate files on the server itself. It ends with a trusted certificate for local use.

## Why claude.ai needs a publicly trusted certificate

claude.ai, Claude Desktop and the Claude mobile apps connect to custom connectors from Anthropic's servers, not from your machine. So for them the server must be reachable from the internet over HTTPS, with a certificate from a public certificate authority, such as Let's Encrypt. They do not accept a self-signed certificate or one from a private authority such as mkcert's.

The sign-in page is opened in the person's own browser at the same address, so it needs the same public certificate.

Claude Code and other clients on your own machine can also use a certificate you trust yourself (see [A trusted local certificate](#a-trusted-local-certificate)).

To check from outside, run this on another machine, without `-k`:

```bash
curl https://companies-house.example.com/healthz
```

It prints `ok` only when the address is reachable and its certificate is trusted.

## How the server picks its certificate

At startup, in this order:

1. **`INSECURE_HTTP=true`**: plain HTTP, no TLS, whatever else is set. The server logs a warning, `INSECURE_HTTP=true: serving plaintext HTTP without TLS`. Use it only when TLS is terminated in front of the server on a network you trust.
2. **`TLS_CERT_PATH` and `TLS_KEY_PATH` both set**: the server loads the PEM certificate chain and private key and logs `serving HTTPS with certificate from files`. If either file is missing or invalid, it stops with `error: cannot load TLS certificate <cert> / key <key>: ...`; it never falls back to a self-signed certificate. Setting only one of the two stops startup with `TLS_CERT_PATH and TLS_KEY_PATH must be set together`.
3. **Neither set**: the server generates a self-signed certificate (see below).

With certificate files, the server checks the files' modification times every 30 seconds. When they change, for example after a renewal, it loads them again and logs `tls certificate reloaded`, without a restart. If the new files cannot be loaded, it keeps the previous certificate and logs `tls certificate reload failed, keeping previous certificate`.

## The self-signed certificate

- It is generated in memory at startup for the `PUBLIC_URL` host, `localhost`, `127.0.0.1` and `::1`.
- It is never written to disk, and a new one is made on every restart.
- The startup log warns `no TLS_CERT_PATH/TLS_KEY_PATH set: serving HTTPS with an in-memory self-signed certificate (not trusted by claude.ai)`, with its SHA-256 fingerprint (`fingerprint_sha256`, colon-separated hex) and its names (`sans`).

It is good for:

- Trying the server on your own machine, with `curl -k`.
- Encrypting the hop from a tunnel or reverse proxy in front of the server that does not verify the certificate. A proxy can pin the logged fingerprint instead, but the fingerprint changes on every restart; use certificate files if you want to pin.

It does not work for claude.ai directly, and it is awkward for Claude Code, which would have to trust a new certificate after every restart. For Claude Code on your own machine, use [a trusted local certificate](#a-trusted-local-certificate).

## HSTS

- With certificate files, every response carries `Strict-Transport-Security: max-age=31536000` (one year). Browsers that have seen it then use only HTTPS for that host.
- With the self-signed certificate or plain HTTP, the server sends no HSTS header, so the tunnel or proxy in front sets its own policy.
- Every response, in every mode, carries `X-Content-Type-Options: nosniff`.

## Option A: a Cloudflare tunnel

The simplest way to get a trusted public address without opening a port. The tunnel's edge certificate is trusted by claude.ai, and the server's self-signed certificate encrypts the hop from the tunnel to the server.

- [Getting started](getting-started.md#with-docker-compose-and-a-cloudflare-tunnel) has a complete Docker Compose example.
- In the tunnel's public hostname settings, point the service to `https://mcp-companies-house:8443` and turn on **No TLS Verify**, because the server's certificate is self-signed.
- Set `PUBLIC_URL` to the tunnel's hostname, for example `https://companies-house.example.com`.
- Give the tunnel container a fixed address and put it in `TRUSTED_PROXIES`. The server then takes each person's address from the `X-Forwarded-For` header of the tunnel's requests, when the tunnel sends one; a request without it counts as coming from the tunnel itself (see [Security](security.md#limits-on-key-checks)).

## Option B: a reverse proxy

Run the server behind nginx, Caddy, Traefik or another proxy that holds a trusted certificate.

- **Upstream.** Proxy to `https://<server>:8443` with certificate verification off, or set `INSECURE_HTTP=true` on the server if the proxy and the server share a network you trust, and proxy to `http://<server>:8443`.
- **Keep the `Host` header.** `/companies` and `/mcp` accept only requests for the `PUBLIC_URL` host, `localhost`, `127.0.0.1` and `::1`, to block DNS rebinding. A proxy that rewrites `Host` to an internal name gets `403 Forbidden: Host header is not allowed`.
- **Do not buffer responses.** MCP's Streamable HTTP transport can answer with a stream of events, which a buffering proxy holds back.
- **Pass the client's address.** Add `X-Forwarded-For` and set `TRUSTED_PROXIES` to the proxy's address. The server then uses the right-most address in `X-Forwarded-For` that is not a trusted proxy. Without it, every request seems to come from the proxy, and a few mistyped keys can hold up everyone's sign-in for a minute.
- **Body size.** The server accepts request bodies up to 1 MiB and answers larger ones with `413`.

An nginx example, with nginx and the server binary on the same machine:

```nginx
server {
    listen 443 ssl;
    server_name companies-house.example.com;
    ssl_certificate     /etc/letsencrypt/live/companies-house.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/companies-house.example.com/privkey.pem;

    location / {
        proxy_pass https://127.0.0.1:8443;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_buffering off;
    }
}
```

with `PUBLIC_URL=https://companies-house.example.com` and `TRUSTED_PROXIES=127.0.0.1` on the server. If you add request rate limits at the proxy, read [Request rate limits](operations.md#request-rate-limits) first: limiting by client address on the MCP endpoints would block claude.ai users.

## Option C: the server holds the certificate

Give the server a certificate from Let's Encrypt or another public authority:

```bash
docker run -d --name mcp-companies-house -p 443:8443 \
  -e PUBLIC_URL=https://companies-house.example.com \
  -e OAUTH_SIGNING_KEY_FILE=/run/secrets/oauth_signing_key \
  -v /path/to/secrets/oauth_signing_key:/run/secrets/oauth_signing_key:ro \
  -v /path/to/certs:/certs:ro \
  -e TLS_CERT_PATH=/certs/fullchain.pem \
  -e TLS_KEY_PATH=/certs/privkey.pem \
  ghcr.io/trozz/mcp-companies-house:latest
```

- The container listens on 8443; `-p 443:8443` publishes it on the standard HTTPS port, so `PUBLIC_URL` needs no port.
- The files must be readable by UID 65532, the user the container runs as. If your certificate tool keeps the key readable by its owner only, copy the files to a directory the container can read after each renewal.
- If the paths are symbolic links, as in a Let's Encrypt `live` directory, the links must resolve inside the container too. Mount the whole `/etc/letsencrypt` directory, for example at `/etc/letsencrypt`, and point `TLS_CERT_PATH` and `TLS_KEY_PATH` at the files in its `live` directory.
- Renewals are picked up within 30 seconds, without a restart.
- With certificate files, the server sends HSTS (see [HSTS](#hsts)).

## A trusted local certificate

For Claude Code or testing on your own machine, [mkcert](https://github.com/FiloSottile/mkcert) makes a certificate your machine trusts:

```bash
mkcert -install
mkcert -cert-file cert.pem -key-file key.pem localhost 127.0.0.1 ::1
PUBLIC_URL=https://localhost:8443 \
OAUTH_SIGNING_KEY="$OAUTH_SIGNING_KEY" \
TLS_CERT_PATH=$PWD/cert.pem TLS_KEY_PATH=$PWD/key.pem \
  ./mcp-companies-house
```

Then connect Claude Code with your own key:

```bash
claude mcp add --transport http ch https://localhost:8443/mcp \
  --header "Authorization: Bearer $COMPANIES_HOUSE_API_KEY"
```

`mkcert -install` adds mkcert's authority to your system's trust store. Clients built on Node.js may not read that store; point `NODE_EXTRA_CA_CERTS` at the `rootCA.pem` file in the directory `mkcert -CAROOT` prints. A certificate from mkcert works only on machines that trust its authority, so it does not work for claude.ai.

Next: [Security](security.md).
