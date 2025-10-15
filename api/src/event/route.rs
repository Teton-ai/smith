use crate::State;
use axum::{
    Error, Extension,
    response::sse::{Event, Sse},
};
use futures::stream::Stream;
use std::time::Duration;
use tokio_stream::{StreamExt as _, wrappers::BroadcastStream};

const EVENTS_TAG: &str = "events";

#[utoipa::path(
    get,
    path = "/events",
    responses(
        (status = StatusCode::OK, description = "Event stream retrieved successfully"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to initiate event stream"),
    ),
    tag = EVENTS_TAG
)]
pub async fn sse_handler(
    Extension(state): Extension<State>,
) -> Sse<impl Stream<Item = Result<Event, Error>>> {
    let tx_message = state.public_events;

    let guard = tx_message.lock().await;
    let rx_message = (*guard).subscribe();
    let stream = BroadcastStream::new(rx_message);

    let stream = stream.map(move |item| Event::default().json_data(item.unwrap()));

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(1))
            .text("keep-alive-text"),
    )
}
