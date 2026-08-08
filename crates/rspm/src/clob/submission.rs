//! Opaque engine-owned submission mint bound to one authenticated CLOB client.
use crate::auth::AuthenticatedProtocolAuthority;
use alloc::sync::{Arc, Weak};
use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    task::Context,
    task::Poll,
};
use polymarket::clob::types::response::PostOrderResponse;
use std::sync::{Mutex, MutexGuard};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

static NEXT_CONTROLLER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Eq, PartialEq)]
pub(super) enum PostOrderOutcome {
    Accepted(String),
    NotAccepted,
}

pub(super) fn classify_post_order_response(
    response: PostOrderResponse,
) -> Result<PostOrderOutcome, ()> {
    if response.success {
        let order_id = response.order_id;
        if crate::auth::venue_identifier_is_valid(&order_id) {
            Ok(PostOrderOutcome::Accepted(order_id))
        } else {
            Err(())
        }
    } else if response.order_id.is_empty() {
        Ok(PostOrderOutcome::NotAccepted)
    } else {
        Err(())
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct ActiveSubmissionAuthority {
    revocation_epoch: u64,
    protocol_authority: AuthenticatedProtocolAuthority,
}

struct SubmissionControllerState {
    revocation_epoch: AtomicU64,
    poisoned: AtomicBool,
    active: Mutex<Option<ActiveSubmissionAuthority>>,
    fence: Arc<RwLock<()>>,
}

impl SubmissionControllerState {
    fn lock_active(&self) -> Result<MutexGuard<'_, Option<ActiveSubmissionAuthority>>, ()> {
        match self.active.lock() {
            Ok(mut active) if self.poisoned.load(Ordering::Acquire) => {
                *active = None;
                Err(())
            }
            Ok(active) => Ok(active),
            Err(poison) => {
                let mut active = poison.into_inner();
                *active = None;
                self.poisoned.store(true, Ordering::Release);
                Err(())
            }
        }
    }

    fn poison_and_clear_active(&self) {
        self.poisoned.store(true, Ordering::Release);
        let mut active = match self.active.lock() {
            Ok(active) => active,
            Err(poison) => poison.into_inner(),
        };
        *active = None;
    }
}

/// Client-private identity used to reject capabilities minted for another client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SubmissionControllerBinding(u64);

/// Engine-owned controller that is paired once with an exact [`super::ClobClient`].
///
/// The CLOB client stores only the non-minting binding. Engine admission owns
/// this controller and can mint an attempt capability only while its private
/// recovery, budget, credential, and protocol checks remain exact.
#[derive(Clone)]
pub struct SubmissionController {
    binding: SubmissionControllerBinding,
    state: Arc<SubmissionControllerState>,
}

/// Exclusive open/close lease. While held, no submission capability exists.
#[derive(Debug)]
pub struct SubmissionActivationLease {
    controller: SubmissionController,
    _fence: OwnedRwLockWriteGuard<()>,
}

/// One opaque build-sign-POST capability.
///
/// Fields and minting are private. The capability retains the shared read
/// fence and is bound to one controller ID, revocation epoch, credential
/// identity, and protocol generation.
#[derive(Debug)]
pub struct SubmissionAttemptAuthority {
    binding: SubmissionControllerBinding,
    revocation_epoch: u64,
    protocol_authority: AuthenticatedProtocolAuthority,
    state: Weak<SubmissionControllerState>,
    _fence: OwnedRwLockReadGuard<()>,
}

fn reserve_controller_identity(counter: &AtomicU64) -> crate::Result<u64> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |identity| {
            (identity != 0).then(|| identity.checked_add(1)).flatten()
        })
        .map_err(|_| crate::Error::SubmissionControllerIdentityExhausted)
}

pub(super) fn controller_pair() -> crate::Result<(SubmissionControllerBinding, SubmissionController)>
{
    let identity = reserve_controller_identity(&NEXT_CONTROLLER_ID)?;
    let binding = SubmissionControllerBinding(identity);
    let controller = SubmissionController {
        binding,
        state: Arc::new(SubmissionControllerState {
            revocation_epoch: AtomicU64::new(0),
            poisoned: AtomicBool::new(false),
            active: Mutex::new(None),
            fence: Arc::new(RwLock::new(())),
        }),
    };
    Ok((binding, controller))
}

impl core::fmt::Debug for SubmissionController {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("SubmissionController")
            .field("revocation_epoch", &self.revocation_epoch())
            .field("poisoned", &self.is_poisoned())
            .finish_non_exhaustive()
    }
}

