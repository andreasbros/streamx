use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tracing_subscriber::Layer;

const HISTORY_SIZE: usize = 500;
static SEQ: AtomicU64 = AtomicU64::new(1);

pub struct LogHistory {
    entries: Mutex<VecDeque<String>>,
}

impl LogHistory {
    fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(HISTORY_SIZE)),
        }
    }

    fn push(&self, entry: String) {
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() >= HISTORY_SIZE {
                entries.pop_front();
            }
            entries.push_back(entry);
        }
    }

    pub fn recent(&self) -> Vec<String> {
        self.entries
            .lock()
            .map(|e| e.iter().cloned().collect())
            .unwrap_or_default()
    }
}

pub struct BroadcastLayer {
    tx: broadcast::Sender<String>,
    history: Arc<LogHistory>,
}

impl BroadcastLayer {
    pub fn new(tx: broadcast::Sender<String>) -> (Self, Arc<LogHistory>) {
        let history = Arc::new(LogHistory::new());
        (
            Self {
                tx,
                history: history.clone(),
            },
            history,
        )
    }
}

impl<S> Layer<S> for BroadcastLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let meta = event.metadata();
        let level = meta.level().as_str();
        let target = meta.target();

        let mut visitor = MessageVisitor {
            message: String::new(),
        };
        event.record(&mut visitor);

        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);

        let json = match serde_json::to_string(&serde_json::json!({
            "seq": seq,
            "ts": ts,
            "level": level,
            "target": target,
            "message": visitor.message,
        })) {
            Ok(j) => j,
            Err(_) => return,
        };

        self.history.push(json.clone());

        if self.tx.receiver_count() > 0 {
            let _ = self.tx.send(json);
        }
    }
}

struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else if self.message.is_empty() {
            self.message = format!("{}: {value:?}", field.name());
        } else {
            self.message
                .push_str(&format!(" {}: {value:?}", field.name()));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else if self.message.is_empty() {
            self.message = format!("{}: {value}", field.name());
        } else {
            self.message
                .push_str(&format!(" {}: {value}", field.name()));
        }
    }
}
