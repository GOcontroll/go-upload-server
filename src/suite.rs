//! What the GOcontroll Suite (a web page) asks of a controller, beside the upload itself:
//!
//! - `GET /info`: which controller this is and which application it holds;
//! - `POST /application/start` and `/application/stop`: the service that runs the application;
//! - `GET /xcp`: a WebSocket to the XCP server of the running application, one XCP packet per
//!   message. The application listens for XCP on TCP on this controller only as far as a page is
//!   concerned: a browser speaks no raw TCP;
//! - the CORS headers a page needs to read an answer, also of `POST /upload`.
//!
//! `POST /upload` itself is not touched: a request that is not sent by a page of the Suite gets
//! the answer it always got.

use std::process::Command;
use std::time::UNIX_EPOCH;

use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        Request,
    },
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};

/// Raised when the shape of an answer changes in a way a page must know about.
const API: u32 = 1;
const ELF: &str = "/usr/simulink/GOcontroll_Linux.elf";
const A2L: &str = "/usr/simulink/GOcontroll_Linux.a2l";
const SERVICE: &str = "go-simulink";
/// Where an application listens for XCP when its A2L does not say (XCP_PORT_NUM of the blockset).
const XCP_PORT: u16 = 50002;
/// The start of an A2L holds what is asked of it here (MOD_PAR and IF_DATA); the rest is not read.
const A2L_HEAD: u64 = 64 * 1024;
/// An XCP packet on TCP is at most this long (XCPMAXCTOLENGTH of the blockset).
const MAX_PACKET: usize = 255;
/// Packet identifiers from here on are answers, events and service requests; below it, measurements.
const FIRST_NON_DAQ_PID: u8 = 0xfc;
/// Packets of the application that wait for the page.
const QUEUE: usize = 512;

/// The pages that may use what is here.
const ORIGINS: [&str; 2] = ["https://suite.gocontroll.tools", "https://suite.gocontroll.com"];

/// Whether a page of this origin is the Suite, or the Suite as a developer runs it on its own computer.
fn origin_allowed(origin: &str) -> bool {
    ORIGINS.contains(&origin)
        || ["http://localhost", "http://127.0.0.1"].iter().any(|local| {
            origin == *local
                || origin
                    .strip_prefix(local)
                    .and_then(|rest| rest.strip_prefix(':'))
                    .is_some_and(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
        })
}

fn origin_of(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::ORIGIN).and_then(|value| value.to_str().ok())
}

/// A page of another site asks. What is not a page (no Origin) is let through, as the upload always was.
fn foreign(headers: &HeaderMap) -> bool {
    origin_of(headers).is_some_and(|origin| !origin_allowed(origin))
}

fn text(status: StatusCode, message: &str) -> Response {
    (status, message.to_owned()).into_response()
}

fn refused() -> Response {
    text(StatusCode::FORBIDDEN, "This page may not use the upload server\n")
}

/// The headers a page of the Suite needs to read an answer, and the answer to its preflight.
/// A request of anything else passes as it is.
pub async fn cors(request: Request, next: Next) -> Response {
    let Some(origin) = origin_of(request.headers()).filter(|origin| origin_allowed(origin)).map(str::to_owned) else {
        return next.run(request).await;
    };
    let mut response = if request.method() == Method::OPTIONS {
        let mut preflight = StatusCode::NO_CONTENT.into_response();
        let headers = preflight.headers_mut();
        headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, OPTIONS"));
        headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("content-type"));
        headers.insert("access-control-allow-private-network", HeaderValue::from_static("true"));
        headers.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
        preflight
    } else {
        next.run(request).await
    };
    if let Ok(value) = HeaderValue::from_str(&origin) {
        response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
        response.headers_mut().insert(header::VARY, HeaderValue::from_static("Origin"));
    }
    response
}

// ── What the controller says about itself ───────────────────────────────────

/// A JSON string, or null.
fn json_text(value: Option<&str>) -> String {
    let Some(value) = value else { return "null".to_owned() };
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A property of the device tree: text that ends in a NUL.
fn device_tree(name: &str) -> Option<String> {
    let raw = std::fs::read(format!("/sys/firmware/devicetree/base/{name}")).ok()?;
    let value = String::from_utf8_lossy(&raw).trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_owned();
    (!value.is_empty()).then_some(value)
}

/// What a command prints when it succeeds.
fn printed(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn service_active() -> bool {
    Command::new("systemctl").args(["is-active", "--quiet", SERVICE]).status().is_ok_and(|status| status.success())
}

/// `{"size", "modified"}` of a file (modified: seconds since 1970), without the closing brace; None when it is not there.
fn file_json(path: &str) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok().and_then(|at| at.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    Some(format!("{{\"size\":{},\"modified\":{}", meta.len(), modified))
}

/// The start of the A2L on this controller.
fn a2l_head() -> Option<String> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(A2L).ok()?.take(A2L_HEAD).read_to_end(&mut head).ok()?;
    Some(String::from_utf8_lossy(&head).into_owned())
}

/// The build an A2L is of: `EPK "<name>"` in its MOD_PAR.
fn a2l_epk(a2l: &str) -> Option<String> {
    a2l.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("EPK")?;
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let rest = rest.trim().strip_prefix('"')?;
        Some(rest[..rest.find('"')?].to_owned())
    })
}

