use std::{path::PathBuf, pin::Pin, sync::Arc};
use hyper::{Request, Response, body::{Bytes, Incoming}, server::conn::http1, service::Service};
use http_body_util::Full;
use tokio::sync::watch;

type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone)]
struct Server {
    dir: Arc<PathBuf>,
    rx: watch::Receiver<std::time::Instant>
}

impl Server {
    fn error_message(title: &str, detail: &str) -> Full<Bytes> {
        Full::new(Bytes::from(format!(r#"
            <!DOCTYPE html>
            <html>
                <head>
                    <meta charset="UTF-8">
                    <style>body {{ font-family: sans-serif; }} main {{ margin: auto; padding: 20px; width: fit-content; }}</style>
                </head>
                <body>
                    <main>
                        <h1>{}</h1>
                        {}
                    </main>
                </body>
            </html>
        "#, title, detail).into_bytes()))
    }

    async fn handle_hotreload(&mut self, request: Request<Incoming>) -> Result<Response<Full<Bytes>>, Error> {
        let response = Response::builder()
            .status(200)
            .header("Cache-Control", "no-cache")
            .header("Content-Type", "text/plain");
        let before = std::time::Instant::now();
            
        if request.method().as_str() == "GET" {
            println!("info: server: GET /hotreload => long-poll started");
            let time = self.rx.borrow_and_update().clone();
            if time < before {
                if let Err(e) = self.rx.changed().await {
                    println!("error: server: hot reload error: {:?}", e);
                }
            }
        }

        println!("info: server: {} /hotreload => 200 okay, long-poll waited {:?}", request.method(), before.elapsed());

        return Ok(response.body(Full::new(Bytes::new()))?)
    }

    async fn handle_request(&mut self, request: Request<Incoming>) -> Result<Response<Full<Bytes>>, Error> {
         if request.method().as_str() != "GET" && request.method().as_str() != "HEAD" {
            println!("info: server: {} {} => 405 method not allowed", request.method(), request.uri().path());
            return Ok(Response::builder().status(405)
                .header("Allow", "GET, HEAD")
                .body(Self::error_message("405 Method Not Allowed", &format!(
                    "The {} method is not supported", request.method()
                )))?);
        }

        if request.uri().path() == "/hotreload" {
            return self.handle_hotreload(request).await
        }

        let Ok(path) = urlencoding::decode(request.uri().path())
            else { 
                println!("info: server: {} {} => 400 bad request: could not decode path", request.method(), request.uri().path());
                return Ok(Response::builder().status(400)
                    .body(Self::error_message("400 Bad Request", &format!(
                        "The path could not be decoded: {:?}", request.uri().path()
                    )))?)
            };
        let path = if path == "/" { "/index.html" } else { &path };
        let path = path.trim_start_matches("/");
        let path = self.dir.join(path);

        if !path.is_file() {
            println!("info: server: {} {} => 404 not found", request.method(), request.uri().path());
            return Ok(Response::builder().status(404)
                .body(Self::error_message("404 Not Found", &format!(
                    "Requested: {:?}", request.uri().path()
                )))?)
        }

        match tokio::fs::read(&path).await {
            Err(e) => {
                println!("info: server: {} {} => 500 internal server error: {}", request.method(), request.uri().path(), e);
                Ok(Response::builder().status(500)
                    .body(Self::error_message("500 Internal Server Error", &format!("{}", e)))?)
            }
            Ok(contents) => {
                let etag = format!("\"{:016x}\"", {
                    use std::hash::Hasher;
                    let mut hasher = std::hash::DefaultHasher::new();
                    hasher.write(&contents);
                    hasher.finish()
                });

                let mut response = Response::builder()
                    .header("Cache-Control", "public, must-revalidate")
                    .header("ETag", &etag)
                    .header("Vary", "Accept-Encoding");

                if let Some(mtag) = request.headers().get("if-none-match") && etag.as_bytes() == mtag.as_bytes() {
                    println!("info: server: {} {} => 304 not modified, etag {}", request.method(), request.uri().path(), etag);
                    return Ok(response
                        .status(304)
                        .body(Full::new(Bytes::new()))?);
                }

                let content_type = mime_guess::from_path(&path).first();
                let should_compress = if let Some(mime) = &content_type {
                    response = response.header("Content-Type", mime.as_ref());
                    mime.type_() == "text" || [
                        "application/json", "application/javascript", "application/xml", "image/svg+xml"
                    ].contains(&mime.essence_str())
                } else { false };

                if request.method().as_str() == "HEAD" {
                    println!("info: server: {} {} => 200 okay", request.method(), request.uri().path());
                    return Ok(response.status(200).body(Full::new(Bytes::new()))?);
                }   

                if should_compress && let Some(enc) = request.headers().get("accept-encoding") && enc.to_str().map(|s| s.contains("gzip")).unwrap_or(false) {
                    let buffer = tokio::task::spawn_blocking(move || {
                        use std::io::Write;
                        let mut buffer = Vec::new();
                        {
                            let mut encoder = flate2::write::GzEncoder::new(&mut buffer, flate2::Compression::fast());
                            encoder.write_all(&contents)?;
                        }
                        Ok::<_, std::io::Error>(buffer)
                    }).await;

                    let buffer = match buffer {
                        Err(e) => {
                            println!("info: server: {} {} => 500 internal server error: {}", request.method(), request.uri().path(), e);
                            return Ok(response.status(500)
                                .body(Self::error_message("500 Internal Server Error", &format!("{}", e)))?
                            )
                        },
                        Ok(Err(e)) => {
                            println!("info: server: {} {} => 500 internal server error: {}", request.method(), request.uri().path(), e);
                            return Ok(response.status(500)
                                .body(Self::error_message("500 Internal Server Error", &format!("{}", e)))?
                            )
                        },
                        Ok(Ok(buffer)) => buffer
                    };

                    println!("info: server: {} {} => 200 okay, gzipped, {} bytes, content-type: {:?}", request.method(), request.uri().path(), buffer.len(), content_type);
                    Ok(response.status(200)
                        .header("Content-Encoding", "gzip")
                        .body(Full::new(Bytes::from(buffer)))?)
                } else {
                    println!("info: server: {} {} => 200 okay, {} bytes, content-type: {:?}", request.method(), request.uri().path(), contents.len(), content_type);
                    Ok(response.status(200)
                        .body(Full::new(Bytes::from(contents)))?)
                }
            }
        }
    }
}

impl Service<Request<Incoming>> for Server {
    type Response = Response<Full<Bytes>>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, request: Request<Incoming>) -> Self::Future {
        let mut server = self.clone();
        Box::pin(async move { server.handle_request(request).await })
    }
}

async fn server_main(dir: PathBuf, port: u16, rx: watch::Receiver<std::time::Instant>) -> Result<(), Error> {
    let server = Server { dir: Arc::new(dir), rx };
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("info: server: listening on 127.0.0.1:{port}");
    loop {
        let (stream, _) = listener.accept().await?;
        let io = hyper_util::rt::TokioIo::new(stream);
        let srv = server.clone();
        tokio::task::spawn(async move {
            if let Err(e) = http1::Builder::new().serve_connection(io, srv).await {
                println!("error: server: could not serve connection: {}", e);
            }
        });
    }
}

pub fn start_server(dir: PathBuf, port: u16) -> watch::Sender<std::time::Instant> {
    let (tx, rx) = watch::channel(std::time::Instant::now());
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Runtime::new()
            .inspect_err(|e| println!("error: server: could not start runtime: {}", e)) 
            else { return };
        rt.block_on(async { 
            if let Err(e) = rt.spawn(server_main(dir, port, rx)).await {
                println!("error: server: died with: {}", e);
            } 
        });
    });
    tx
}