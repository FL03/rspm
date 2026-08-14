//! Bounded process custody for authenticated socket task retirement.

mod failed;

use std::{
    future::Future,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc as std_mpsc,
    },
    time::Duration,
};

use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, oneshot, watch},
    task::JoinHandle,
};

use super::{
    AuthenticatedUserWsError, RETIREMENT_RESPONSE_MARGIN, SESSION_SHUTDOWN_TIMEOUT,
    state_cell::StateCell,
};
use failed::{drain_failed_retirements, failed_retirements_are_poisoned, retain_failed_retirement};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RetirementState {
    Active,
    Requested,
    RetainedByProcessCustodian,
    TerminalGraceful,
    TerminalCancelled,
}

impl RetirementState {
    pub(super) const fn is_terminal(self) -> bool {
        matches!(self, Self::TerminalGraceful | Self::TerminalCancelled)
    }
}

enum ProcessRetirementCommand {
    Retain {
        task: JoinHandle<()>,
        state: StateCell,
        retirement_state_tx: watch::Sender<RetirementState>,
        _slot: OwnedSemaphorePermit,
    },
    Barrier(oneshot::Sender<()>),
}

struct ProcessRetirementCustodian {
    command_tx: std_mpsc::SyncSender<ProcessRetirementCommand>,
    _thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    retained: Arc<AtomicUsize>,
    transfers: Arc<AtomicUsize>,
    poisoned: Arc<AtomicBool>,
    slots: Arc<Semaphore>,
}

/// Linear owner for a session task removed from its session mutex.
///
/// Cancellation or panic drops this guard, which transfers the still-owned
/// handle and permit into process custody instead of detaching the task.
pub(super) struct SessionRetirementGuard {
    task: Option<JoinHandle<()>>,
    state: StateCell,
    retirement_state_tx: watch::Sender<RetirementState>,
    slot: Option<OwnedSemaphorePermit>,
}

impl SessionRetirementGuard {
    pub(super) fn new(
        task: JoinHandle<()>,
        state: StateCell,
        retirement_state_tx: watch::Sender<RetirementState>,
    ) -> Self {
        Self {
            task: Some(task),
            state,
            retirement_state_tx,
            slot: None,
        }
    }

    pub(super) fn install_slot(&mut self, slot: Option<OwnedSemaphorePermit>) {
        self.slot = slot;
    }

    pub(super) fn task(&mut self) -> &mut JoinHandle<()> {
        self.task
            .as_mut()
            .expect("linear session retirement guard owns one task")
    }

    pub(super) fn complete(mut self) {
        drop(self.task.take());
        drop(self.slot.take());
    }
}

impl Drop for SessionRetirementGuard {
    fn drop(&mut self) {
        let Some(task) = self.task.take() else {
            return;
        };
        transfer_to_process_custodian(
            task,
            self.state.clone(),
            self.retirement_state_tx.clone(),
            self.slot.take(),
        );
    }
}

const MAX_PROCESS_RETIREMENTS: usize = 64;
static PROCESS_RETIREMENT_CUSTODIAN: OnceLock<
    Result<ProcessRetirementCustodian, AuthenticatedUserWsError>,
> = OnceLock::new();

pub(super) async fn retirement_custodian<Connection>(
    connection: Connection,
    state: StateCell,
    mut shutdown_rx: watch::Receiver<bool>,
    retirement_state_tx: watch::Sender<RetirementState>,
    grace_timeout: Duration,
) where
    Connection: Future<Output = ()> + Send + 'static,
{
    let mut connection = Box::pin(connection);
    tokio::select! {
        () = &mut connection => {
            retirement_state_tx.send_replace(RetirementState::TerminalGraceful);
            return;
        }
        () = wait_for_shutdown(&mut shutdown_rx) => {}
    }

    retirement_state_tx.send_replace(RetirementState::Requested);
    state.mark_disconnected();
    let grace = tokio::time::sleep(grace_timeout);
    tokio::pin!(grace);
    let disposition = tokio::select! {
        biased;
        () = &mut connection => RetirementState::TerminalGraceful,
        () = &mut grace => RetirementState::TerminalCancelled,
    };
    drop(connection);
    if disposition == RetirementState::TerminalCancelled {
        state.mark_evidence_gap();
    }
    state.mark_disconnected();
    retirement_state_tx.send_replace(disposition);
}

async fn wait_for_shutdown(shutdown_rx: &mut watch::Receiver<bool>) {
    while !*shutdown_rx.borrow_and_update() && shutdown_rx.changed().await.is_ok() {}
}

