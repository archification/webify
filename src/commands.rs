use crate::interaction::InteractionState;
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};
use std::future::Future;

pub fn register_all(state: &mut InteractionState) {
    state.register_command("countdown", countdown);
}

/// Usage: /countdown [seconds]
fn countdown(args: Vec<String>, tx: broadcast::Sender<String>) -> impl Future<Output = ()> + Send {
    async move {
        let seconds = args.get(0)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(5);
        let finished = if args.len() > 1 {
            args[1..].join(" ")
        } else {
                "guacamole".to_string()
        };
        let seconds = seconds.clamp(1, 60);
        let start_msg = format!(
            r#"<div hx-swap-oob="beforeend:#chat-container"><div class="system-msg">Starting countdown from {}...</div></div>"#,
            seconds
        );
        let _ = tx.send(start_msg);
        for i in (1..=seconds).rev() {
            sleep(Duration::from_secs(1)).await;
            let tick_msg = format!(
                r#"<div hx-swap-oob="beforeend:#chat-container"><div class="system-msg">... {}</div></div>"#,
                i
            );
            let _ = tx.send(tick_msg);
        }
        sleep(Duration::from_secs(1)).await;
        let go_msg = format!(
            r#"<div hx-swap-oob="beforeend:#chat-container"><div class="system-msg" style="color: #dc322f; font-weight: bold;">{}</div></div>"#,
            finished
        );
        let _ = tx.send(go_msg);
    }
}
