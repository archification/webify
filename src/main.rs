mod config;
mod constants;
mod generate;
mod help;
mod limits;
mod media;
mod out;
mod routes;
mod upload;
mod utils;
mod slideshow;
mod solarized_dark;

use crate::config::read_config;
use crate::generate::*;
use crate::help::print_help;
use crate::out::setup;
use crate::routes::app;
use crate::solarized_dark::SolarizedDark;

use ratatui::{
    crossterm::event::{self, Event},
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use tachyonfx::{
    fx, CenteredShrink, Duration, EffectRenderer, Interpolation, Shader
};
use axum_server::tls_rustls::RustlsConfig;
use axum_server_dual_protocol::ServerExt;
use solarized::{
    BLUE, CYAN, GREEN, MAGENTA, ORANGE, RED, VIOLET, YELLOW,
    PrintMode::NewLine,
    clear,
    print_colored,
};
use std::time::{Duration as StdDuration, Instant};
use std::{env, io};

fn format_address(scope: &str, ip: &str, port: u16) -> String {
    match scope {
        "localhost" | "local" => format!("127.0.0.1:{}", &port),
        "lan" => format!("{}:{}", &ip, &port),
        "public" | "production" | "prod" => format!("0.0.0.0:{}", &port),
        _ => format!("127.0.0.1:{}", &port),
    }
}

struct Theme;

impl Theme {
    const fn oob_color() -> Color { SolarizedDark::Base03.color() }
    fn border_style() -> Style {
        Style::default()
            .bg(SolarizedDark::Base02.color())
            .fg(SolarizedDark::Orange.color())
    }
    fn quote() -> Style {
        Style::default()
            .bg(SolarizedDark::Base02.color())
            .fg(SolarizedDark::Base0.color())
    }
}

fn render_ui(f: &mut Frame<'_>, area: Rect) {
    // Clear the background
    Block::default()
        .style(Style::default().bg(Theme::oob_color()))
        .render(f.area(), f.buffer_mut());
    // Render the main content block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border_style())
        .title(" Starting... ")
        .style(Theme::quote());
    f.render_widget(block, area);
    // Render the text inside the block using Solarized colors
    let content = Paragraph::new(
        Text::from(vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("R", Style::default().fg(SolarizedDark::Magenta.color())),
                Span::styled("a", Style::default().fg(SolarizedDark::Blue.color())),
                Span::styled("i", Style::default().fg(SolarizedDark::Cyan.color())),
                Span::styled("n", Style::default().fg(SolarizedDark::Green.color())),
                Span::styled("b", Style::default().fg(SolarizedDark::Yellow.color())),
                Span::styled("o", Style::default().fg(SolarizedDark::Orange.color())),
                Span::styled("w", Style::default().fg(SolarizedDark::Red.color())),
                Span::styled("s", Style::default().fg(SolarizedDark::Magenta.color())),
            ]).alignment(Alignment::Center),
            Line::from("").alignment(Alignment::Center),
            Line::from("Server is firing up!").alignment(Alignment::Center),
        ])
    );
    let content_area = area.inner(Margin::new(1, 1));
    f.render_widget(content, content_area);
}

/// Sets up the terminal, runs the animation, and restores the terminal.
fn run_startup_animation() -> io::Result<()> {
    // Initialize terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut last_frame = Instant::now();
    // Define the effect
    let mut effect = fx::sequence(&[
        fx::fade_from(Theme::oob_color(), Theme::oob_color(), (500, Interpolation::Linear)),
        fx::coalesce((800, Interpolation::BounceOut)),
        fx::prolong_end(Duration::from_millis(1000), fx::dissolve((500, Interpolation::Linear))),
    ]);
    while effect.running() {
        // Poll for events, exit on key press
        if event::poll(StdDuration::from_millis(16))? {
            if let Event::Key(_) = event::read()? {
                break;
            }
        }
        // Calculate elapsed time
        let now = Instant::now();
        let elapsed = now - last_frame;
        last_frame = now;
        // Draw the frame
        terminal.draw(|f| {
            let area = f.area().inner_centered(50, 7);
            render_ui(f, area);
            f.render_effect(&mut effect, area, elapsed.into());
        })?;
    }
    // Restore terminal
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

#[tokio::main]
async fn main() {
    clear();
    let args: Vec<String> = env::args().collect();
    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_help(args[0].clone());
        return;
    };
    if let Err(e) = run_startup_animation() {
        eprintln!("Failed to run startup animation: {}", e);
    }
    print_colored(
        &["R", "a", "i", "n", "b", "o", "w", "s"],
        &[VIOLET, BLUE, CYAN, GREEN, YELLOW, ORANGE, RED, MAGENTA],
        NewLine,
    );
    let config_option = read_config();
    if let Some(config) = config_option {
        setup().await;
        let app = app(&config).await;
        if config.ssl_enabled {
            let ssladdr =
                format_address(config.scope.as_str(), config.ip.as_str(), config.ssl_port);
            let ssl_config = RustlsConfig::from_pem_file(
                config
                    .ssl_cert_path
                    .clone()
                    .expect("SSL cert path is required"),
                config
                    .ssl_key_path
                    .clone()
                    .expect("SSL key path is required"),
            )
            .await
            .expect("Failed to configure SSL");
            let server =
                axum_server_dual_protocol::bind_dual_protocol(ssladdr.parse().unwrap(), ssl_config)
                    .set_upgrade(true)
                    .serve(app.clone().into_make_service());
            let server_task = tokio::spawn(async {
                server.await.unwrap();
            });
            server_task.await.unwrap();
        }
        if !config.ssl_enabled {
            let addr = format_address(config.scope.as_str(), config.ip.as_str(), config.port);
            let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
            let server = axum::serve(listener, app);
            let server_task = tokio::spawn(async {
                server.await.unwrap();
            });
            server_task.await.unwrap();
        }
    } else {
        generate_files();
    }
}
