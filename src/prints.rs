use solarized::{
    print_colored, print_fancy, clear,
    VIOLET, BLUE, CYAN, GREEN, YELLOW, ORANGE, RED, MAGENTA,
    WHITE,
    BOLD, UNDERLINED, ITALIC,
    PrintMode::NewLine,
};

pub fn help() {
    print_fancy(&[
        ("This program is designed to be a modular web service.\n", CYAN, vec![]),
        ("There is a hardcoded path which mounts static/home.html to /\n", CYAN, vec![]),
        ("All other paths are read from config.toml\n", CYAN, vec![]),
        ("If config.toml does not exist, an example project structure can be created.\n", CYAN, vec![]),
        ("The config.toml file should contain something similar to the following.\n", CYAN, vec![]),

        ("\nip", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("\"0.0.0.0\"\n", CYAN, vec![]),

        ("port", BLUE, vec![]),
        (" = ", WHITE, vec![]),
        ("12345\n\n", CYAN, vec![]),

        ("[routes]\n", ORANGE, vec![]),

        ("\"/something\"", BLUE, vec![]),
        (" = [", WHITE, vec![]),
        ("\"static/home.html\"", CYAN, vec![]),
        ("]\n", WHITE, vec![]),

        ("\"/stuff\"", BLUE, vec![]),
        (" = [", WHITE, vec![]),
        ("\"static/stuff.html\"", CYAN, vec![]),
        (", ", WHITE, vec![]),
        ("\"static/media\"", CYAN, vec![]),
        ("]", WHITE, vec![]),
    ], NewLine);
    return;
}
