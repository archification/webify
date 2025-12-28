use axum::{
    body::Body,
    extract::{Request, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use fastcgi_client::{Client, Params, Request as FcgiRequest};
use tokio::net::TcpStream;

pub async fn handle_php(Path(script_name): Path<String>, req: Request) -> impl IntoResponse {
    let (parts, _body) = req.into_parts();
    let full_script_path = format!("/var/www/phpbb/{}", script_name);
    let mut params = Params::default();
    params.insert("SCRIPT_FILENAME".into(), full_script_path.into());
    params.insert("REQUEST_METHOD".into(), parts.method.to_string().into());
    params.insert("QUERY_STRING".into(), parts.uri.query().unwrap_or("").into());
    params.insert("SCRIPT_NAME".into(), format!("/forum/{}", script_name).into());
    let stream = match TcpStream::connect("127.0.0.1:9000").await {
        Ok(s) => s,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "PHP-FPM not found").into_response(),
    };
    let client = Client::new(stream);
    let mut empty_body = tokio::io::empty();
    let fcgi_req = FcgiRequest::new(params, &mut empty_body);
    match client.execute_once(fcgi_req).await {
        Ok(output) => Response::new(Body::from(output.stdout.unwrap_or_default())),
        Err(_e) => (StatusCode::INTERNAL_SERVER_ERROR, "PHP execution failed").into_response(),
    }
}
