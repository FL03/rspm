//! Raw-frame retention and transport durability ledger.

use super::*;

impl StateCell {
    pub(in crate::auth::ws) async fn reserve_raw_frame_capacity(
        &self,
    ) -> Option<OwnedSemaphorePermit> {
        Arc::clone(&self.raw_frame_capacity)
            .acquire_owned()
            .await
            .ok()
    }

    pub(in crate::auth::ws) fn reserve_raw_frame_sequence(&self) -> Option<u64> {
        let mut sequence = None;
        let updated = self.update(|state| {
            let Some(next) = state.raw_frame_sequence.checked_add(1) else {
                Self::fail_counter(state, AuthenticatedUserCounterExhaustion::RawFrameSequence);
                return;
            };
            state.raw_frame_sequence = next;
            sequence = Some(next);
        });
        updated.then_some(sequence).flatten()
    }

    pub(in crate::auth::ws) fn retain_raw_frame(
        &self,
        evidence: AuthenticatedUserRawFrame,
        capacity: OwnedSemaphorePermit,
    ) -> bool {
        let frame_sequence = evidence.frame_sequence();
        if frame_sequence == 0 {
            self.mark_evidence_gap();
            return false;
        }
        let Ok(mut state) = self.lock() else {
            return false;
        };
        let mut raw_frames = match self.raw_frames.lock() {
            Ok(raw_frames) => raw_frames,
            Err(poison) => {
                drop(poison.into_inner());
                self.fail_closed_after_poison(&mut state);
                self.state_tx.send_replace(*state);
                return false;
            }
        };
        let inserted = raw_frames
            .insert(
                frame_sequence,
                PendingRawFrame {
                    evidence,
                    _capacity: capacity,
                },
            )
            .is_none();
        state.pending_raw_frame_count = raw_frames.len();
        if !inserted {
            state.evidence_gap = true;
            Self::advance_gap_version(&mut state);
        }
        let snapshot = *state;
        drop(raw_frames);
        drop(state);
        self.state_tx.send_replace(snapshot);
        inserted
    }

    pub(in crate::auth::ws) fn pending_raw_frames(
        &self,
    ) -> Vec<AuthenticatedUserRawFrame> {
        let (pending, mutex_poisoned) = match self.raw_frames.lock() {
            Ok(raw_frames) => (
                raw_frames
                    .values()
                    .map(|pending| pending.evidence.clone())
                    .collect(),
                false,
            ),
            Err(poison) => {
                let raw_frames = poison.into_inner();
                (
                    raw_frames
                        .values()
                        .map(|pending| pending.evidence.clone())
                        .collect(),
                    true,
                )
            }
        };
        if mutex_poisoned || self.authority_poisoned.load(Ordering::Acquire) {
            self.publish_poisoned();
        }
        pending
    }

    pub(in crate::auth::ws) fn acknowledge_raw_frame_durable(
        &self,
        frame_sequence: u64,
    ) -> bool {
        let Ok(mut state) = self.lock() else {
            return false;
        };
        let mut raw_frames = match self.raw_frames.lock() {
            Ok(raw_frames) => raw_frames,
            Err(poison) => {
                drop(poison.into_inner());
                self.fail_closed_after_poison(&mut state);
                self.state_tx.send_replace(*state);
                return false;
            }
        };
        let removed = raw_frames.remove(&frame_sequence).is_some();
        if removed {
            state.pending_raw_frame_count = raw_frames.len();
        }
        let snapshot = *state;
        drop(raw_frames);
        drop(state);
        self.state_tx.send_replace(snapshot);
        removed
    }

    pub(in crate::auth::ws) fn mark_enqueued(&self, range: TransportSequenceRange) {
        self.update(|state| {
            if range.first != 0
                && range.first <= range.last
                && range.last <= state.transport_sequence
            {
                state.enqueued_sequence = state.enqueued_sequence.max(range.last);
            } else {
                state.delivery_gap = true;
                Self::advance_gap_version(state);
            }
        });
    }

    pub(in crate::auth::ws) fn mark_dropped(
        &self,
        range: TransportSequenceRange,
    ) -> bool {
        let mut retained = false;
        let snapshot = {
            let Ok(mut state) = self.lock() else {
                return false;
            };
            if range.first == 0 || range.first > range.last || range.last > state.transport_sequence
            {
                state.delivery_gap = true;
                Self::advance_gap_version(&mut state);
                *state
            } else {
                let mut ranges = match self.dropped_ranges.lock() {
                    Ok(ranges) => ranges,
                    Err(poison) => {
                        drop(poison.into_inner());
                        self.fail_closed_after_poison(&mut state);
                        self.state_tx.send_replace(*state);
                        return false;
                    }
                };
                if let Some((_, previous_last)) = ranges.last_mut()
                    && previous_last.checked_add(1) == Some(range.first)
                {
                    *previous_last = range.last;
                    retained = true;
                } else if ranges.len() < MAX_DROPPED_RANGES {
                    ranges.push((range.first, range.last));
                    retained = true;
                } else {
                    state.consumer_closed = true;
                }
                state.delivery_gap = true;
                Self::advance_gap_version(&mut state);
                *state
            }
        };
        self.state_tx.send_replace(snapshot);
        retained
    }

    pub(in crate::auth::ws) fn acknowledge_durable(&self, sequence: u64) -> bool {
        let mut acknowledged = true;
        let snapshot = {
            let Ok(mut state) = self.lock() else {
                return false;
            };
            if sequence == 0 || sequence > state.transport_sequence {
                return false;
            }
            if sequence <= state.durable_sequence {
                return true;
            }
            let mut out_of_order = match self.durable_out_of_order.lock() {
                Ok(durable) => durable,
                Err(poison) => {
                    drop(poison.into_inner());
                    self.fail_closed_after_poison(&mut state);
                    self.state_tx.send_replace(*state);
                    return false;
                }
            };
            if out_of_order.len() >= MAX_DURABLE_OUT_OF_ORDER && !out_of_order.contains(&sequence) {
                state.consumer_closed = true;
                state.delivery_gap = true;
                Self::advance_gap_version(&mut state);
                acknowledged = false;
                let snapshot = *state;
                drop(out_of_order);
                drop(state);
                self.state_tx.send_replace(snapshot);
                return false;
            }
            out_of_order.insert(sequence);
            while state.durable_sequence < state.transport_sequence {
                let Ok(next) = Self::next_durable_sequence(state.durable_sequence) else {
                    Self::fail_counter(
                        &mut state,
                        AuthenticatedUserCounterExhaustion::DurableSequence,
                    );
                    acknowledged = false;
                    break;
                };
                if !out_of_order.remove(&next) {
                    break;
                }
                state.durable_sequence = next;
            }
            *state
        };
        self.state_tx.send_replace(snapshot);
        acknowledged
    }
}