pub(super) async fn wait_for_retirement_terminal(
    mut retirement: watch::Receiver<RetirementState>,
    timeout: Duration,
) -> bool {
    tokio::time::timeout(timeout, async {
        while !retirement.borrow_and_update().is_terminal() {
            retirement.changed().await.map_err(|_| ())?;
        }
        Ok::<(), ()>(())
    })
    .await
    .is_ok_and(|result| result.is_ok())
}

fn checked_counter_increment(counter: &AtomicUsize, poisoned: &AtomicBool) -> bool {
    if poisoned.load(Ordering::Acquire) {
        return false;
    }
    if counter
        .try_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            value.checked_add(1)
        })
        .is_err()
    {
        poisoned.store(true, Ordering::Release);
        return false;
    }
    true
}

fn checked_counter_decrement(counter: &AtomicUsize, poisoned: &AtomicBool) -> bool {
    if counter
        .try_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            value.checked_sub(1)
        })
        .is_err()
    {
        poisoned.store(true, Ordering::Release);
        return false;
    }
    true
}

fn start_process_retirement_custodian()
-> Result<ProcessRetirementCustodian, AuthenticatedUserWsError> {
    let (command_tx, command_rx) = std_mpsc::sync_channel(MAX_PROCESS_RETIREMENTS);
    let retained = Arc::new(AtomicUsize::new(0));
    let transfers = Arc::new(AtomicUsize::new(0));
    let poisoned = Arc::new(AtomicBool::new(false));
    let slots = Arc::new(Semaphore::new(MAX_PROCESS_RETIREMENTS));
    let thread_retained = Arc::clone(&retained);
    let thread_poisoned = Arc::clone(&poisoned);
    let thread = std::thread::Builder::new()
        .name("rspm-authenticated-ws-retirement".to_owned())
        .spawn(move || {
            while let Ok(command) = command_rx.recv() {
                match command {
                    ProcessRetirementCommand::Retain {
                        task,
                        state,
                        retirement_state_tx,
                        _slot,
                    } => {
                        let terminal = futures::executor::block_on(task);
                        if terminal.is_err() || !retirement_state_tx.borrow().is_terminal() {
                            state.mark_evidence_gap();
                            state.mark_disconnected();
                            retirement_state_tx.send_replace(RetirementState::TerminalCancelled);
                        }
                        if !checked_counter_decrement(&thread_retained, &thread_poisoned) {
                            state.mark_evidence_gap();
                        }
                    }
                    ProcessRetirementCommand::Barrier(completion) => {
                        let _caller_was_dropped = completion.send(()).is_err();
                    }
                }
            }
        })
        .map_err(|_| AuthenticatedUserWsError::RetirementCustodianUnavailable)?;
    Ok(ProcessRetirementCustodian {
        command_tx,
        _thread: Mutex::new(Some(thread)),
        retained,
        transfers,
        poisoned,
        slots,
    })
}

fn process_retirement_custodian()
-> Result<&'static ProcessRetirementCustodian, AuthenticatedUserWsError> {
    match PROCESS_RETIREMENT_CUSTODIAN.get_or_init(start_process_retirement_custodian) {
        Ok(custodian) => Ok(custodian),
        Err(error) => Err(*error),
    }
}

pub(super) fn try_reserve_retirement_slot(
    slots: Arc<Semaphore>,
) -> Result<OwnedSemaphorePermit, AuthenticatedUserWsError> {
    slots
        .try_acquire_owned()
        .map_err(|_| AuthenticatedUserWsError::RetirementCapacityExhausted)
}

pub(super) fn reserve_process_retirement_slot()
-> Result<OwnedSemaphorePermit, AuthenticatedUserWsError> {
    let custodian = process_retirement_custodian()?;
    if custodian.poisoned.load(Ordering::Acquire) || failed_retirements_are_poisoned() {
        return Err(AuthenticatedUserWsError::RetirementCustodianUnavailable);
    }
    try_reserve_retirement_slot(Arc::clone(&custodian.slots))
}

pub(super) fn transfer_to_process_custodian(
    task: JoinHandle<()>,
    state: StateCell,
    retirement_state_tx: watch::Sender<RetirementState>,
    slot: Option<OwnedSemaphorePermit>,
) {
    let custodian = match process_retirement_custodian() {
        Ok(custodian) => custodian,
        Err(_) => {
            retain_failed_retirement(task, state, retirement_state_tx, slot);
            return;
        }
    };
    let Some(slot) = slot else {
        retain_failed_retirement(task, state, retirement_state_tx, None);
        return;
    };
    transfer_to_known_custodian(custodian, task, state, retirement_state_tx, slot);
}

