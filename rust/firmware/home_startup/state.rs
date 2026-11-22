// SPDX-License-Identifier: MIT
//! The same lazy mounted record scope for Home and the separate update boot.
use super::{Board, CardPort, HomeMedium, HomePort, NvsBlobBackend, ReadingStateRouter, StateFacts, nvs};

pub(crate) fn with_state_parts<'sdk, N: nvs::Native, P: CardPort, R>(
    board: &mut Board<'sdk>, nvs: &mut N, card: &mut P, facts: &mut StateFacts,
    operation: impl FnOnce(
        &mut ReadingStateRouter<'_, NvsBlobBackend<'_, N>, HomePort<'_, 'sdk, P>, HomeMedium>,
    ) -> R,
) -> R {
    let mut native = HomePort { board, card };
    let mut medium = HomeMedium;
    let mut store = ReadingStateRouter::new(
        NvsBlobBackend::new(nvs), facts, &mut medium, &mut native,
    );
    operation(&mut store)
}
