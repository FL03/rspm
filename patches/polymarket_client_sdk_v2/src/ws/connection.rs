#![expect(
    clippy::module_name_repetitions,
    reason = "Connection types expose their domain in the name for clarity"
)]

use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use backoff::backoff::Backoff as _;
use futures::{SinkExt as _, StreamExt as _};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::net::TcpStream;
use tokio::sync::{Mutex as AsyncMutex, broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{interval, sleep, timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

use super::config::Config;
use super::error::WsError;
use super::traits::MessageParser;
use crate::auth::Credentials;
use crate::error::Kind;
use crate::ws::WithCredentials;
use crate::{Result, error::Error};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Broadcast channel capacity for incoming messages.
const BROADCAST_CAPACITY: usize = 8192;

static ACTIVE_CONNECTION_TASKS: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_HEARTBEAT_TASKS: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_RECONNECTION_TASKS: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_CONNECTED_SOCKETS: AtomicUsize = AtomicUsize::new(0);

/// Returns the number of live SDK connection-loop tasks in this process.
#[must_use]
pub fn active_connection_tasks() -> usize {
    ACTIVE_CONNECTION_TASKS.load(Ordering::Relaxed)
}

/// Returns the number of live SDK heartbeat tasks in this process.
#[must_use]
pub fn active_heartbeat_tasks() -> usize {
    ACTIVE_HEARTBEAT_TASKS.load(Ordering::Relaxed)
}

/// Returns the number of live SDK subscription reconnection tasks in this process.
#[must_use]
pub fn active_reconnection_tasks() -> usize {
    ACTIVE_RECONNECTION_TASKS.load(Ordering::Relaxed)
}

/// Returns the number of WebSocket streams currently connected by the SDK.
#[must_use]
pub fn active_connected_sockets() -> usize {
    ACTIVE_CONNECTED_SOCKETS.load(Ordering::Relaxed)
}

pub(crate) struct ActivityGuard(&'static AtomicUsize);

impl ActivityGuard {
    fn new(counter: &'static AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }

    pub(crate) fn reconnection_task() -> Self {
        Self::new(&ACTIVE_RECONNECTION_TASKS)
    }
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

struct AbortOnDropTask(Option<JoinHandle<()>>);

impl AbortOnDropTask {
    fn new(handle: JoinHandle<()>) -> Self {
        Self(Some(handle))
    }

    async fn abort_and_join(mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
            let _: std::result::Result<(), tokio::task::JoinError> = handle.await;
        }
    }
}

impl Drop for AbortOnDropTask {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

/// Connection state tracking.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not connected
    Disconnected,
    /// Attempting to connect
    Connecting,
    /// Successfully connected
    Connected {
        /// When the connection was established
        since: Instant,
    },
    /// Reconnecting after failure
    Reconnecting {
        /// Current reconnection attempt number
        attempt: u32,
    },
}

impl ConnectionState {
    /// Check if the connection is currently active.
    #[must_use]
    pub const fn is_connected(self) -> bool {
        matches!(self, Self::Connected { .. })
    }
}

/// Manages WebSocket connection lifecycle, reconnection, and heartbeat.
///
/// This generic connection manager handles all WebSocket connection concerns:
/// - Establishing and maintaining connections
/// - Automatic reconnection with exponential backoff
/// - Heartbeat monitoring via PING/PONG
/// - Broadcasting messages to multiple subscribers
///
/// # Type Parameters
///
/// - `M`: Message type that implements [`DeserializeOwned`] among other "helper" types
/// - `P`: Parser type that implements [`MessageParser<M>`]
///
/// # Example
///
/// ```ignore
/// let parser = SimpleParser;
/// let connection = ConnectionManager::new(
///     "wss://example.com".to_owned(),
///     config,
///     parser,
/// )?;
///
/// // Subscribe to messages
/// let mut rx = connection.subscribe();
/// while let Ok(msg) = rx.recv().await {
///     println!("Received: {:?}", msg);
/// }
/// ```
pub struct ConnectionManager<M, P>
where
    M: DeserializeOwned + Debug + Clone + Send + 'static,
    P: MessageParser<M>,
{
    runtime: Arc<ConnectionRuntime<M>>,
    /// Watch channel receiver for state changes (for use in checking the current state)
    state_rx: watch::Receiver<ConnectionState>,
    /// Phantom data for unused type parameters
    _phantom: PhantomData<P>,
}

impl<M, P> Clone for ConnectionManager<M, P>
where
    M: DeserializeOwned + Debug + Clone + Send + 'static,
    P: MessageParser<M>,
{
    fn clone(&self) -> Self {
        Self {
            runtime: Arc::clone(&self.runtime),
            state_rx: self.state_rx.clone(),
            _phantom: PhantomData,
        }
    }
}

struct ConnectionRuntime<M> {
    state_tx: watch::Sender<ConnectionState>,
    sender_tx: Mutex<Option<mpsc::UnboundedSender<String>>>,
    broadcast_tx: Mutex<Option<broadcast::Sender<M>>>,
    cancellation: CancellationToken,
    task: AsyncMutex<Option<JoinHandle<()>>>,
}

impl<M> ConnectionRuntime<M> {
    fn begin_shutdown(&self) {
        self.cancellation.cancel();
        self.sender_tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.broadcast_tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    async fn shutdown(&self) {
        self.begin_shutdown();
        let mut task = self.task.lock().await;
        if let Some(handle) = task.take() {
            let _: std::result::Result<(), tokio::task::JoinError> = handle.await;
        }
    }
}

impl<M> Drop for ConnectionRuntime<M> {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.sender_tx
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.broadcast_tx
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }
}

impl<M, P> ConnectionManager<M, P>
where
    M: DeserializeOwned + Debug + Clone + Send + 'static,
    P: MessageParser<M>,
{
    /// Create a new connection manager and start the connection loop.
    ///
    /// The `parser` is used to deserialize incoming WebSocket messages.
    /// The connection loop runs in a background task and automatically
    /// handles reconnection according to the config's `ReconnectConfig`.
    pub fn new(endpoint: String, config: Config, parser: P) -> Result<Self> {
        let (sender_tx, sender_rx) = mpsc::unbounded_channel();
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let (state_tx, state_rx) = watch::channel(ConnectionState::Disconnected);
        let cancellation = CancellationToken::new();
        let task_broadcast_tx = broadcast_tx.clone();
        let task_state_tx = state_tx.clone();
        let task_cancellation = cancellation.clone();
        let handle = tokio::spawn(async move {
            let _task_guard = ActivityGuard::new(&ACTIVE_CONNECTION_TASKS);
            Self::connection_loop(
                endpoint,
                config,
                sender_rx,
                task_broadcast_tx,
                parser,
                task_state_tx,
                task_cancellation,
            )
            .await;
        });
        let runtime = Arc::new(ConnectionRuntime {
            state_tx: state_tx.clone(),
            sender_tx: Mutex::new(Some(sender_tx)),
            broadcast_tx: Mutex::new(Some(broadcast_tx.clone())),
            cancellation: cancellation.clone(),
            task: AsyncMutex::new(Some(handle)),
        });

        Ok(Self {
            runtime,
            state_rx,
            _phantom: PhantomData,
        })
    }

    /// Main connection loop with automatic reconnection.
    async fn connection_loop(
        endpoint: String,
        config: Config,
        mut sender_rx: mpsc::UnboundedReceiver<String>,
        broadcast_tx: broadcast::Sender<M>,
        parser: P,
        state_tx: watch::Sender<ConnectionState>,
        cancellation: CancellationToken,
    ) {
        let mut attempt = 0_u32;
        let mut backoff: backoff::ExponentialBackoff = config.reconnect.clone().into();

        loop {
            if cancellation.is_cancelled() || sender_rx.is_closed() {
                break;
            }

            let state_rx = state_tx.subscribe();
            _ = state_tx.send(ConnectionState::Connecting);

            let connection = tokio::select! {
                () = cancellation.cancelled() => break,
                connection = connect_async(&endpoint) => connection,
            };

            match connection {
                Ok((ws_stream, _)) => {
                    let _socket_guard = ActivityGuard::new(&ACTIVE_CONNECTED_SOCKETS);
                    attempt = 0;
                    backoff.reset();
                    _ = state_tx.send(ConnectionState::Connected {
                        since: Instant::now(),
                    });

                    // Handle connection
                    if let Err(e) = Self::handle_connection(
                        ws_stream,
                        &mut sender_rx,
                        &broadcast_tx,
                        state_rx,
                        config.clone(),
                        &parser,
                        cancellation.clone(),
                    )
                    .await
                    {
                        #[cfg(feature = "tracing")]
                        tracing::error!("Error handling connection: {e:?}");
                        #[cfg(not(feature = "tracing"))]
                        let _: &_ = &e;
                    }
                }
                Err(e) => {
                    let error = Error::with_source(Kind::WebSocket, WsError::Connection(e));
                    #[cfg(feature = "tracing")]
                    tracing::warn!("Unable to connect: {error:?}");
                    #[cfg(not(feature = "tracing"))]
                    let _: &_ = &error;
                    attempt = attempt.saturating_add(1);
                }
            }

            // Check if we should stop reconnecting
            if let Some(max) = config.reconnect.max_attempts
                && attempt >= max
            {
                _ = state_tx.send(ConnectionState::Disconnected);
                break;
            }

            // Update state and wait with exponential backoff
            _ = state_tx.send(ConnectionState::Reconnecting { attempt });

            if let Some(duration) = backoff.next_backoff() {
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    () = sleep(duration) => {}
                }
            }
        }

        _ = state_tx.send(ConnectionState::Disconnected);
    }

    /// Handle an active WebSocket connection.
    async fn handle_connection(
        ws_stream: WsStream,
        sender_rx: &mut mpsc::UnboundedReceiver<String>,
        broadcast_tx: &broadcast::Sender<M>,
        state_rx: watch::Receiver<ConnectionState>,
        config: Config,
        parser: &P,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let (mut write, mut read) = ws_stream.split();

        // Channel to notify heartbeat loop when PONG is received
        let (pong_tx, pong_rx) = watch::channel(Instant::now());
        let (ping_tx, mut ping_rx) = mpsc::unbounded_channel();

        let heartbeat_cancellation = cancellation.child_token();
        let heartbeat_task_cancellation = heartbeat_cancellation.clone();
        let heartbeat_task = AbortOnDropTask::new(tokio::spawn(async move {
            let _task_guard = ActivityGuard::new(&ACTIVE_HEARTBEAT_TASKS);
            Self::heartbeat_loop(
                ping_tx,
                state_rx,
                &config,
                pong_rx,
                heartbeat_task_cancellation,
            )
            .await;
        }));

        let result = loop {
            tokio::select! {
                biased;

                // Drain already-enqueued messages before honoring shutdown. Closing
                // the sender is the shutdown signal, and queued unsubscribe frames
                // must still reach an active peer.
                text = sender_rx.recv() => {
                    match text {
                        Some(text) => {
                            if write.send(Message::Text(text.into())).await.is_err() {
                                break Ok(());
                            }
                        }
                        None => break Ok(()),
                    }
                }

                () = cancellation.cancelled() => {
                    break Ok(());
                }
                // Handle incoming messages
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) if text == "PONG" => {
                            _ = pong_tx.send(Instant::now());
                        }
                        Some(Ok(Message::Text(text))) => {
                            #[cfg(feature = "tracing")]
                            tracing::trace!(%text, "Received WebSocket text message");

                            // Parse messages using the provided parser
                            match parser.parse(text.as_bytes()) {
                                Ok(messages) => {
                                    for message in messages {
                                        #[cfg(feature = "tracing")]
                                        tracing::trace!(?message, "Parsed WebSocket message");
                                        _ = broadcast_tx.send(message);
                                    }
                                }
                                Err(e) => {
                                    #[cfg(feature = "tracing")]
                                    tracing::warn!(%text, error = %e, "Failed to parse WebSocket message");
                                    #[cfg(not(feature = "tracing"))]
                                    let _: (&_, &_) = (&text, &e);
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            break Err(Error::with_source(
                                Kind::WebSocket,
                                WsError::ConnectionClosed,
                            ));
                        }
                        Some(Err(e)) => {
                            break Err(Error::with_source(
                                Kind::WebSocket,
                                WsError::Connection(e),
                            ));
                        }
                        _ => {
                            // Ignore binary frames and unsolicited PONG replies.
                        }
                    }
                }

                // Handle PING requests from heartbeat loop
                Some(()) = ping_rx.recv() => {
                    if write.send(Message::Text("PING".into())).await.is_err() {
                        break Ok(());
                    }
                }

                // Check if connection is still active
                else => {
                    break Ok(());
                }
            }
        };

        heartbeat_cancellation.cancel();
        heartbeat_task.abort_and_join().await;

        result
    }

    /// Heartbeat loop that sends PING messages and monitors PONG responses.
    async fn heartbeat_loop(
        ping_tx: mpsc::UnboundedSender<()>,
        state_rx: watch::Receiver<ConnectionState>,
        config: &Config,
        mut pong_rx: watch::Receiver<Instant>,
        cancellation: CancellationToken,
    ) {
        let mut ping_interval = interval(config.heartbeat_interval);

        loop {
            tokio::select! {
                () = cancellation.cancelled() => break,
                _ = ping_interval.tick() => {}
            }

            // Check if still connected
            if !state_rx.borrow().is_connected() {
                break;
            }

            // Mark current PONG state as seen before sending PING
            // This prevents changed() from returning immediately due to a stale PONG
            drop(pong_rx.borrow_and_update());

            // Send PING request to message loop
            let ping_sent = Instant::now();
            if ping_tx.send(()).is_err() {
                // Message loop has terminated
                break;
            }

            // Wait for PONG within timeout
            let pong_result = tokio::select! {
                () = cancellation.cancelled() => break,
                result = timeout(config.heartbeat_timeout, pong_rx.changed()) => result,
            };

            match pong_result {
                Ok(Ok(())) => {
                    let last_pong = *pong_rx.borrow_and_update();
                    if last_pong < ping_sent {
                        #[cfg(feature = "tracing")]
                        tracing::debug!(
                            "PONG received but older than last PING, connection may be stale"
                        );
                        break;
                    }
                }
                Ok(Err(_)) => {
                    // Channel closed, connection is terminating
                    break;
                }
                Err(_) => {
                    // Timeout waiting for PONG
                    #[cfg(feature = "tracing")]
                    tracing::warn!(
                        "Heartbeat timeout: no PONG received within {:?}",
                        config.heartbeat_timeout
                    );
                    break;
                }
            }
        }
    }

    /// Send a subscription request to the WebSocket server.
    pub fn send<R: Serialize>(&self, request: &R) -> Result<()> {
        let json = serde_json::to_string(request)?;
        self.runtime
            .sender_tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .ok_or(WsError::ConnectionClosed)?
            .send(json)
            .map_err(|_e| WsError::ConnectionClosed)?;
        Ok(())
    }

    /// Send a subscription request to the WebSocket server.
    pub fn send_authenticated<R: WithCredentials>(
        &self,
        request: &R,
        credentials: &Credentials,
    ) -> Result<()> {
        let json = request.as_authenticated(credentials)?;
        self.runtime
            .sender_tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .ok_or(WsError::ConnectionClosed)?
            .send(json)
            .map_err(|_e| WsError::ConnectionClosed)?;
        Ok(())
    }

    /// Get the current connection state.
    #[must_use]
    pub fn state(&self) -> ConnectionState {
        *self.state_rx.borrow()
    }

    /// Subscribe to incoming messages.
    ///
    /// Each call returns a new independent receiver. Multiple subscribers can
    /// receive messages concurrently without blocking each other.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<M> {
        self.runtime
            .broadcast_tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map_or_else(
                || {
                    let (sender, receiver) = broadcast::channel(1);
                    drop(sender);
                    receiver
                },
                broadcast::Sender::subscribe,
            )
    }

    /// Subscribe to connection state changes.
    ///
    /// Returns a receiver that notifies when the connection state changes.
    /// This is useful for detecting reconnections and re-establishing subscriptions.
    #[must_use]
    pub fn state_receiver(&self) -> watch::Receiver<ConnectionState> {
        self.runtime.state_tx.subscribe()
    }

    /// Stop this shared connection and wait for its connection and heartbeat tasks.
    ///
    /// Shutdown is terminal, idempotent, and shared by all clones. Calls made after
    /// shutdown return [`WsError::ConnectionClosed`].
    pub async fn shutdown(&self) {
        self.runtime.shutdown().await;
    }

    pub(crate) fn begin_shutdown(&self) {
        self.runtime.begin_shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast::error::TryRecvError;

    #[test]
    fn unread_burst_holds_8192_and_reports_the_8193rd_as_lagged() {
        let (sender, mut receiver) = broadcast::channel::<usize>(BROADCAST_CAPACITY);
        for value in 0..8_192 {
            sender.send(value).expect("receiver remains subscribed");
        }

        for expected in 0..8_192 {
            assert_eq!(
                receiver.try_recv(),
                Ok(expected),
                "an unread burst of 8192 frames must not lag"
            );
        }
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));

        let (sender, mut receiver) = broadcast::channel::<usize>(BROADCAST_CAPACITY);
        for value in 0..8_193 {
            sender.send(value).expect("receiver remains subscribed");
        }
        assert_eq!(
            receiver.try_recv(),
            Err(TryRecvError::Lagged(1)),
            "the 8193rd frame is the explicit one-frame lag boundary"
        );
    }
}
