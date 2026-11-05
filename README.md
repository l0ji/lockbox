# lockbox

the pedant's stumbleupon (it's a link aggregator).

## Crates

| Crate | Kind | Role |
| --- | --- | --- |
| `lockbox-core` | lib | Reference implementation of the protocol server |
| `lockbox-proto` | lib | API specification of the protocol |
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
