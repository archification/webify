pub static IMAGE_DATA: &[u8] = include_bytes!("thing.png");
pub static PDF_DATA: &[u8] = include_bytes!("asdf.pdf");
pub static ARCHIVE_DATA: &[u8] = include_bytes!("todos.zip");
pub static EXAMPLE_CONFIG: &str = r#"ip = "127.0.0.1"
port = 12345
ssl_enabled = false
ssl_port = 44444
ssl_cert_path = "pems/cert.pem"
ssl_key_path = "pems/key.pem"
todo_enabled = true
todo_ip = "127.0.0.1"
todo_port = 11111

[routes]
"/" = ["static/home.html"]
"/stuff" = ["static/stuff.html", "static/media"]
"/pdf" = ["static/pdf.html", "static/documents"]
"#;
pub static EXAMPLE_HOME: &str = r#"<!doctype html>
<html lang="en-US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>guacamole</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>PDF Document</h1>
    <a href="/pdf">documents</a>

    <h1>Image and Video</h1>
    <a href="/stuff">stuff</a>
</body>
</html>
"#;
pub static EXAMPLE_STUFF: &str = r#"<!doctype html>
<html lang="en-US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>guacamole</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <style>
    img, video {
        max-width: 100%;
        height: auto;
        display: block;
        margin: 0 auto;
    }
    </style>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>PDF Document</h1>
    <a href="/pdf">documents</a>

    <h1>Home Page</h1>
    <a href="/something">home</a>

    <div class="container">
        <h1>Welcome to the stuff page.</h1>
        <p>This page shows media files.</p>
        <!-- MEDIA_INSERTION_POINT -->
    </div>
</body>
</html>
"#;
pub static EXAMPLE_PDF: &str = r#"<!doctype html>
<html lang="en-US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>guacamole</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>Image and Video</h1>
    <a href="/stuff">stuff</a>

    <h1>Home Page</h1>
    <a href="/something">home</a>
    <!-- MEDIA_INSERTION_POINT -->
</body>
</html>
"#;
pub static EXAMPLE_ERROR: &str = r#"<!doctype html>
<html lang="en-US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>guacamole</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>ERROR</h1>
    <p>This page does not exist.</p>
</body>
</html>
"#;
