use solarized::{
    print_fancy,
    VIOLET, BLUE, CYAN, ORANGE,
    WHITE, GREY,
    PrintMode::NewLine,
};

pub fn print_help(binaryname: String) {
    print_fancy(&[
        ("This program is designed to be a modular web service.\n", CYAN, vec![]),
        ("All paths and routes are configured in config.toml\n", CYAN, vec![]),
        ("If config.toml does not exist, an example project structure can be created.\n\n", CYAN, vec![]),
        ("There is builtin archive support. Example:\n", CYAN, vec![]),
        (&format!("{} -b ", binaryname), VIOLET, vec![]),
        ("<", CYAN, vec![]),
        ("path/to/source/directory", VIOLET, vec![]),
        ("> <", CYAN, vec![]),
        ("path/to/destination.zip", VIOLET, vec![]),
        (">\n\n", CYAN, vec![]),
        ("The ", CYAN, vec![]),
        ("config.toml", VIOLET, vec![]),
        (" file will contain something similar to the following.\n\n", CYAN, vec![]),
//base web config
        ("scope", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("\"localhost\"\n", CYAN, vec![]),
        ("ip", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("\"192.168.1.142\"\n", CYAN, vec![]),
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
//upload limit
        ("upload_size_limit", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("2000000000", CYAN, vec![]),
        (" # 2 GB (2 * 1000 * 1000 * 1000)\n", GREY, vec![]),
        ("upload_storage_limit", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("2000000000\n", CYAN, vec![]),
//browser
        ("browser", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("false\n\n", CYAN, vec![]),
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
//upload route
        ("\"/uplaod\"", BLUE, vec![]),
        (" = [", WHITE, vec![]),
        ("\"static/upload.html\"", CYAN, vec![]),
        ("]\n", WHITE, vec![]),
    ], NewLine);
    std::process::exit(0);
}
