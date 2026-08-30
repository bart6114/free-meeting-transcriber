const COMMANDS: &[&str] = &[
    "authorization_status",
    "request_full_access",
    "list_todo_lists",
    "fetch_todos",
    "create_todo",
    "complete_todo",
    "delete_todo",
    "github_issue_state",
    "github_issue_detail",
    "github_issue_comments",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
