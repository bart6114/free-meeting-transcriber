const COMMANDS: &[&str] = &[
    "execute",
    "execute_proxy",
    "execute_transaction",
    "get_meeting",
    "get_meeting_transcript",
    "get_legacy_import_report",
    "list_meetings",
    "subscribe",
    "unsubscribe",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
