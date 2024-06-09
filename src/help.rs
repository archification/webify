use solarized::{
    print_fancy,
    VIOLET, BLUE, CYAN, ORANGE,
    WHITE, GREY,
    PrintMode::NewLine,
};

pub fn print_help() {
    print_fancy(&[
        ("This program is designed to be a modular web service.\n", CYAN, vec![]),
        ("All paths and routes are configured in config.toml\n", CYAN, vec![]),
        ("If config.toml does not exist, an example project structure can be created.\n\n", CYAN, vec![]),
        ("There is builtin archive support. Example:\n", CYAN, vec![]),
        ("webify -b <path/to/source/directory> <path/to/destination/zip>\n\n", VIOLET, vec![]),
        ("The config.toml file will contain something similar to the following.\n", CYAN, vec![]),
//base web config
        ("\nip", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("\"127.0.0.1\"\n", CYAN, vec![]),
        ("port", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("12345\n", CYAN, vec![]),
//ssl config
        ("ssl_enabled", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("false\n", CYAN, vec![]),
        ("ssl_port", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("44444\n", CYAN, vec![]),
        ("ssl_cert_path", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("\"pems/cert.pem\"\n", CYAN, vec![]),
        ("ssl_key_path", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("\"pems/key.pem\"\n", CYAN, vec![]),
//todo config
        ("todo_enabled", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("true\n", CYAN, vec![]),
        ("todo_ip", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("\"127.0.0.1\"\n", CYAN, vec![]),
        ("todo_port", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("11111\n", CYAN, vec![]),
//upload limit
        ("upload_size_limit", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("2000000000", CYAN, vec![]),
        (" # 2 GB (2 * 1000 * 1000 * 1000)\n\n", GREY, vec![]),
//default routes
        ("[routes]\n", ORANGE, vec![]),
//home route
        ("\"/\"", BLUE, vec![]),
        (" = [", WHITE, vec![]),
        ("\"static/home.html\"", CYAN, vec![]),
        ("]\n", WHITE, vec![]),
//stuff route
        ("\"/stuff\"", BLUE, vec![]),
        (" = [", WHITE, vec![]),
        ("\"static/stuff.html\"", CYAN, vec![]),
        (", ", WHITE, vec![]),
        ("\"static/media\"", CYAN, vec![]),
        ("]\n", WHITE, vec![]),
//pdf route
        ("\"/pdf\"", BLUE, vec![]),
        (" = [", WHITE, vec![]),
        ("\"static/pdf.html\"", CYAN, vec![]),
        (", ", WHITE, vec![]),
        ("\"static/documents\"", CYAN, vec![]),
        ("]\n", WHITE, vec![]),
//downloads route
        ("\"/downloads\"", BLUE, vec![]),
        (" = [", WHITE, vec![]),
        ("\"static/downloads.html\"", CYAN, vec![]),
        (", ", WHITE, vec![]),
        ("\"static/files\"", CYAN, vec![]),
        ("]\n", WHITE, vec![]),
    ], NewLine);
}
