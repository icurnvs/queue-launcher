use serde::Deserialize;
use std::{io::Read, sync::mpsc, thread};
use tiny_http::{Header, Response, Server};
use uuid::Uuid;

pub const PORT: u16 = 43192;
const PAGE: &str = include_str!("waiting-room.html");

#[derive(Debug, Deserialize)]
pub struct Report {
    pub session: Uuid,
    pub storage: Uuid,
    pub cookie: Uuid,
    pub had_storage: bool,
    pub had_cookie: bool,
    pub page: Uuid,
    pub counter: usize,
    pub bridge_denied: bool,
}

pub fn start(verify: bool) -> super::Result<(tauri::Url, mpsc::Receiver<Report>)> {
    let server = Server::http(("127.0.0.1", PORT)).map_err(|error| error.to_string())?;
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        for mut request in server.incoming_requests() {
            if request.method() == &tiny_http::Method::Get && request.url() == "/fixture" {
                let _ = request.respond(
                    Response::from_string(PAGE)
                        .with_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap())
                        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                        .with_header(Header::from_bytes("Content-Security-Policy", "default-src 'none'; script-src 'unsafe-inline' ipc: http://ipc.localhost; style-src 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost").unwrap()),
                );
            } else if verify
                && request.method() == &tiny_http::Method::Post
                && request.url() == "/report"
            {
                if let Ok(report) = serde_json::from_reader(request.as_reader().take(2048)) {
                    let _ = send.send(report);
                }
                let _ = request.respond(Response::empty(204));
            } else {
                let _ = request.respond(Response::empty(404));
            }
        }
    });
    Ok((format!("http://127.0.0.1:{PORT}/fixture").parse()?, receive))
}
