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

### Node metadata

| Route | Reply |
| --- | --- |
| `GET /api/node/health` | `{"ok":true}` |
| `GET /api/node/status` | returns general information about the node |
| `GET /api/node/settings` | every setting the user can change |
| `PATCH /api/node/settings` | modify the current node settings |

### RSS Reader

| Route | Reply |
| -- | -- |
| `GET /api/reader/feeds` | the feeds the reader follows |
| `POST /api/reader/feeds` | add a feed by its address, or by a page which advertises one |
| `GET /api/reader/feeds/{id}` | one feed with the state of its last poll |
| `PATCH /api/reader/feeds/{id}` | change the settings of one feed |
| `DELETE /api/reader/feeds/{id}` | unsubscribe, dropping the entries it brought |
| `POST /api/reader/feeds/{id}/poll` | fetch the feed now |
| `GET /api/reader/entries` | the reading list, by feed, read state, star and text |
| `GET /api/reader/entries/{id}` | one entry with its body |
| `PATCH /api/reader/entries/{id}` | mark the entry read, starred or archived |
| `DELETE /api/reader/entries/{id}` | delete one entry |
