use progenitor_utils::OpenApiSpec;

const TYPE_REPLACEMENTS: &[(&str, &str)] = &[
    (
        "GoogleListCalendarsResponse",
        "hypr_google_calendar::ListCalendarsResponse",
    ),
    (
        "GoogleListEventsResponse",
        "hypr_google_calendar::ListEventsResponse",
    ),
    (
        "OutlookListCalendarsResponse",
        "hypr_outlook_calendar::ListCalendarsResponse",
    ),
    (
        "OutlookListEventsResponse",
        "hypr_outlook_calendar::ListEventsResponse",
    ),
    ("CollectionPage", "hypr_ticket_interface::CollectionPage"),
    ("TicketPage", "hypr_ticket_interface::TicketPage"),
];

// NOTE(fork): this used to re-derive `openapi.gen.json` on every build from
// the hosted backend's master spec at `apps/api/openapi.gen.json`. `apps/api`
// was deleted (see chore: remove server-side apps, backend crates, and
// deploy configs) since this is now a fully local app. `crates/calendar` and
// `plugins/todo` still depend on the generated client types below, so instead
// of deriving from source we build directly off the already-filtered,
// already-3.0-converted spec that was last generated and is committed
// alongside this crate. Regenerate that file (via the old pipeline, against
// a real backend spec) if these types ever need to change.
fn main() {
    let src = concat!(env!("CARGO_MANIFEST_DIR"), "/openapi.gen.json");
    println!("cargo:rerun-if-changed={src}");

    OpenApiSpec::from_path(src).generate_with_replacements("codegen.rs", TYPE_REPLACEMENTS);
}
