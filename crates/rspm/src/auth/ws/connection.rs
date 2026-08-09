//! Reconnecting authenticated socket transport loop.
use crate::auth::ws::{
    AuthenticatedUserEventBatch, AuthenticatedUserFrameReceipt, AuthenticatedUserWsConfig,
    MAX_FRAME_BYTES, SOCKET_CLOSE_TIMEOUT, retirement::close_socket_within, state_cell::StateCell,
};
use futures::{SinkExt as _, StreamExt as _};
use std::{future::Future, time::Duration};
use tokio::{
    sync::{mpsc, watch},
    time::{Instant, MissedTickBehavior},
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};

pub(super) enum SocketPoll<T> {
    Received {
        output: T,
        receipt: Option<AuthenticatedUserFrameReceipt>,
    },
    Shutdown,
}

pub(super) async fn connection_loop(
    endpoint: String,
    subscription: String,
    config: AuthenticatedUserWsConfig,
    state: StateCell,
    event_tx: mpsc::Sender<AuthenticatedUserEventBatch>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let socket_config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES));
    let mut reconnect_attempt = 0_u32;
    let mut reconnect_delay = config.initial_reconnect_delay;

    loop {
        if *shutdown_rx.borrow() {
            break;
        }
        state.mark_connecting();
        let connected = tokio::select! {
            result = tokio::time::timeout(
                config.connect_timeout,
                connect_async_with_config(&endpoint, Some(socket_config), false),
            ) => Some(result),
            changed = shutdown_rx.changed() => {
                if changed.is_err() {
                    state.mark_disconnected();
                }
                None
            }
        };
        if connected.is_none() {
            break;
        }
        let Some(Ok(Ok((mut socket, _response)))) = connected else {
            if *shutdown_rx.borrow() {
                break;
            }
            state.mark_disconnected();
            let Some(next_attempt) = next_reconnect_attempt(reconnect_attempt) else {
                state.mark_evidence_gap();
                break;
            };
            reconnect_attempt = next_attempt;
            if reconnect_exhausted(config.max_reconnect_attempts, reconnect_attempt)
                || wait_or_shutdown(reconnect_delay, &mut shutdown_rx).await
            {
                break;
            }
            let Some(next_delay) = doubled_delay(reconnect_delay, config.max_reconnect_delay)
            else {
                state.mark_evidence_gap();
                break;
            };
            reconnect_delay = next_delay;
            continue;
        };

        let Ok(generation) = state.try_mark_connected() else {
            break;
        };
        let subscribed = tokio::select! {
            result = tokio::time::timeout(
                config.connect_timeout,
                socket.send(Message::Text(subscription.clone().into())),
            ) => Some(result),
            changed = shutdown_rx.changed() => {
                if changed.is_err() {
                    state.mark_disconnected();
                }
                None
            }
        };
        if subscribed.is_none() {
            state.mark_disconnected();
            break;
        }
        if !matches!(subscribed, Some(Ok(Ok(())))) {
            state.mark_disconnected();
        } else {
            state.mark_subscription_written(generation);
            let proof_ping = tokio::select! {
                result = tokio::time::timeout(
                    config.connect_timeout,
                    socket.send(Message::Text("PING".into())),
                ) => Some(result),
                changed = shutdown_rx.changed() => {
                    if changed.is_err() {
                        state.mark_disconnected();
                    }
                    None
                }
            };
            if !matches!(proof_ping, Some(Ok(Ok(())))) {
                state.mark_disconnected();
            } else {
                reconnect_attempt = 0;
                reconnect_delay = config.initial_reconnect_delay;
                let mut heartbeat = tokio::time::interval(config.heartbeat_interval);
                heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
                heartbeat.tick().await;
                let mut outstanding_ping_since = Some(Instant::now());

                loop {
                    if *shutdown_rx.borrow() {
                        if !close_socket_within(socket.close(None), SOCKET_CLOSE_TIMEOUT).await {
                            state.mark_disconnected();
                        }
                        break;
                    }

                    // Capacity is owned before the socket is polled. Once a
                    // data frame exists in this process, its batch transfers
                    // synchronously into the reserved slot; cancellation can
                    // only win before receipt.
                    let permit = tokio::select! {
                        biased;
                        () = event_tx.closed() => {
                            state.mark_consumer_closed();
                            break;
                        }
                        changed = shutdown_rx.changed() => {
                            if changed.is_err() || *shutdown_rx.borrow() {
                                if !close_socket_within(
                                    socket.close(None),
                                    SOCKET_CLOSE_TIMEOUT,
                                )
                                .await
                                {
                                    state.mark_disconnected();
                                }
                                break;
                            }
                            continue;
                        }
                        _ = heartbeat.tick() => {
                            if outstanding_ping_since
                                .is_some_and(|sent_at| sent_at.elapsed() >= config.heartbeat_timeout)
                            {
                                break;
                            }
                            if outstanding_ping_since.is_none() {
                                if socket.send(Message::Text("PING".into())).await.is_err() {
                                    break;
                                }
                                outstanding_ping_since = Some(Instant::now());
                            }
                            continue;
                        }
                        permit = event_tx.reserve() => {
                            match permit {
                                Ok(permit) => permit,
                                Err(_) => {
                                    state.mark_consumer_closed();
                                    break;
                                }
                            }
                        }
                    };

                    let raw_capacity = tokio::select! {
                        biased;
                        changed = shutdown_rx.changed() => {
                            if changed.is_err() || *shutdown_rx.borrow() {
                                if !close_socket_within(
                                    socket.close(None),
                                    SOCKET_CLOSE_TIMEOUT,
                                )
                                .await
                                {
                                    state.mark_disconnected();
                                }
                                break;
                            }
                            continue;
                        }
                        _ = heartbeat.tick() => {
                            if outstanding_ping_since
                                .is_some_and(|sent_at| sent_at.elapsed() >= config.heartbeat_timeout)
                            {
                                break;
                            }
                            if outstanding_ping_since.is_none() {
                                if socket.send(Message::Text("PING".into())).await.is_err() {
                                    break;
                                }
                                outstanding_ping_since = Some(Instant::now());
                            }
                            continue;
                        }
                        capacity = state.reserve_raw_frame_capacity() => {
                            let Some(capacity) = capacity else {
                                state.mark_evidence_gap();
                                break;
                            };
                            capacity
                        }
                    };

                    let socket_poll = tokio::select! {
                        biased;
                        socket_poll = receive_or_shutdown(socket.next(), &mut shutdown_rx) => {
                            socket_poll
                        }
                        _ = heartbeat.tick() => {
                            if outstanding_ping_since
                                .is_some_and(|sent_at| sent_at.elapsed() >= config.heartbeat_timeout)
                            {
                                break;
                            }
                            if outstanding_ping_since.is_none() {
                                if socket.send(Message::Text("PING".into())).await.is_err() {
                                    break;
                                }
                                outstanding_ping_since = Some(Instant::now());
                            }
                            continue;
                        }
                    };

                    match socket_poll {
                        SocketPoll::Shutdown => {
                            if !close_socket_within(socket.close(None), SOCKET_CLOSE_TIMEOUT).await
                            {
                                state.mark_disconnected();
                            }
                            break;
                        }
                        SocketPoll::Received {
                            output: message,
                            receipt,
                        } => {
                            let Some(receipt) = receipt else {
                                state.mark_evidence_gap();
                                break;
                            };
                            let batch = match message {
                                Some(Ok(Message::Text(text))) if text.as_str() == "PONG" => {
                                    if outstanding_ping_since.take().is_some() {
                                        state.mark_server_liveness_proven(generation);
                                    }
                                    continue;
                                }
                                // The documented user-channel heartbeat is the text
                                // PING/PONG pair. A control-frame Pong may be emitted
                                // by an intermediary and cannot prove this generation.
                                Some(Ok(Message::Pong(_))) => continue,
                                Some(Ok(Message::Ping(payload))) => {
                                    if socket.send(Message::Pong(payload)).await.is_err() {
                                        break;
                                    }
                                    continue;
                                }
                                Some(Ok(Message::Text(text))) => {
                                    AuthenticatedUserEventBatch::capture_text_frame(text.as_bytes())
                                }
                                Some(Ok(Message::Binary(bytes))) => {
                                    AuthenticatedUserEventBatch::capture_binary_frame(&bytes)
                                }
                                Some(Ok(Message::Frame(frame))) => {
                                    AuthenticatedUserEventBatch::capture_raw_frame(frame.payload())
                                }
                                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                            };
                            let event_count = usize::try_from(batch.sequence_width())
                                .expect("bounded authenticated frame width fits usize");
                            let Some(sequence_range) =
                                state.reserve_transport_sequences(event_count)
                            else {
                                state.mark_delivery_gap();
                                if !close_socket_within(socket.close(None), SOCKET_CLOSE_TIMEOUT)
                                    .await
                                {
                                    state.mark_disconnected();
                                }
                                break;
                            };
                            let Some(frame_sequence) = state.reserve_raw_frame_sequence() else {
                                state.mark_evidence_gap();
                                break;
                            };
                            let batch = batch.with_transport_context(
                                receipt,
                                frame_sequence,
                                sequence_range.first,
                                generation,
                                state.snapshot().gap_version,
                            );
                            let frame_gap = batch.frame_gap();
                            if !state.retain_raw_frame(batch.raw_evidence(), raw_capacity) {
                                state.mark_evidence_gap();
                                break;
                            }
                            permit.send(batch);
                            state.mark_enqueued(sequence_range);
                            if frame_gap.is_some() {
                                state.mark_schema_gap();
                                if !close_socket_within(socket.close(None), SOCKET_CLOSE_TIMEOUT)
                                    .await
                                {
                                    state.mark_disconnected();
                                }
                                break;
                            }
                        }
                    }
                }
                state.mark_disconnected();
            }
        }

        if *shutdown_rx.borrow() {
            break;
        }
        let Some(next_attempt) = next_reconnect_attempt(reconnect_attempt) else {
            state.mark_evidence_gap();
            break;
        };
        reconnect_attempt = next_attempt;
        if reconnect_exhausted(config.max_reconnect_attempts, reconnect_attempt)
            || wait_or_shutdown(reconnect_delay, &mut shutdown_rx).await
        {
            break;
        }
        let Some(next_delay) = doubled_delay(reconnect_delay, config.max_reconnect_delay) else {
            state.mark_evidence_gap();
            break;
        };
        reconnect_delay = next_delay;
    }
    state.mark_disconnected();
}

