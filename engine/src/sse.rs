//! Tiny SSE helpers. axum's built-in SSE is fine but provider streams come back
//! line-buffered from reqwest, so we just emit `data: ...\n\n` strings ourselves
//! into a bytes-stream Response.

pub fn data_line(payload: &str) -> String {
    format!("data: {}\n\n", payload)
}

pub fn done_line() -> &'static str {
    "data: [DONE]\n\n"
}
