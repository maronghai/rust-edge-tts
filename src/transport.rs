use crate::{Error, Result};
use base64::Engine;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{client::uri_mode, http::Request, stream::Mode},
};
pub trait Socket: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Socket for T {}
pub(crate) async fn connect(
    request: Request<()>,
    explicit: Option<&str>,
) -> Result<WebSocketStream<MaybeTlsStream<Box<dyn Socket>>>> {
    let host = request
        .uri()
        .host()
        .ok_or_else(|| Error::InvalidConfig("missing host".into()))?
        .to_owned();
    let port =
        request
            .uri()
            .port_u16()
            .unwrap_or(if matches!(uri_mode(request.uri())?, Mode::Tls) {
                443
            } else {
                80
            });
    let bypass = std::env::var("NO_PROXY")
        .or_else(|_| std::env::var("no_proxy"))
        .unwrap_or_default()
        .split(',')
        .any(|item| {
            let item = item.trim();
            item == "*"
                || (!item.is_empty()
                    && (host == item
                        || host.ends_with(&format!(".{}", item.trim_start_matches('.')))))
        });
    let env = if explicit.is_none() && !bypass {
        ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
    } else {
        None
    };
    let proxy = explicit.or(env.as_deref());
    let stream: Box<dyn Socket> = if let Some(proxy) = proxy {
        let url = url::Url::parse(proxy).map_err(|e| Error::InvalidConfig(e.to_string()))?;
        let ph = url
            .host_str()
            .ok_or_else(|| Error::InvalidConfig("missing connection host".into()))?;
        match url.scheme() {
            "socks5" | "socks5h" => {
                let addr = (ph, url.port().unwrap_or(1080));
                let stream = if url.username().is_empty() {
                    tokio_socks::tcp::Socks5Stream::connect(addr, (host.as_str(), port)).await
                } else {
                    tokio_socks::tcp::Socks5Stream::connect_with_password(
                        addr,
                        (host.as_str(), port),
                        url.username(),
                        url.password().unwrap_or(""),
                    )
                    .await
                }
                .map_err(|e| Error::Protocol(e.to_string()))?;
                Box::new(stream)
            }
            "http" => {
                let mut stream = TcpStream::connect((ph, url.port().unwrap_or(80))).await?;
                let auth = if url.username().is_empty() {
                    String::new()
                } else {
                    format!(
                        "Proxy-Authorization: Basic {}\r\n",
                        base64::engine::general_purpose::STANDARD.encode(format!(
                            "{}:{}",
                            url.username(),
                            url.password().unwrap_or("")
                        ))
                    )
                };
                stream
                    .write_all(
                        format!(
                            "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n{auth}\r\n"
                        )
                        .as_bytes(),
                    )
                    .await?;
                let mut response = Vec::new();
                while !response.ends_with(b"\r\n\r\n") {
                    let b = stream.read_u8().await?;
                    response.push(b);
                    if response.len() > 16384 {
                        return Err(Error::Protocol("CONNECT response too large".into()));
                    }
                }
                let status = String::from_utf8_lossy(&response);
                if status
                    .lines()
                    .next()
                    .and_then(|s| s.split_whitespace().nth(1))
                    != Some("200")
                {
                    return Err(Error::Protocol("CONNECT request rejected".into()));
                }
                Box::new(stream)
            }
            _ => {
                return Err(Error::InvalidConfig(
                    "supported connection schemes: http, socks5, socks5h".into(),
                ));
            }
        }
    } else {
        Box::new(TcpStream::connect((host.as_str(), port)).await?)
    };
    let (ws, _) = tokio_tungstenite::client_async_tls(request, stream).await?;
    Ok(ws)
}