fn transfer_to_known_custodian(
    custodian: &ProcessRetirementCustodian,
    task: JoinHandle<()>,
    state: StateCell,
    retirement_state_tx: watch::Sender<RetirementState>,
    slot: OwnedSemaphorePermit,
) {
    if !checked_counter_increment(&custodian.transfers, &custodian.poisoned)
        || !checked_counter_increment(&custodian.retained, &custodian.poisoned)
    {
        retain_failed_retirement(task, state, retirement_state_tx, Some(slot));
        return;
    }
    retirement_state_tx.send_replace(RetirementState::RetainedByProcessCustodian);
    if let Err(error) = custodian
        .command_tx
        .try_send(ProcessRetirementCommand::Retain {
            task,
            state,
            retirement_state_tx,
            _slot: slot,
        })
    {
        let _counter_valid = checked_counter_decrement(&custodian.retained, &custodian.poisoned);
        let ProcessRetirementCommand::Retain {
            task,
            state,
            retirement_state_tx,
            _slot,
        } = (match error {
            std_mpsc::TrySendError::Full(cmd) | std_mpsc::TrySendError::Disconnected(cmd) => cmd,
        })
        else {
            return;
        };
        retain_failed_retirement(task, state, retirement_state_tx, Some(_slot));
    }
}

async fn drain_retirement_ledgers<ProcessDrain, FailedDrain>(
    process_drain: ProcessDrain,
    failed_drain: FailedDrain,
) -> bool
where
    ProcessDrain: Future<Output = bool>,
    FailedDrain: Future<Output = bool>,
{
    let (process_drained, failed_drained) = tokio::join!(process_drain, failed_drain);
    process_drained && failed_drained
}

/// Wait for all authenticated socket tasks transferred before this call.
pub async fn drain_authenticated_user_ws_retirements() -> bool {
    tokio::time::timeout(
        SESSION_SHUTDOWN_TIMEOUT + RETIREMENT_RESPONSE_MARGIN,
        drain_retirement_ledgers(
            async {
                match PROCESS_RETIREMENT_CUSTODIAN.get() {
                    None => true,
                    Some(Err(_)) => false,
                    Some(Ok(custodian)) => {
                        let (completion_tx, completion_rx) = oneshot::channel();
                        custodian
                            .command_tx
                            .try_send(ProcessRetirementCommand::Barrier(completion_tx))
                            .is_ok()
                            && completion_rx.await.is_ok()
                            && custodian.retained.load(Ordering::Acquire) == 0
                            && !custodian.poisoned.load(Ordering::Acquire)
                    }
                }
            },
            drain_failed_retirements(),
        ),
    )
    .await
    .unwrap_or(false)
}

#[cfg(test)]
pub(super) fn transfer_to_disconnected_custodian_for_test(
    task: JoinHandle<()>,
    state: StateCell,
    retirement_state_tx: watch::Sender<RetirementState>,
    slot: OwnedSemaphorePermit,
) {
    let (command_tx, command_rx) = std_mpsc::sync_channel(1);
    drop(command_rx);
    let custodian = ProcessRetirementCustodian {
        command_tx,
        _thread: Mutex::new(None),
        retained: Arc::new(AtomicUsize::new(0)),
        transfers: Arc::new(AtomicUsize::new(0)),
        poisoned: Arc::new(AtomicBool::new(false)),
        slots: Arc::new(Semaphore::new(0)),
    };
    transfer_to_known_custodian(&custodian, task, state, retirement_state_tx, slot);
}

#[cfg(test)]
pub(super) async fn drain_failed_retirements_for_test() -> bool {
    drain_failed_retirements().await
}

#[cfg(test)]
pub(super) fn process_custodian_transfer_count() -> usize {
    PROCESS_RETIREMENT_CUSTODIAN
        .get()
        .and_then(|result| result.as_ref().ok())
        .map_or(0, |custodian| custodian.transfers.load(Ordering::Acquire))
}

pub(super) async fn close_socket_within<Close, CloseError>(close: Close, timeout: Duration) -> bool
where
    Close: Future<Output = Result<(), CloseError>>,
{
    matches!(tokio::time::timeout(timeout, close).await, Ok(Ok(())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retirement_counter_overflow_is_sticky_and_never_wraps() {
        let counter = AtomicUsize::new(usize::MAX);
        let poisoned = AtomicBool::new(false);
        assert!(!checked_counter_increment(&counter, &poisoned));
        assert!(poisoned.load(Ordering::Acquire));
        assert_eq!(counter.load(Ordering::Acquire), usize::MAX);
        assert!(!checked_counter_increment(&counter, &poisoned));
    }

    #[test]
    fn retirement_counter_underflow_is_sticky_and_never_wraps() {
        let counter = AtomicUsize::new(0);
        let poisoned = AtomicBool::new(false);
        assert!(!checked_counter_decrement(&counter, &poisoned));
        assert!(poisoned.load(Ordering::Acquire));
        assert_eq!(counter.load(Ordering::Acquire), 0);
    }
}
