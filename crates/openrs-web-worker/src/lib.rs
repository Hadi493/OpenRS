//! Platform-neutral message handling for the OpenRS storage worker.
//!
//! # Why this is a library
//!
//! The worker itself is browser-only: it owns the OPFS database and talks to the
//! UI thread through `postMessage`. None of that is needed to decide *what* a
//! message means, though. [`handle_message`] takes a [`Repository`] and the text
//! the UI sent, performs the checkout, and returns the text to post back. That
//! keeps the interesting part testable natively and in Chromium, where the
//! storage guarantee suite already runs, without starting a worker.
//!
//! # Protocol
//!
//! | Step | Message | Code |
//! |---|---|---|
//! | UI → worker | `openrs-checkout 1` + `L` lines | [`decode_checkout_request`] |
//! | worker → UI | `openrs-checkout-reply 1` + `S` + catalog | [`checkout_reply_message`] |
//! | worker → UI | `openrs-checkout-reply 1` + `E` | [`checkout_failure_message`] |
//!
//! Formats and escaping live in `openrs_core::wire`, so the UI and the worker
//! cannot drift apart.
//!
//! # Failure rules
//!
//! - A rejected checkout writes nothing (one transaction, see `openrs-storage`)
//!   and replies with the attempt's key, so the UI keeps the bill and can retry.
//! - A recorded sale whose catalog reload fails still replies with an error, but
//!   the error says the sale was recorded. Retrying is safe because the key is
//!   reused: storage replays the sale instead of creating a second one.
//! - A message that cannot be decoded replies with an empty key. The web shell
//!   reports the reason against the attempt it sent, because it knows the key.

use openrs_core::{
    CheckoutReceipt, checkout_failure_message, checkout_reply_message, decode_checkout_request,
};
use openrs_storage::{CheckoutOutcome, Repository};

/// Handles one message from the UI thread and returns the reply to post back.
///
/// The caller owns the message transport; this function owns the decision. It
/// never panics on bad input: an unreadable request becomes a failure reply.
///
/// The reply on success includes the **whole catalog after the sale**, so the UI
/// does not need a second round trip to show the new stock counts.
pub fn handle_message(repository: &mut dyn Repository, text: &str) -> String {
    let request = match decode_checkout_request(text) {
        Ok(request) => request,
        Err(error) => {
            return checkout_failure_message(
                "",
                &format!("the storage worker could not read the request: {error}"),
            );
        }
    };

    match repository.checkout(&request) {
        Ok(CheckoutOutcome::Completed(sale) | CheckoutOutcome::Replayed(sale)) => {
            let receipt = CheckoutReceipt {
                sale_id: sale.id,
                checkout_id: sale.checkout_id,
                total_minor: sale.total_minor,
                currency: sale.currency,
            };

            match repository.load_catalog() {
                Ok(catalog) => checkout_reply_message(&receipt, &catalog),
                Err(error) => checkout_failure_message(
                    &receipt.checkout_id,
                    &format!(
                        "the sale {} was recorded, but the catalog could not be reloaded: {error}",
                        receipt.sale_id
                    ),
                ),
            }
        }
        Err(error) => checkout_failure_message(&request.checkout_id, &error.to_string()),
    }
}
