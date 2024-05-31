pub static IMAGE_DATA: &[u8] = include_bytes!("thing.png");
pub static PDF_DATA: &[u8] = include_bytes!("asdf.pdf");
pub static ARCHIVE_DATA: &[u8] = include_bytes!("todos.zip");
pub static EXAMPLE_CONFIG: &str = r#"ip = "127.0.0.1"
port = 12345
ssl_enabled = false
ssl_port = 12345
ssl_cert_path = "pems/cert.pem"
ssl_key_path = "pems/key.pem"
todo_enabled = true
todo_ip = "127.0.0.1"
todo_port = 11111
upload_size_limit = 2000000000 # 2 GB (2 * 1000 * 1000 * 1000)
#upload_size_limit = "disabled" # disables the limit and allows any size (not recommended)
upload_storage_limit = 2000000000

[routes]
"/" = ["static/home.html"]
"/stuff" = ["static/stuff.html", "static/media"]
"/pdf" = ["static/pdf.html", "static/documents"]
"/downloads" = ["static/downloads.html", "static/files"]
"/playlists" = ["static/playlists.html", "static/audio/"]
"/upload" = ["static/upload.html"]
"/files" = ["static/uploads.html", "uploads"]
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

    <h1>Upload</h1>
    <a href="/upload">upload</a>

    <h1>Files</h1>
    <a href="/files">Files</a>
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
    <a href="/">home</a>

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
    <a href="/">home</a>
    <!-- MEDIA_INSERTION_POINT -->
</body>
</html>
"#;
pub static EXAMPLE_DOWNLOADS: &str = r#"<!doctype html>
<html lang="en-US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>guacamole</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <div class="container">
        <h1>Welcome to the downloads page.</h1>
        <p>This page hosts files for download.</p>
        <!-- MEDIA_INSERTION_POINT -->
    </div>
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
pub static PLAYLISTS: &str = r#"<!doctype html>
<html lang="en-US">
<head>
    <meta charset="utf-8" />
    <title>Audio Player</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css">
    <style>
    img, video {
        max-width: 100%;
        height: auto;
        display: block;
        margin: 0 auto;
    }
    </style>
</head>
<body>
    <h1>Audio Player</h1>
    <audio id="audioPlayer" controls autoplay></audio>
    <script>
        <!-- JS_INSERTION_POINT -->
        let trackIndex = 0;
        const audioPlayer = document.getElementById('audioPlayer');
        function playTrack(trackIndex) {
            if (trackIndex < playlist.length) {
                audioPlayer.src = playlist[trackIndex];
                audioPlayer.play();
            } else {
                console.log('End of playlist');
            }
        }
        audioPlayer.addEventListener('ended', function() {
            trackIndex++;
            if (trackIndex < playlist.length) {
                playTrack(trackIndex);
            } else {
                trackIndex = 0;
                playTrack(trackIndex);
            }
        });
        playTrack(trackIndex);
    </script>
</body>
</html>
"#;
pub static UPLOAD: &str = r#"<!doctype html>
<html lang="en-US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>upload</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>Upload a file</h1>
    <form id="uploadForm" enctype="multipart/form-data">
        <input type="file" id="fileInput" name="file" required>
        <button type="button" onclick="uploadFile()">Upload</button>
    </form>
    <div id="progressBar" style="width: 0%; height: 20px; background-color: #4CAF50;"></div>
    <script>
        function uploadFile() {
            const formData = new FormData();
            const fileInput = document.getElementById('fileInput');
            formData.append("file", fileInput.files[0]);
            const xhr = new XMLHttpRequest();
            xhr.open("POST", "/upload", true);
            let uploadComplete = false;
            xhr.upload.onprogress = function(event) {
                if (event.lengthComputable) {
                    const percentComplete = (event.loaded / event.total) * 100;
                    document.getElementById('progressBar').style.width = percentComplete + '%';
                    if (percentComplete === 100) {
                        uploadComplete = true;
                    }
                }
            };
            xhr.onload = function() {
                if (xhr.status == 200 && uploadComplete) {
                    document.open();
                    document.write(xhr.responseText);
                    document.close();
                } else if (xhr.status != 200) {
                    alert('Error: ' + xhr.responseText);
                }
            };
            xhr.send(formData);
        }
    </script>
    <h1>Uploaded Files</h1>
    <a href="/files">View Uploaded Files</a>
</body>
</html>
"#;
pub static FILES: &str = r#"<!doctype html>
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
        <h1>Welcome to the uploads page</h1>
        <p>This page shows the uploaded files.</p>
        <!-- MEDIA_INSERTION_POINT -->
    </div>
    <h1>Upload More Files</h1>
    <a href="/upload">Upload File</a>
</body>
</html>
"#;
