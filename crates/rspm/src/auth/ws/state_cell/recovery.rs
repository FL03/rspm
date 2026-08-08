//! Authenticated catch-up preparation and compare-and-set commit.

use super::*;

impl StateCell {
    pub(in crate::clob::authenticated_ws) fn prepare_catch_up_finalization(
        &self,
        token: AuthenticatedUserRecoveryToken,
        credential_authority: AuthenticatedCredentialAuthority,
        transport_watermark: u64,
        minimum_liveness_version: u64,
    ) -> Option<AuthenticatedUserCatchUpFinalization> {
        let Ok(mut state) = self.lock() else {
            return None;
        };
        if state.recovery_token() != Some(token)
            || state.consumer_closed
            || state.rest_proof_generation != Some(token.generation())
            || state.rest_credential_authority != Some(credential_authority)
            || state.rest_proof_liveness_floor != Some(minimum_liveness_version)
            || !matches!(
                state.subscription,
                AuthenticatedUserSubscriptionState::ServerResponsive
            )
            || state.transport_sequence != transport_watermark
            || state.liveness_version <= minimum_liveness_version
        {
            return None;
        }

        let durable = match self.durable_out_of_order.lock() {
            Ok(durable) => durable,
            Err(poison) => {
                drop(poison.into_inner());
                self.fail_closed_after_poison(&mut state);
                self.state_tx.send_replace(*state);
                return None;
            }
        };
        let dropped = match self.dropped_ranges.lock() {
            Ok(dropped) => dropped,
            Err(poison) => {
                drop(poison.into_inner());
                drop(durable);
                self.fail_closed_after_poison(&mut state);
                self.state_tx.send_replace(*state);
                return None;
            }
        };
        let mut next_durable = durable.clone();
        let mut next_dropped = dropped.clone();
        let mut durable_sequence = state.durable_sequence;
        loop {
            if durable_sequence >= transport_watermark {
                break;
            }
            let next = Self::next_durable_sequence(durable_sequence).ok()?;
            if next_durable.remove(&next) {
                durable_sequence = next;
                continue;
            }
            let Some((first, last)) = next_dropped.first().copied() else {
                break;
            };
            if next < first {
                break;
            }
            if next <= last {
                durable_sequence = last.min(transport_watermark);
                next_dropped.remove(0);
                continue;
            }
            next_dropped.remove(0);
        }
        if durable_sequence != transport_watermark {
            return None;
        }
        let mut next_state = *state;
        next_state.durable_sequence = durable_sequence;
        next_state.schema_gap = false;
        next_state.delivery_gap = false;
        next_state.evidence_gap = false;
        next_state.catch_up_generation = Some(token.generation());
        if !next_state.is_ready() {
            return None;
        }
        Some(AuthenticatedUserCatchUpFinalization {
            expected_state: *state,
            next_state,
            expected_durable_out_of_order: durable.iter().copied().collect(),
            next_durable_out_of_order: next_durable.into_iter().collect(),
            expected_dropped_ranges: dropped.clone(),
            next_dropped_ranges: next_dropped,
        })
    }

    pub(in crate::clob::authenticated_ws) fn commit_catch_up_finalization(
        &self,
        finalization: AuthenticatedUserCatchUpFinalization,
    ) -> bool {
        let snapshot = {
            let Ok(mut state) = self.lock() else {
                return false;
            };
            let mut durable = match self.durable_out_of_order.lock() {
                Ok(durable) => durable,
                Err(poison) => {
                    drop(poison.into_inner());
                    self.fail_closed_after_poison(&mut state);
                    self.state_tx.send_replace(*state);
                    return false;
                }
            };
            let mut dropped = match self.dropped_ranges.lock() {
                Ok(dropped) => dropped,
                Err(poison) => {
                    drop(poison.into_inner());
                    drop(durable);
                    self.fail_closed_after_poison(&mut state);
                    self.state_tx.send_replace(*state);
                    return false;
                }
            };
            if *state != finalization.expected_state
                || durable
                    .iter()
                    .copied()
                    .ne(finalization.expected_durable_out_of_order.iter().copied())
                || *dropped != finalization.expected_dropped_ranges
            {
                return false;
            }
            *state = finalization.next_state;
            *durable = finalization.next_durable_out_of_order.into_iter().collect();
            *dropped = finalization.next_dropped_ranges;
            *state
        };
        self.state_tx.send_replace(snapshot);
        true
    }

    #[cfg(test)]
    pub(in crate::clob::authenticated_ws) fn complete_catch_up(
        &self,
        token: AuthenticatedUserRecoveryToken,
        transport_watermark: u64,
    ) -> bool {
        let credential_authority =
            AuthenticatedCredentialAuthority::new(token.credential_identity(), 1)
                .expect("test credential generation");
        let Some(liveness_floor) = self.mark_authenticated_rest_proven(token, credential_authority)
        else {
            return false;
        };
        self.mark_server_liveness_proven(token.generation());
        self.prepare_catch_up_finalization(
            token,
            credential_authority,
            transport_watermark,
            liveness_floor,
        )
        .is_some_and(|finalization| self.commit_catch_up_finalization(finalization))
    }

    pub(in crate::clob::authenticated_ws) fn authority_matches(
        &self,
        token: AuthenticatedUserRecoveryToken,
        credential_authority: AuthenticatedCredentialAuthority,
        transport_watermark: u64,
    ) -> bool {
        let state = self.snapshot();
        state.recovery_token() == Some(token)
            && state.rest_credential_authority == Some(credential_authority)
            && state.transport_sequence == transport_watermark
            && state.is_ready()
    }
}