/// The TCP port an A2L names for XCP: the second number of its XCP_ON_TCP_IP.
fn a2l_xcp_port(a2l: &str) -> Option<u16> {
    let after = &a2l[a2l.find("/begin XCP_ON_TCP_IP")? + "/begin XCP_ON_TCP_IP".len()..];
    let port = after.split_whitespace().nth(1)?;
    match port.strip_prefix("0x").or_else(|| port.strip_prefix("0X")) {
        Some(hex) => u16::from_str_radix(hex, 16).ok(),
        None => port.parse().ok(),
    }
}

fn info_json() -> String {
    let a2l = a2l_head();
    let elf = file_json(ELF).map_or("null".to_owned(), |file| format!("{file}}}"));
    let a2l_file = file_json(A2L).map_or("null".to_owned(), |file| {
        format!(
            "{file},\"epk\":{},\"xcp_port\":{}}}",
            json_text(a2l.as_deref().and_then(a2l_epk).as_deref()),
            a2l.as_deref().and_then(a2l_xcp_port).map_or("null".to_owned(), |port| port.to_string())
        )
    });
    format!(
        "{{\"server\":{{\"name\":\"go-upload-server\",\"version\":{},\"api\":{API}}},\
          \"controller\":{{\"platform\":{},\"hardware\":{},\"hostname\":{},\"serial\":{}}},\
          \"application\":{{\"running\":{},\"elf\":{elf},\"a2l\":{a2l_file}}}}}\n",
        json_text(Some(crate::VERSION)),
        json_text(device_tree("platform").as_deref()),
        json_text(device_tree("hardware").as_deref()),
        json_text(printed("hostname", &[]).as_deref()),
        json_text(printed("go-sn", &["read"]).as_deref()),
        service_active(),
    )
}

fn info_response() -> Response {
    (StatusCode::OK, [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], info_json()).into_response()
}

pub async fn info(headers: HeaderMap) -> Response {
    if foreign(&headers) {
        return refused();
    }
    info_response()
}

/// Start or stop the service that runs the application, and say how the controller stands then.
fn service(headers: &HeaderMap, action: &str) -> Response {
    if foreign(headers) {
        return refused();
    }
    match Command::new("systemctl").args([action, SERVICE]).status() {
        Ok(status) if status.success() => info_response(),
        Ok(_) => text(StatusCode::INTERNAL_SERVER_ERROR, &format!("systemctl {action} {SERVICE} failed\n")),
        Err(err) => text(StatusCode::INTERNAL_SERVER_ERROR, &format!("systemctl could not be run: {err}\n")),
    }
}

pub async fn start(headers: HeaderMap) -> Response {
    service(&headers, "start")
}

pub async fn stop(headers: HeaderMap) -> Response {
    service(&headers, "stop")
}

// ── XCP: a WebSocket to the application ─────────────────────────────────────

/// An XCP packet as it goes over TCP: its length and a counter, both 16 bits little endian, then the packet.
fn frame(packet: &[u8], counter: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + packet.len());
    out.extend_from_slice(&(packet.len() as u16).to_le_bytes());
    out.extend_from_slice(&counter.to_le_bytes());
    out.extend_from_slice(packet);
    out
}

pub async fn xcp(headers: HeaderMap, socket: WebSocketUpgrade) -> Response {
    if foreign(&headers) {
        return refused();
    }
    socket.on_upgrade(bridge)
}

