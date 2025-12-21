pub static IMAGE_DATA: &[u8] = include_bytes!("thing.png");
pub static PDF_DATA: &[u8] = include_bytes!("asdf.pdf");
pub static EXAMPLE_CONFIG: &str = r#"scope = "localhost"
ip = "192.168.1.142"
port = 12345
ssl_enabled = false
ssl_port = 12345
ssl_cert_path = "pems/cert.pem"
ssl_key_path = "pems/key.pem"
upload_size_limit = 2147483648 # 2 GB (2 * 1024 * 1024 * 1024)
#upload_size_limit = "disabled" # allows any size
upload_storage_limit = 2147483648
browser = false
slideshow_autoplay = false
slideshow_timer = 20

[whitelist]
"default" = ["127.0.0.1", "5.6.7.8"]

[whitelist."example.local"]
"allowed_ips" = ["192.168.1.50", "192.168.1.51"]

[routes]
"/" = ["static/home.html"]
"/stuff" = ["static/stuff.html", "static/media", "random"]
"/thumb" = ["static/thumb.html", "static/media/"]
"/pdf" = ["static/pdf.html", "static/documents"]
"/downloads" = ["static/downloads.html", "static/files"]
"/playlists" = ["static/playlists.html", "static/audio/"]
"/upload" = ["static/upload.html"]
"/files" = ["static/uploads.html", "uploads"]
"/slideshow" = ["slideshow", "static/slides"]
"/blog" = ["static/blog.html", "static/posts"]

[routes."example.com"]
"/" = ["static/examplesite/guacamole.html"]
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

    <h1>Thumbnails</h1>
    <a href="/thumb">thumb</a>

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

    <h1>Welcome to the stuff page.</h1>
    <p>This page shows media files.</p>

    <div class="container">
        <!-- MEDIA_INSERTION_POINT -->
    </div>
</body>
</html>
"#;
pub static EXAMPLE_THUMB: &str = r#"<!doctype html>
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
    .container {
      display: flex;
      flex-wrap: wrap;
      gap: 1rem;
      justify-content:center;
    }
    </style>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>PDF Document</h1>
    <a href="/pdf">documents</a>

    <h1>Home Page</h1>
    <a href="/">home</a>

    <h1>Welcome to the stuff page.</h1>
    <p>This page shows media files as thumbnails.</p>

    <div class="container">
        <!-- THUMBNAIL_INSERTION_POINT -->
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
    </body>
</html>
"#;
pub static EXAMPLE_BLOG: &str = r#"<!doctype html>
<html lang="en-US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>My Blog</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <style>
    .blog-list-container {
        max-width: 800px;
        margin: 0 auto;
        padding: 20px;
    }
    .blog-list-container h1, .blog-list-container h2 {
        text-align: center;
    }
    .post-link {
        display: block;
        padding: 15px;
        margin-bottom: 10px;
        background-color: #073642;
        border-radius: 5px;
        text-decoration: none;
        color: #93a1a1;
        transition: background-color 0.3s;
    }
    .post-link:hover {
        background-color: #586e75;
        color: #fdf6e3;
    }
    </style>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <div class="blog-list-container">
        <h1>Welcome to My Blog</h1>
        <p style="text-align: center;">Here are some things I've written about.</p>
        <hr>
            <!-- MEDIA_INSERTION_POINT -->
        </div>
    <div style="text-align: center; margin-top: 40px;">
        <a href="/">Back to Home</a>
    </div>
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
    <h1>Home Page</h1>
    <a href="/">home</a>
    <div class="container">
        <h1>Welcome to the downloads page.</h1>
        <p>This page hosts files for download.</p>
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
    <h1>Home Page</h1>
    <a href="/">home</a>
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
    <h1>Home Page</h1>
    <a href="/">home</a>
    <h1>Audio Player</h1>
    <audio id="audioPlayer" controls autoplay></audio>
    <script>
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
    <h1>Home Page</h1>
    <a href="/">home</a>
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
    <h1>Home Page</h1>
    <a href="/">home</a>
    <div class="container">
        <h1>Welcome to the uploads page</h1>
        <p>This page shows the uploaded files.</p>
        </div>
    <h1>Upload More Files</h1>
    <a href="/upload">Upload File</a>
</body>
</html>
"#;
pub static EXAMPLE_FIRST_SLIDE: &str = r#"# Welcome to the Slideshow

## Created with reveal.js

- Supports **markdown** formatting
- Code highlighting
- Keyboard navigation
- Responsive design

```rust
fn main() {
    println!("Hello there.");
}
```
"Simple is better than complex"
"#;
pub static EXAMPLE_SECOND_SLIDE: &str = r#"## Solarized Theme

* **Dark background** for comfortable viewing
* Complementary accent colors
* Clean typography

```python
def fibonacci(n):
    a, b = 0, 1
    for _ in range(n):
        yield a
        a, b = b, a + b
```
[some other slide](slideshow?current=0)

![image](static/images/localImage.jpg)
"#;
pub static EXAMPLE_FIRST_POST: &str = r#"## Welcome to the post

* Supports **markdown** formatting
* Code highlighting
* Keyboard navigation
* Responsive design

```rust
fn main() {
    println!("Hello there.");
}
```
"Simple is better than complex"
"#;
pub static EXAMPLE_SECOND_POST: &str = r#"## Solarized Theme

* **Dark background** for comfortable viewing
* Complementary accent colors
* Clean typography

```python
def fibonacci(n):
    a, b = 0, 1
    for _ in range(n):
        yield a
        a, b = b, a + b
```
[some other slide](slideshow?current=0)

![image](static/images/localImage.jpg)
"#;
