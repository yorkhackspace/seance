//! # planchette
//!
//! Receives a design file as a sequence of bytes and writes it to `/dev/usb/lp0`

use std::path::PathBuf;

use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use planchette::PrintJob;
use seance::{cut_file, svg::parse_svg, SendToDeviceError, ToolPass};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route(
            "/",
            get("You are lost wayward traveller, there is naught for you here"),
        )
        .route("/jobs", post(send_file_to_device));

    // First recorded evidence of a Séance, in the writing of Arthur Young dated 1789.
    let listener = tokio::net::TcpListener::bind("0.0.0.0:1789").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Handles requests to send a file to the attached device.
async fn send_file_to_device(Json(mut payload): Json<PrintJob>) -> impl IntoResponse {
    // We require exactly 16 tool passes, we will truncate if we have too many and otherwise
    // we will create some disabled tool passes at 0 power and 100% speed. They _should_ be
    // skipped but if, for whatever reason, they get used then they (hopefully) won't do
    // anything destructive and will be over quickly.
    let number_of_tool_passes = payload.tool_passes.len();
    match number_of_tool_passes.cmp(&16) {
        std::cmp::Ordering::Greater => {
            payload.tool_passes.truncate(16);
        }
        std::cmp::Ordering::Less => {
            payload.tool_passes.resize(
                16,
                ToolPass::new("skipped".to_string(), 0, 0, 0, 0, 1000, false),
            );
        }
        std::cmp::Ordering::Equal => {}
    }

    let tree = match parse_svg(&payload.design_file) {
        Ok(tree) => tree,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Error parsing design: {err}"),
            )
                .into_response()
        }
    };

    match cut_file(
        &tree,
        &payload.file_name,
        &payload.tool_passes,
        &PathBuf::from("/dev/usb/lp0"),
        &payload.offset,
    ) {
        Ok(_) => (StatusCode::OK,).into_response(),
        Err(SendToDeviceError::ErrorParsingSvg(err)) => (
            StatusCode::BAD_REQUEST,
            format!("Error parsing design: {err}"),
        )
            .into_response(),
        Err(SendToDeviceError::FailedToWriteToPrinter(err)) => {
            (StatusCode::INTERNAL_SERVER_ERROR, err).into_response()
        }
    }
}
