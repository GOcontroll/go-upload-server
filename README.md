# go-upload-server

The server on a Moduline Linux controller (L4, M1, HMI1) that takes a new application: port **8001**.
Disable it in a production environment: it has no login and gives write access to the controller.

## What it answers

| Request | What it does |
|---|---|
| `POST /upload`, multipart field `elfFile` (a `.elf`) | Stops `go-simulink` when it runs, writes `/usr/simulink/GOcontroll_Linux.elf`, starts the service again when it ran. |
| `POST /upload`, field `a2lFile` (a `.a2l`) | Writes `/usr/simulink/GOcontroll_Linux.a2l`, runs `go-parse-a2l`, restarts `nodered`. |
| `POST /upload`, field `ovpnFile` (a `.ovpn`) | Writes `/etc/openvpn/moduline.conf`, restarts `openvpn` when it runs. |
| `GET /info` | JSON: this server (`name`, `version`, `api`), the controller (`platform`, `hardware`, `hostname`, `serial`), the application (`running`, the `elf` and the `a2l` with `size` and `modified` in seconds since 1970, and of the A2L the `epk` and the `xcp_port` it names). What is not there is `null`. |
| `POST /application/start`, `POST /application/stop` | Starts or stops `go-simulink`; answers as `GET /info`. |
| `GET /xcp` (WebSocket) | XCP to the running application: a binary message is one XCP packet, in both directions. The server puts the header of XCP on TCP (length and counter) around it and connects to the port the A2L on the controller names (50002 without one), on this controller only. Closes with 4000 when no application listens, with 4001 when the application goes. When the page goes, the application is told to stop measuring and to disconnect. |

One field per `POST /upload`, as it always was. The upload itself is unchanged since 1.1.5.

## The GOcontroll Suite

`/info`, `/application/*`, `/xcp` and the CORS headers (1.2.0) are there for the GOcontroll Suite, a web page:
its stations Flash and Monitor reach a Linux controller at its IP address through this server. A page of
`https://suite.gocontroll.tools`, `https://suite.gocontroll.com` or a developer's own `http://localhost:<port>`
gets the CORS headers it needs to read an answer, and the answer to its preflight. A page of another site gets
no CORS headers, and 403 on `/info`, `/application/*` and `/xcp`. A request that is not sent by a page (no
`Origin` header: curl, MATLAB) is answered as before.

## Tests

```
cargo test
```

## Building

to build run 
```
RUSTFLAGS="-Zlocation-detail=none -C target-cpu=cortex-a53" cargo +nightly zigbuild -Z build-std=std,panic_abort --target aarch64-unknown-linux-gnu.2.31 --release
```
and then
```
upx --best --lzma target/aarch64-unknown-linux-gnu/release/go-upload-server
```
Make sure it is compiled using glibc 2.31 or lower

package it as .deb:
```
cargo deb --no-build --target aarch64-unknown-linux-gnu --no-strip
```

sign the package:
```
dpkg-sig --sign builder target/aarch64-unknown-linux-gnu/debian/go-upload-server_*_arm64.deb
```