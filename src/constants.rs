pub static IMAGE_DATA: &[u8] = include_bytes!("thing.png");
pub static PDF_DATA: &[u8] = include_bytes!("asdf.pdf");
pub static EXAMPLE_CONFIG: &str = r#"ip = "127.0.0.1"
port = 12345

[routes]
"/something" = ["static/home.html"]
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
    something
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
    <div class="container">
        <h1>Welcome to the stuff page.</h1>
        <p>This page shows media files.</p>
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
    <p>guacamole is good</p>
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
    This page does not exist.
</body>
</html>
"#;