/// Pass XCP packets between the page and the application until one of them goes: a binary message
/// of the page is one command, a packet of the application one binary message.
async fn bridge(mut socket: WebSocket) {
    let port = a2l_head().as_deref().and_then(a2l_xcp_port).unwrap_or(XCP_PORT);
    let stream = match TcpStream::connect(("127.0.0.1", port)).await {
        Ok(stream) => stream,
        Err(_) => {
            // 4000: the application is not there to talk to.
            let reason = format!("No application listens for XCP on TCP port {port}");
            _ = socket.send(Message::Close(Some(CloseFrame { code: 4000, reason: reason.into() }))).await;
            return;
        }
    };
    _ = stream.set_nodelay(true);
    let (mut from_app, mut to_app) = stream.into_split();
    let (queue, mut packets) = mpsc::channel::<Vec<u8>>(QUEUE);
    let reader = tokio::spawn(async move {
        loop {
            let mut head = [0u8; 4];
            if from_app.read_exact(&mut head).await.is_err() {
                break;
            }
            let mut packet = vec![0u8; u16::from_le_bytes([head[0], head[1]]) as usize];
            if from_app.read_exact(&mut packet).await.is_err() {
                break;
            }
            // The application does not wait for a reader: a measurement that finds the queue full is
            // dropped, an answer waits for its place.
            let sent = if packet.first().is_some_and(|pid| *pid < FIRST_NON_DAQ_PID) {
                !matches!(queue.try_send(packet), Err(mpsc::error::TrySendError::Closed(_)))
            } else {
                queue.send(packet).await.is_ok()
            };
            if !sent {
                break;
            }
        }
    });
    let mut counter: u16 = 0;
    let mut app_gone = false;
    loop {
        tokio::select! {
            packet = packets.recv() => match packet {
                Some(packet) => {
                    if socket.send(Message::Binary(packet.into())).await.is_err() {
                        break;
                    }
                }
                None => {
                    app_gone = true;
                    break;
                }
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Binary(packet))) => {
                    if packet.is_empty() || packet.len() > MAX_PACKET {
                        break;
                    }
                    if to_app.write_all(&frame(&packet, counter)).await.is_err() {
                        app_gone = true;
                        break;
                    }
                    counter = counter.wrapping_add(1);
                }
                Some(Ok(Message::Close(_))) => {
                    // The answer to a close goes out with the next read.
                    _ = socket.recv().await;
                    break;
                }
                Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
    reader.abort();
    if app_gone {
        // 4001: the application went (stopped, or programmed again).
        _ = socket.send(Message::Close(Some(CloseFrame { code: 4001, reason: "The application closed the XCP connection".into() }))).await;
    } else {
        // The page went: the application stops measuring and is free for the next one, also when the
        // page did not say so itself.
        _ = to_app.write_all(&frame(&[0xdd, 0x00], counter)).await;
        _ = to_app.write_all(&frame(&[0xfe], counter.wrapping_add(1))).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "    /begin MOD_PAR \"x\"\n      ADDR_EPK\t\t\t0x0000\n      EPK\t\t\t\t\"GOcontroll_L4251126_121800\"\n    /end MOD_PAR\n\
        \n    /begin IF_DATA XCP\n      /begin XCP_ON_TCP_IP\n        0x100\n        50002\n        \"HOST_NAME\" Moduline L4\n      /end XCP_ON_TCP_IP\n    /end IF_DATA XCP\n";

    #[test]
    fn the_suite_and_a_developers_own_computer_are_let_in() {
        for origin in ["https://suite.gocontroll.tools", "https://suite.gocontroll.com", "http://localhost:8787", "http://127.0.0.1:8787", "http://localhost"] {
            assert!(origin_allowed(origin), "{origin}");
        }
        for origin in ["https://example.com", "http://suite.gocontroll.com", "https://suite.gocontroll.com.example.com", "http://localhost.example.com", "http://localhost:", "http://localhost:80a", "null", ""] {
            assert!(!origin_allowed(origin), "{origin}");
        }
    }

    #[test]
    fn only_a_page_of_another_site_is_foreign() {
        let mut headers = HeaderMap::new();
        assert!(!foreign(&headers)); // not a page: as the upload always was
        headers.insert(header::ORIGIN, HeaderValue::from_static("https://suite.gocontroll.com"));
        assert!(!foreign(&headers));
        headers.insert(header::ORIGIN, HeaderValue::from_static("https://example.com"));
        assert!(foreign(&headers));
    }

    #[test]
    fn an_a2l_names_its_build_and_its_xcp_port() {
        assert_eq!(a2l_epk(HEAD).as_deref(), Some("GOcontroll_L4251126_121800"));
        assert_eq!(a2l_xcp_port(HEAD), Some(50002));
        assert_eq!(a2l_xcp_port("/begin XCP_ON_TCP_IP 0x100 0xC352"), Some(50002));
        assert_eq!(a2l_epk("ADDR_EPK 0x0000\nEPKX \"no\""), None);
        assert_eq!(a2l_xcp_port("/begin XCP_ON_CAN 0x100 500"), None);
        assert_eq!(a2l_xcp_port("/begin XCP_ON_TCP_IP 0x100 port"), None);
    }

    #[test]
    fn json_text_escapes_what_json_cannot_hold() {
        assert_eq!(json_text(None), "null");
        assert_eq!(json_text(Some("Moduline L4")), "\"Moduline L4\"");
        assert_eq!(json_text(Some("a\"b\\c\n")), "\"a\\\"b\\\\c\\u000a\"");
    }

    #[test]
    fn a_packet_is_framed_with_its_length_and_a_counter() {
        assert_eq!(frame(&[0xff, 0x00], 0x0102), vec![2, 0, 2, 1, 0xff, 0x00]);
    }
}