impl SubmissionController {
    /// Publish revocation synchronously before any closer waits for in-flight work.
    pub fn request_revocation(&self) {
        let mut active = match self.state.active.lock() {
            Ok(active) => active,
            Err(poison) => {
                self.state.poisoned.store(true, Ordering::Release);
                poison.into_inner()
            }
        };
        if self
            .state
            .revocation_epoch
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
                epoch.checked_add(1)
            })
            .is_err()
        {
            self.state.poisoned.store(true, Ordering::Release);
        }
        *active = None;
    }

    #[must_use]
    pub fn revocation_epoch(&self) -> u64 {
        self.state.revocation_epoch.load(Ordering::Acquire)
    }

    #[must_use]
    fn is_poisoned(&self) -> bool {
        self.state.poisoned.load(Ordering::Acquire)
    }

    /// Drain every capability minted before this exclusive boundary.
    pub async fn acquire_exclusive(&self) -> SubmissionActivationLease {
        let fence = Arc::clone(&self.state.fence).write_owned().await;
        SubmissionActivationLease {
            controller: self.clone(),
            _fence: fence,
        }
    }

    /// Mint one capability after acquiring the shared fence and rechecking
    /// engine-owned private recovery and budget authority.
    pub async fn authorize_if<F>(
        &self,
        expected_protocol_authority: AuthenticatedProtocolAuthority,
        authorize: F,
    ) -> Option<SubmissionAttemptAuthority>
    where
        F: FnOnce() -> bool,
    {
        let fence = Arc::clone(&self.state.fence).read_owned().await;
        let authorized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(authorize));
        let authorized = match authorized {
            Ok(authorized) => authorized,
            Err(payload) => {
                self.state.poison_and_clear_active();
                std::panic::resume_unwind(payload);
            }
        };
        let Ok(active) = self.state.lock_active() else {
            return None;
        };
        let revocation_epoch = self.revocation_epoch();
        let expected = ActiveSubmissionAuthority {
            revocation_epoch,
            protocol_authority: expected_protocol_authority,
        };
        if !authorized || self.is_poisoned() || *active != Some(expected) {
            drop(active);
            self.request_revocation();
            return None;
        }
        drop(active);
        Some(SubmissionAttemptAuthority {
            binding: self.binding,
            revocation_epoch,
            protocol_authority: expected_protocol_authority,
            state: Arc::downgrade(&self.state),
            _fence: fence,
        })
    }
}

impl SubmissionActivationLease {
    /// Activate the exact authority only if no revocation occurred since the
    /// recovery barrier captured `expected_revocation_epoch`.
    pub fn activate(
        &self,
        expected_revocation_epoch: u64,
        protocol_authority: AuthenticatedProtocolAuthority,
    ) -> bool {
        if self.controller.is_poisoned()
            || self.controller.revocation_epoch() != expected_revocation_epoch
        {
            return false;
        }
        let Ok(mut active) = self.controller.state.lock_active() else {
            return false;
        };
        if self.controller.is_poisoned()
            || active.is_some()
            || self.controller.revocation_epoch() != expected_revocation_epoch
        {
            return false;
        }
        *active = Some(ActiveSubmissionAuthority {
            revocation_epoch: expected_revocation_epoch,
            protocol_authority,
        });
        true
    }
}

impl SubmissionAttemptAuthority {
    pub(super) fn revalidate(
        &self,
        expected_binding: SubmissionControllerBinding,
        expected_protocol_authority: AuthenticatedProtocolAuthority,
    ) -> bool {
        if self.binding != expected_binding
            || self.protocol_authority != expected_protocol_authority
        {
            return false;
        }
        let Some(state) = self.state.upgrade() else {
            return false;
        };
        let Ok(active) = state.lock_active() else {
            return false;
        };
        !state.poisoned.load(Ordering::Acquire)
            && state.revocation_epoch.load(Ordering::Acquire) == self.revocation_epoch
            && *active
                == Some(ActiveSubmissionAuthority {
                    revocation_epoch: self.revocation_epoch,
                    protocol_authority: self.protocol_authority,
                })
    }

    /// Validate authority and perform the first credential-bearing transport
    /// poll while holding the same mutex used by synchronous revocation.
    fn poll_first_transport<F>(
        &self,
        expected_binding: SubmissionControllerBinding,
        expected_protocol_authority: AuthenticatedProtocolAuthority,
        transport: Pin<&mut F>,
        context: &mut Context<'_>,
    ) -> Result<Poll<F::Output>, ()>
    where
        F: Future,
    {
        if self.binding != expected_binding
            || self.protocol_authority != expected_protocol_authority
        {
            return Err(());
        }
        let Some(state) = self.state.upgrade() else {
            return Err(());
        };
        let Ok(mut active) = state.lock_active() else {
            return Err(());
        };
        let expected = ActiveSubmissionAuthority {
            revocation_epoch: self.revocation_epoch,
            protocol_authority: self.protocol_authority,
        };
        if state.poisoned.load(Ordering::Acquire)
            || state.revocation_epoch.load(Ordering::Acquire) != self.revocation_epoch
            || *active != Some(expected)
        {
            return Err(());
        }
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| transport.poll(context)));
        let result = match result {
            Ok(result) => result,
            Err(payload) => {
                *active = None;
                state.poisoned.store(true, Ordering::Release);
                drop(active);
                std::panic::resume_unwind(payload);
            }
        };
        drop(active);
        Ok(result)
    }
}

/// Poll a credential-bearing transport only after atomically linearizing its
/// first poll against submission revocation.
///
/// Constructing an `async fn` future does not poll it. Holding the authority
/// mutex through the first transport poll gives the boundary one exact order:
///
/// - revocation publishes first: validation fails and the transport receives
///   zero polls;
/// - first poll wins: revocation cannot publish until that poll returns, and
///   the attempt's read fence then retains all later in-flight polls.
pub(super) async fn poll_transport_after_begin<F>(
    attempt: &SubmissionAttemptAuthority,
    expected_binding: SubmissionControllerBinding,
    expected_protocol_authority: AuthenticatedProtocolAuthority,
    transport: F,
) -> Result<F::Output, ()>
where
    F: Future,
{
    let mut transport = core::pin::pin!(transport);
    let mut began = false;
    core::future::poll_fn(|context| {
        if !began {
            match attempt.poll_first_transport(
                expected_binding,
                expected_protocol_authority,
                transport.as_mut(),
                context,
            ) {
                Ok(Poll::Ready(output)) => return Poll::Ready(Ok(output)),
                Ok(Poll::Pending) => began = true,
                Err(()) => return Poll::Ready(Err(())),
            }
            return Poll::Pending;
        }
        transport.as_mut().poll(context).map(Ok)
    })
    .await
}