pub(super) async fn receive_or_shutdown<MessageFuture, Output>(
    message: MessageFuture,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> SocketPoll<Output>
where
    MessageFuture: Future<Output = Output>,
{
    tokio::pin!(message);
    loop {
        tokio::select! {
            biased;
            message = &mut message => return SocketPoll::Received {
                output: message,
                receipt: AuthenticatedUserFrameReceipt::capture(),
            },
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    return SocketPoll::Shutdown;
                }
            }
        }
    }
}

fn reconnect_exhausted(max_attempts: Option<u32>, attempts: u32) -> bool {
    max_attempts.is_some_and(|maximum| attempts >= maximum)
}

fn next_reconnect_attempt(current: u32) -> Option<u32> {
    current.checked_add(1)
}

fn doubled_delay(current: Duration, maximum: Duration) -> Option<Duration> {
    current.checked_mul(2).map(|delay| delay.min(maximum))
}

async fn wait_or_shutdown(delay: Duration, shutdown_rx: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        () = tokio::time::sleep(delay) => false,
        changed = shutdown_rx.changed() => changed.is_err() || *shutdown_rx.borrow(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_attempt_exhaustion_cannot_wrap() {
        assert_eq!(next_reconnect_attempt(u32::MAX), None);
        assert_eq!(next_reconnect_attempt(u32::MAX - 1), Some(u32::MAX));
    }

    #[test]
    fn reconnect_delay_overflow_is_explicit() {
        assert_eq!(doubled_delay(Duration::MAX, Duration::MAX), None);
        assert_eq!(
            doubled_delay(Duration::from_secs(2), Duration::from_secs(3)),
            Some(Duration::from_secs(3))
        );
    }
}
