use crate::interaction::{oob_append, system_line, InteractionState};
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
        // Only the final message is user text; system_line escapes it.
        let finished: String = finished.chars().take(100).collect();
        let _ = tx.send(oob_append(&system_line(&format!("Starting countdown from {}...", seconds))));
        for i in (1..=seconds).rev() {
            sleep(Duration::from_secs(1)).await;
            let _ = tx.send(oob_append(&system_line(&format!("... {}", i))));
        }
        sleep(Duration::from_secs(1)).await;
        let _ = tx.send(oob_append(&system_line(&finished)).replace("chat-system", "chat-system chat-alert"));
    }
}
