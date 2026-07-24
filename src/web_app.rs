use std::borrow::Cow;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::Command;

use anyhow::{Context, Result};

const INDEX_HTML: &[u8] = include_bytes!("../web/index.html");
const TRAP_INFO_EXPORT_JS: &[u8] = include_bytes!("../web/trap_info_export.js");
const PKG_WASM: &[u8] = include_bytes!("../web/pkg/serval_charton_web_bg.wasm");
const PKG_JS: &[u8] = include_bytes!("../web/pkg/serval_charton_web.js");

pub fn serve(bind: SocketAddr, open_browser: bool) -> Result<()> {
    let listener = TcpListener::bind(bind)
        .with_context(|| format!("failed to bind local web server on {bind}"))?;
    let local_addr = listener
        .local_addr()
        .context("failed to determine local web server address")?;
    let url = format!("http://{local_addr}/");

    println!("WASM explorer available at {url}");
    println!("Open this URL in your browser, or Ctrl-click the link to open it quickly.");
    println!("请在浏览器中打开这个地址，或按住 Ctrl 点击链接快速打开。");
    println!("Press Ctrl-C to stop the local server.");

    if open_browser {
        if let Err(error) = try_open_browser(&url) {
            eprintln!("warning: failed to open browser automatically: {error}");
        }
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle_connection(stream) {
                    // Browsers open several parallel/speculative connections and
                    // close the ones they don't use; writing to those fails with
                    // a broken pipe / reset. That is expected, not an error.
                    if !is_benign_disconnect(&error) {
                        eprintln!("request handling error: {error:#}");
                    }
                }
            }
            Err(error) => {
                eprintln!("incoming connection error: {error}");
            }
        }
    }

    Ok(())
}

/// A client closing its connection before we finish writing is normal browser
/// behaviour (parallel/speculative sockets), not a server error worth logging.
fn is_benign_disconnect(error: &anyhow::Error) -> bool {
    use std::io::ErrorKind;
    error.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
            )
        })
    })
}

fn handle_connection(mut stream: TcpStream) -> Result<()> {
    let mut request_line = String::new();
    {
        let mut reader = BufReader::new(&mut stream);
        reader
            .read_line(&mut request_line)
            .context("failed to read request line")?;

        // Drain the remaining request headers before responding. If we close
        // the socket while the client is still sending its headers, the OS can
        // reset the connection and truncate our response body (notably the
        // multi-megabyte WASM bundle).
        let mut header_line = String::new();
        loop {
            header_line.clear();
            let read = reader
                .read_line(&mut header_line)
                .context("failed to read request headers")?;
            if read == 0 || header_line == "\r\n" || header_line == "\n" {
                break;
            }
        }
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let raw_path = parts.next().unwrap_or("/");
    let path = raw_path.split('?').next().unwrap_or("/");

    let response = match method {
        "GET" | "HEAD" => route(path),
        _ => Response::method_not_allowed(),
    };

    write_response(&mut stream, method == "HEAD", response)
}

fn route(path: &str) -> Response<'_> {
    match path {
        "/" | "/index.html" => Response::ok("text/html; charset=utf-8", INDEX_HTML),
        "/trap_info_export.js" => {
            Response::ok("text/javascript; charset=utf-8", TRAP_INFO_EXPORT_JS)
        }
        "/pkg/serval_charton_web_bg.wasm" => Response::ok("application/wasm", PKG_WASM),
        "/pkg/serval_charton_web.js" => Response::ok("text/javascript; charset=utf-8", PKG_JS),
        _ => Response::not_found(),
    }
}

fn write_response(stream: &mut TcpStream, head_only: bool, response: Response<'_>) -> Result<()> {
    let status_text = match response.status_code {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "OK",
    };

    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        response.status_code,
        status_text,
        response.content_type,
        response.body.len()
    )
    .context("failed to write response headers")?;

    if !head_only {
        stream
            .write_all(&response.body)
            .context("failed to write response body")?;
    }

    stream.flush().context("failed to flush response")
}

fn try_open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .status()
            .context("failed to spawn macOS browser opener")?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()
            .context("failed to spawn Windows browser opener")?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(url)
            .status()
            .context("failed to spawn Linux browser opener")?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Ok(())
}

struct Response<'a> {
    status_code: u16,
    content_type: &'a str,
    body: Cow<'a, [u8]>,
}

impl<'a> Response<'a> {
    fn ok(content_type: &'a str, body: &'a [u8]) -> Self {
        Self {
            status_code: 200,
            content_type,
            body: Cow::Borrowed(body),
        }
    }

    fn not_found() -> Self {
        Self {
            status_code: 404,
            content_type: "text/plain; charset=utf-8",
            body: Cow::Borrowed(b"404 not found"),
        }
    }

    fn method_not_allowed() -> Self {
        Self {
            status_code: 405,
            content_type: "text/plain; charset=utf-8",
            body: Cow::Borrowed(b"405 method not allowed"),
        }
    }
}
