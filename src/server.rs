use std::path::PathBuf;
use simple_server::{Request, ResponseBuilder, ResponseResult};

struct Server {
    dir: PathBuf
}

impl Server {
    fn error_message(title: &str, detail: &str) -> Vec<u8> {
        format!(r#"
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
        "#, title, detail).into_bytes()
    }

    fn handle_request(&self, request: Request<Vec<u8>>, mut response: ResponseBuilder) -> ResponseResult {
        if request.method().as_str() != "GET" && request.method().as_str() != "HEAD" {
            println!("info: server: {} {} => 405 method not allowed", request.method(), request.uri().path());
            return Ok(response.status(405)
                .header("Allow", "GET, HEAD")
                .body(Self::error_message("405 Method Not Allowed", &format!(
                    "The {} method is not supported", request.method()
                )))?
            )
        }

        let Ok(path) = urlencoding::decode(request.uri().path())
            else { 
                println!("info: server: {} {} => 400 bad request: could not decode path", request.method(), request.uri().path());
                return Ok(response.status(400)
                    .body(Self::error_message("400 Bad Request", &format!(
                        "The path could not be decoded: {:?}", request.uri().path()
                    )))?
                )
            };
        let path = if path == "/" { "/index.html" } else { &path };
        let path = path.trim_start_matches("/");
        let path = self.dir.join(path);

        if !path.is_file() {
            println!("info: server: {} {} => 404 not found", request.method(), request.uri().path());
            return Ok(response.status(404)
                .body(Self::error_message("404 Not Found", &format!(
                    "Requested: {:?}", request.uri().path()
                )))?
            )
        }

        match std::fs::read(&path) {
            Err(e) => {
                println!("info: server: {} {} => 500 internal server error: {}", request.method(), request.uri().path(), e);
                Ok(response.status(500)
                    .body(Self::error_message("500 Internal Server Error", &format!("{}", e)))?
                )
            }
            Ok(contents) => {
                let etag = format!("\"{:016x}\"", {
                    use std::hash::Hasher;
                    let mut hasher = std::hash::DefaultHasher::new();
                    hasher.write(&contents);
                    hasher.finish()
                });

                response
                    .header("Cache-Control", "public, must-revalidate")
                    .header("ETag", &etag)
                    .header("Vary", "Accept-Encoding");

                if let Some(mtag) = request.headers().get("if-none-match") && etag.as_bytes() == mtag.as_bytes() {
                    println!("info: server: {} {} => 304 not modified, etag {}", request.method(), request.uri().path(), etag);
                    response.status(304);
                    return Ok(response.body(Vec::new())?)
                }

                let content_type = mime_guess::from_path(&path).first();
                let should_compress = if let Some(mime) = &content_type {
                    response.header("Content-Type", mime.as_ref());
                    mime.type_() == "text" || [
                        "application/json", "application/javascript", "application/xml", "image/svg+xml"
                    ].contains(&mime.essence_str())
                } else { false };

                if request.method().as_str() == "HEAD" {
                    println!("info: server: {} {} => 200 okay", request.method(), request.uri().path());
                    return Ok(response.body(Vec::new())?);
                }   

                if should_compress && let Some(enc) = request.headers().get("accept-encoding") && enc.to_str().map(|s| s.contains("gzip")).unwrap_or(false) {
                    use std::io::Write;
                    let mut buffer = Vec::new();
                    {
                        let mut encoder = flate2::write::GzEncoder::new(&mut buffer, flate2::Compression::fast());
                        if let Err(e) = encoder.write_all(&contents) {
                            println!("info: server: {} {} => 500 internal server error: {}", request.method(), request.uri().path(), e);
                            return Ok(response.status(500)
                                .body(Self::error_message("500 Internal Server Error", &format!("{}", e)))?
                            )
                        }
                    }
                    println!("info: server: {} {} => 200 okay, gzipped, {} bytes, content-type: {:?}", request.method(), request.uri().path(), buffer.len(), content_type);
                    response.header("Content-Encoding", "gzip");
                    response.status(200);
                    Ok(response.body(buffer)?)
                } else {
                    println!("info: server: {} {} => 200 okay, {} bytes, content-type: {:?}", request.method(), request.uri().path(), contents.len(), content_type);
                    response.status(200);
                    Ok(response.body(contents)?)
                }
            }
        }

    }
}

pub fn start_server(dir: PathBuf, port: u16) {
    let server = Server { dir };
    std::thread::spawn(move || {
        let server = simple_server::Server::new(move |req, resp| server.handle_request(req, resp));
        println!("info: server: listening on localhost:{port}");
        server.listen("localhost", &format!("{}", port))
    });
}