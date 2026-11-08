# lockbox

the pedant's stumbleupon (it's a link aggregator).

## Crates

> [!warning]
> Binary crates are currently not portable and are only expected to work on UNIX
> boxes. Library crates may or may not work on Windows.

| Crate | Kind | Role |
| --- | --- | --- |
| `lockbox-core` | lib | Reference implementation of the protocol server |
| `lockbox-proto` | lib | Technical specification of the protocol |
| `lockbox-store` | lib | Persistent store for local and federated data |
| `lockboxd` | bin | Persistent UNIX socket daemon |
| `lockbox-web` | bin | Standard web client |

## Run

```sh
cargo run -p lockboxd         # starts the UNIX socket
cargo run -p lockbox-web      # starts a web server on http://127.0.0.1:8080/
```

## Current API

> [!note]
> This will be moved to another page at some point when it gets too large.

| Route | Reply |
| --- | --- |
| `GET /api/health` | `{"ok":true}` |
| `GET /api/status` | returns general information about the node |
