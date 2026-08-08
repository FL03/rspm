//! Cancellation-safe fallback custody when the process custodian rejects a task.

use futures::FutureExt as _;

use super::*;

struct FailedRetirement {
    task: JoinHandle<()>,
    state: StateCell,
    retirement_state_tx: watch::Sender<RetirementState>,
    _slot: Option<OwnedSemaphorePermit>,
}

static FAILED_RETIREMENTS: OnceLock<Mutex<Vec<FailedRetirement>>> = OnceLock::new();
static FAILED_RETIREMENTS_POISONED: AtomicBool = AtomicBool::new(false);
static FAILED_RETIREMENTS_DRAIN_FAILED: AtomicBool = AtomicBool::new(false);

pub(super) fn failed_retirements_are_poisoned() -> bool {
    FAILED_RETIREMENTS_POISONED.load(Ordering::Acquire)
        || FAILED_RETIREMENTS_DRAIN_FAILED.load(Ordering::Acquire)
}

fn poison_failed_retirements(poisoned: &AtomicBool) {
    poisoned.store(true, Ordering::Release);
    if let Some(Ok(custodian)) = PROCESS_RETIREMENT_CUSTODIAN.get() {
        custodian.poisoned.store(true, Ordering::Release);
    }
}

pub(super) fn retain_failed_retirement(
    task: JoinHandle<()>,
    state: StateCell,
    retirement_state_tx: watch::Sender<RetirementState>,
    slot: Option<OwnedSemaphorePermit>,
) {
    task.abort();
    state.mark_evidence_gap();
    state.mark_disconnected();
    retirement_state_tx.send_replace(RetirementState::RetainedByProcessCustodian);
    let failed = FAILED_RETIREMENTS.get_or_init(|| Mutex::new(Vec::new()));
    let mut failed = match failed.lock() {
        Ok(failed) => failed,
        Err(poison) => {
            poison_failed_retirements(&FAILED_RETIREMENTS_POISONED);
            poison.into_inner()
        }
    };
    failed.push(FailedRetirement {
        task,
        state,
        retirement_state_tx,
        _slot: slot,
    });
}

async fn drain_retirements(
    failed: &Mutex<Vec<FailedRetirement>>,
    poisoned: &AtomicBool,
    drain_failed: &AtomicBool,
) -> bool {
    loop {
        let remaining = {
            let mut failed = match failed.lock() {
                Ok(failed) => failed,
                Err(lock_poison) => {
                    poison_failed_retirements(poisoned);
                    lock_poison.into_inner()
                }
            };
            let mut index = 0;
            while index < failed.len() {
                let completion = (&mut failed[index].task).now_or_never();
                let Some(completion) = completion else {
                    index += 1;
                    continue;
                };
                if completion.is_err_and(|error| !error.is_cancelled()) {
                    drain_failed.store(true, Ordering::Release);
                }
                failed[index].state.mark_disconnected();
                failed[index]
                    .retirement_state_tx
                    .send_replace(RetirementState::TerminalCancelled);
                drop(failed.swap_remove(index));
            }
            failed.len()
        };
        if remaining == 0 {
            return !poisoned.load(Ordering::Acquire) && !drain_failed.load(Ordering::Acquire);
        }
        tokio::task::yield_now().await;
    }
}

pub(super) async fn drain_failed_retirements() -> bool {
    drain_retirements(
        FAILED_RETIREMENTS.get_or_init(|| Mutex::new(Vec::new())),
        &FAILED_RETIREMENTS_POISONED,
        &FAILED_RETIREMENTS_DRAIN_FAILED,
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    struct ExitProbe(Arc<AtomicUsize>);

    impl Drop for ExitProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    #[tokio::test]
    async fn cancelled_drain_retains_every_unfinished_handle_and_permit_for_retry() {
        let slots = Arc::new(Semaphore::new(2));
        let exits = Arc::new(AtomicUsize::new(0));
        let poisoned = AtomicBool::new(false);
        let drain_failed = AtomicBool::new(false);
        let failed = Mutex::new(Vec::new());
        let mut releases = Vec::new();
        let mut retirements = Vec::new();

        for _ in 0..2 {
            let (release_tx, release_rx) = oneshot::channel::<()>();
            let probe = ExitProbe(Arc::clone(&exits));
            let task = tokio::spawn(async move {
                let _probe = probe;
                let _ = release_rx.await;
            });
            let slot = try_reserve_retirement_slot(Arc::clone(&slots)).expect("test slot");
            let (retirement_state_tx, retirement_state_rx) =
                watch::channel(RetirementState::RetainedByProcessCustodian);
            failed
                .lock()
                .expect("local retirement ledger")
                .push(FailedRetirement {
                    task,
                    state: StateCell::new(),
                    retirement_state_tx,
                    _slot: Some(slot),
                });
            releases.push(release_tx);
            retirements.push(retirement_state_rx);
        }

        let mut first_drain = Box::pin(drain_retirements(&failed, &poisoned, &drain_failed));
        assert!(matches!(
            futures::poll!(&mut first_drain),
            core::task::Poll::Pending
        ));
        drop(first_drain);
        assert_eq!(failed.lock().expect("retained after cancellation").len(), 2);
        assert_eq!(slots.available_permits(), 0);
        assert_eq!(exits.load(Ordering::Acquire), 0);

        for release in releases {
            release.send(()).expect("release retained task");
        }
        assert!(
            tokio::time::timeout(
                Duration::from_secs(1),
                drain_retirements(&failed, &poisoned, &drain_failed),
            )
            .await
            .expect("retry drain must terminate")
        );
        assert!(failed.lock().expect("drained local ledger").is_empty());
        assert_eq!(slots.available_permits(), 2);
        assert_eq!(exits.load(Ordering::Acquire), 2);
        assert!(retirements.iter_mut().all(
            |retirement| *retirement.borrow_and_update() == RetirementState::TerminalCancelled
        ));
    }

    #[tokio::test]
    async fn false_process_result_still_drains_fallback_custody_and_returns_false() {
        let slots = Arc::new(Semaphore::new(1));
        let slot = try_reserve_retirement_slot(Arc::clone(&slots)).expect("test slot");
        let state = StateCell::new();
        let (retirement_state_tx, mut retirement_state_rx) =
            watch::channel(RetirementState::RetainedByProcessCustodian);
        let task = tokio::spawn(core::future::pending::<()>());
        task.abort();
        tokio::task::yield_now().await;

        let failed = Mutex::new(vec![FailedRetirement {
            task,
            state,
            retirement_state_tx,
            _slot: Some(slot),
        }]);
        let poisoned = AtomicBool::new(false);
        let drain_failed = AtomicBool::new(false);
        assert!(
            !drain_retirement_ledgers(
                core::future::ready(false),
                drain_retirements(&failed, &poisoned, &drain_failed),
            )
            .await
        );
        assert!(failed.lock().expect("fallback ledger drained").is_empty());
        assert_eq!(slots.available_permits(), 1);
        assert_eq!(
            *retirement_state_rx.borrow_and_update(),
            RetirementState::TerminalCancelled
        );
    }
}
