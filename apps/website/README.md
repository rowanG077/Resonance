# Resonance website

A Rust server with HTML templates, SQLite accounts, and ZIP storage. It serves
project downloads and a mod catalogue. The frontend is embedded in the binary.

## Development

From the repository root:

```sh
nix develop .#website
cargo run --locked --manifest-path apps/website/Cargo.toml
```

Open <http://127.0.0.1:3000>. Data lives in `apps/website/data/`.
Restart after editing templates or static files. Without Nix, install Rust and a C compiler.

## Configuration

| Environment variable | Default |
| --- | --- |
| `RESONANCE_WEB_LISTEN` | `127.0.0.1:3000` |
| `RESONANCE_WEB_URL` | `http://127.0.0.1:3000` (public origin, no trailing slash) |
| `RESONANCE_WEB_DATA` | `apps/website/data` in the source tree |
| `RESONANCE_WEB_REGISTRATION` | `false`; set `true` to allow sign-ups |
| `RESONANCE_WEB_TRUSTED_PROXIES` | Empty; comma-separated proxy IPs |
| `RESONANCE_WEB_DISK_RESERVE_MIB` | `2048` |

Use `RUST_LOG` to adjust logging.

## Deployment

Build on the Linux server, then install the [systemd service](deploy/resonance-website.service):

```sh
cargo build --release --locked --manifest-path apps/website/Cargo.toml
sudo useradd --system --user-group --home-dir /var/lib/resonance-website --no-create-home --shell /usr/sbin/nologin resonance-website
sudo install -D -m 0755 apps/website/target/release/resonance-website /opt/resonance-website/resonance-website
sudo install -m 0644 apps/website/deploy/resonance-website.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now resonance-website
```

Skip `useradd` on subsequent deployments and restart the service after replacing
the binary. Check `/healthz` or `journalctl -u resonance-website`.
The service stores data in `/var/lib/resonance-website`; back up that whole directory
with the service stopped.

The service trusts local Caddy. Terminate HTTPS there and overwrite the client-IP
header used for login rate limiting:

```caddyfile
resonance-project.net {
    reverse_proxy 127.0.0.1:3000 {
        header_up X-Resonance-Client-IP {client_ip}
    }
}
```

Behind a CDN, configure Caddy's trusted proxy ranges and `trusted_proxies_strict`
so `{client_ip}` resolves to the visitor. Change the public URL in the service for another domain.

## Administration

Create the first administrator on the server:

```sh
systemd-ask-password "Admin password (10–128 bytes):" | sudo -u resonance-website env RESONANCE_WEB_DATA=/var/lib/resonance-website /opt/resonance-website/resonance-website create-admin USERNAME
```

Sign in and use `/admin` to approve upload access and releases, set account limits,
manage administrators, or remove mods. The CLI also supports `list-accounts`,
`grant-admin USERNAME`, and `revoke-admin USERNAME`. There is no password recovery.

Authors upload ZIPs and can resume interrupted uploads by reselecting the file.
Defaults are 64 MiB per ZIP, 256 MiB expanded, 512 MiB stored, and 2048 MiB uploaded
per day. Administrators change these directly on the account form. ZIP contents
are opaque; package manifest validation is deferred until its format is defined.

Public JSON: `/api/v1/mods` and `/api/v1/mods/{id}`.
ZIP downloads: `/api/v1/releases/{id}/download`.

## Checks

```sh
cargo fmt --manifest-path apps/website/Cargo.toml --check
cargo test --locked --manifest-path apps/website/Cargo.toml
```
