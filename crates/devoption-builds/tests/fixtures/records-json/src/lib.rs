// Trusted build fixture; no runtime execution or database access.
#[allow(unsafe_op_in_unsafe_fn)]
mod bindings;
struct Handler;
impl wasip2::exports::wasi::http::incoming_handler::Guest for Handler {
    fn handle(
        _request: wasip2::http::types::IncomingRequest,
        response: wasip2::http::types::ResponseOutparam,
    ) {
        let value: serde_json::Value = serde_json::from_str(r#"{"status":"ok"}"#).unwrap();
        let _ = bindings::devoption::app::records::append(
            "12345678-1234-4234-8234-123456789abc",
            &value.to_string(),
        );
        let fields = wasip2::http::types::Fields::new();
        let reply = wasip2::http::types::OutgoingResponse::new(fields);
        reply
            .set_status_code(if value["status"] == "ok" { 200 } else { 500 })
            .unwrap();
        wasip2::http::types::ResponseOutparam::set(response, Ok(reply));
    }
}
wasip2::http::proxy::export!(Handler);
