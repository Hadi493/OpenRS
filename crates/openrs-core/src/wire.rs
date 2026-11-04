//! Text encoding for the messages exchanged with the storage worker.
//!
//! # Why this exists
//!
//! In the browser the SQLite database lives in a dedicated worker, because OPFS
//! file handles only exist there (ADR 0002). That worker has to hand a whole
//! catalog to the UI thread through `postMessage`, which carries JavaScript
//! values, not Rust types. Messages are therefore single strings in the small
//! line-based format defined here, so both sides stay typed Rust and the format
//! can be tested natively.
//!
//! # Messages
//!
//! | Direction | Message | Builder | Decoder |
//! |---|---|---|---|
//! | worker → UI | catalog (startup) | [`catalog_message`] | [`decode_message`] |
//! | worker → UI | startup failure | [`error_message`] | [`decode_message`] |
//! | UI → worker | checkout request | [`checkout_request_message`] | [`decode_checkout_request`] |
//! | worker → UI | checkout recorded | [`checkout_reply_message`] | [`decode_checkout_reply`] |
//! | worker → UI | checkout rejected | [`checkout_failure_message`] | [`decode_checkout_reply`] |
//!
//! # Catalog format
//!
//! ```text
//! openrs-catalog 1
//! P<TAB>id<TAB>name<TAB>category
//! V<TAB>id<TAB>product_id<TAB>sku<TAB>price_minor<TAB>currency<TAB>stock
//! ```
//!
//! # Checkout formats
//!
//! ```text
//! openrs-checkout 1
//! L<TAB>variant_id<TAB>quantity
//!
//! openrs-checkout-reply 1
//! S<TAB>sale_id<TAB>checkout_id<TAB>total_minor<TAB>currency
//! openrs-catalog 1
//! P ... / V ...          (the catalog after the stock deduction)
//!
//! openrs-checkout-reply 1
//! E<TAB>checkout_id<TAB>reason
//! ```
//!
//! Fields are escaped (`\\`, `\t`, `\n`, `\r`) so an id or a name may contain
//! any text. The version in each header is checked: an older peer must reject a
//! newer message instead of misreading it. A rejected checkout carries no
//! catalog; the UI keeps the stock it already shows and retries.
//!
//! Decoding rebuilds the catalog with [`catalog::Catalog::new`], so the snapshot
//! is re-sorted and both sides agree on the order regardless of what the
//! database returned.

use std::fmt;

use crate::catalog::{Catalog, Product, Variant};
use crate::checkout::{CheckoutLine, CheckoutReceipt, CheckoutRequest};

/// Header line of the catalog body. Bump the number when the format changes.
const HEADER: &str = "openrs-catalog 1";

/// Prefix of a successful worker message.
pub const CATALOG_PREFIX: &str = "catalog\n";

/// Prefix of a failed worker message.
pub const ERROR_PREFIX: &str = "error\n";

/// Header of a checkout request sent by the UI to the storage worker.
const CHECKOUT_REQUEST_HEADER: &str = "openrs-checkout 1";

/// Header of a worker reply to a checkout request.
const CHECKOUT_REPLY_HEADER: &str = "openrs-checkout-reply 1";

/// Why a catalog message could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError {
    message: String,
}

impl WireError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WireError {}

/// Encodes a catalog snapshot, header line included.
#[must_use]
pub fn encode_catalog(catalog: &Catalog) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');

    for product in catalog.products() {
        out.push('P');
        for field in [&product.id, &product.name, &product.category] {
            out.push('\t');
            escape_into(&mut out, field);
        }
        out.push('\n');
    }

    for variant in catalog.variants() {
        out.push('V');
        for field in [
            &variant.id,
            &variant.product_id,
            &variant.sku,
            &variant.price_minor.to_string(),
            &variant.currency,
            &variant.stock.to_string(),
        ] {
            out.push('\t');
            escape_into(&mut out, field);
        }
        out.push('\n');
    }

    out
}

/// Decodes a catalog body produced by [`encode_catalog`].
///
/// Fails instead of guessing: an unknown header, an unknown record type, a
/// wrong field count, a non-integer amount, or an invalid escape all produce a
/// [`WireError`]. A malformed message must never turn into a partial catalog.
pub fn decode_catalog(text: &str) -> Result<Catalog, WireError> {
    let mut lines = text.split('\n');

    match lines.next() {
        Some(header) if header == HEADER => {}
        Some(header) => {
            return Err(WireError::new(format!(
                "unsupported catalog header `{header}`"
            )));
        }
        None => return Err(WireError::new("empty catalog message")),
    }

    let mut products = Vec::new();
    let mut variants = Vec::new();

    for line in lines {
        if line.is_empty() {
            continue;
        }

        let mut fields = line.split('\t');
        let record = fields.next().unwrap_or_default();
        let rest = fields
            .map(unescape)
            .collect::<Result<Vec<String>, WireError>>()?;

        match (record, rest.len()) {
            ("P", 3) => products.push(Product {
                id: rest[0].clone(),
                name: rest[1].clone(),
                category: rest[2].clone(),
            }),
            ("V", 6) => variants.push(Variant {
                id: rest[0].clone(),
                product_id: rest[1].clone(),
                sku: rest[2].clone(),
                price_minor: parse_integer(&rest[3], "price")?,
                currency: rest[4].clone(),
                stock: parse_integer(&rest[5], "stock")?,
            }),
            ("P" | "V", found) => {
                return Err(WireError::new(format!(
                    "record `{record}` has {found} fields, expected {}",
                    if record == "P" { 3 } else { 6 }
                )));
            }
            (unknown, _) => {
                return Err(WireError::new(format!("unknown record `{unknown}`")));
            }
        }
    }

    Ok(Catalog::new(products, variants))
}

/// Wraps an encoded catalog in the worker message format.
#[must_use]
pub fn catalog_message(catalog: &Catalog) -> String {
    let mut message = String::from(CATALOG_PREFIX);
    message.push_str(&encode_catalog(catalog));
    message
}

/// Builds the worker message that reports a failed catalog load.
///
/// The text is the error's `Display` output, which is what the UI shows; it is
/// not machine-readable.
#[must_use]
pub fn error_message(message: &str) -> String {
    let mut out = String::from(ERROR_PREFIX);
    out.push_str(message);
    out
}

/// One message from the storage worker, whichever family it belongs to.
///
/// The startup message carries the catalog; every later message answers a
/// checkout. The shell matches on this instead of sniffing prefixes, so an
/// unknown or newer message is reported as a startup failure rather than
/// mistaken for a sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerMessage {
    /// The startup answer: a catalog, or why it could not be read.
    Catalog(Result<Catalog, String>),
    /// The answer to a checkout request.
    Checkout(CheckoutReply),
}

/// Decodes any message the storage worker can send.
pub fn decode_worker_message(text: &str) -> WorkerMessage {
    if text.starts_with("openrs-checkout-reply 1") {
        match decode_checkout_reply(text) {
            Ok(reply) => WorkerMessage::Checkout(reply),
            Err(error) => WorkerMessage::Checkout(CheckoutReply::Failed {
                checkout_id: String::new(),
                reason: error.to_string(),
            }),
        }
    } else {
        WorkerMessage::Catalog(decode_message(text))
    }
}

/// Decodes a worker message into a catalog or the problem that prevented one.
///
/// Returns `Err` for a worker-reported failure *and* for an unreadable message:
/// the UI has nothing to display in either case, only a reason.
pub fn decode_message(message: &str) -> Result<Catalog, String> {
    if let Some(body) = message.strip_prefix(CATALOG_PREFIX) {
        decode_catalog(body).map_err(|error| error.to_string())
    } else if let Some(reason) = message.strip_prefix(ERROR_PREFIX) {
        Err(reason.to_owned())
    } else {
        Err(String::from(
            "the storage worker sent a message this build does not understand",
        ))
    }
}

/// What the worker answered to a checkout request.#[derive(Debug, Clone, PartialEq)]
pub enum CheckoutReply {
    /// The sale is recorded. The catalog reflects the deducted stock.
    Recorded {
        /// Summary of the recorded sale, including its idempotency key.
        receipt: CheckoutReceipt,
        /// Catalog after the sale, with the new stock counts.
        catalog: Catalog,
    },
    /// Nothing was recorded; the UI keeps the bill and may retry with the same
    /// key.
    Failed {
        /// Key of the rejected attempt, echoed so a late reply for an older
        /// attempt can be ignored.
        checkout_id: String,
        /// Reason shown to the user.
        reason: String,
    },
}


    /// The sale is recorded. The catalog reflects the deducted stock.
    Recorded {
        /// Summary of the recorded sale, including its idempotency key.
        receipt: CheckoutReceipt,
        /// Catalog after the sale, with the new stock counts.
        catalog: Catalog,
    },
    /// Nothing was recorded; the UI keeps the bill and may retry with the same
    /// key.
    Failed {
        /// Key of the rejected attempt, echoed so a late reply for an older
        /// attempt can be ignored.
        checkout_id: String,
        /// Reason shown to the user.
        reason: String,
    },
}

/// Encodes a checkout request for `postMessage`.
///
/// Only the variant id and quantity are sent. The worker prices the lines from
/// the database, so a modified page cannot propose a price.
#[must_use]
pub fn checkout_request_message(request: &CheckoutRequest) -> String {
    let mut out = String::from(CHECKOUT_REQUEST_HEADER);
    out.push('\n');

    out.push('I');
    out.push('\t');
    escape_into(&mut out, &request.checkout_id);
    out.push('\n');

    for line in &request.lines {
        out.push('L');
        for field in [&line.variant_id, &line.quantity.to_string()] {
            out.push('\t');
            escape_into(&mut out, field);
        }
        out.push('\n');
    }

    out
}

/// Decodes a checkout request produced by [`checkout_request_message`].
///
/// Quantities are parsed as `u32` but not range-checked against stock here; the
/// storage layer owns that rule and reports it as a checkout failure.
pub fn decode_checkout_request(text: &str) -> Result<CheckoutRequest, WireError> {
    let mut lines = text.split('\n');

    match lines.next() {
        Some(header) if header == CHECKOUT_REQUEST_HEADER => {}
        Some(header) => {
            return Err(WireError::new(format!(
                "unsupported checkout header `{header}`"
            )));
        }
        None => return Err(WireError::new("empty checkout message")),
    }

    let mut checkout_id: Option<String> = None;
    let mut checkout_lines = Vec::new();

    for line in lines {
        if line.is_empty() {
            continue;
        }

        let mut fields = line.split('\t');
        let record = fields.next().unwrap_or_default();
        let rest = fields
            .map(unescape)
            .collect::<Result<Vec<String>, WireError>>()?;

        match (record, rest.len()) {
            ("I", 1) => checkout_id = Some(rest[0].clone()),
            ("L", 2) => checkout_lines.push(CheckoutLine {
                variant_id: rest[0].clone(),
                quantity: parse_quantity(&rest[1])?,
            }),
            ("I", found) => {
                return Err(WireError::new(format!(
                    "record `I` has {found} fields, expected 1"
                )));
            }
            ("L", found) => {
                return Err(WireError::new(format!(
                    "record `L` has {found} fields, expected 2"
                )));
            }
            (unknown, _) => {
                return Err(WireError::new(format!("unknown record `{unknown}`")));
            }
        }
    }

    let checkout_id = checkout_id.ok_or_else(|| WireError::new("checkout has no id line"))?;

    Ok(CheckoutRequest {
        checkout_id,
        lines: checkout_lines,
    })
}

/// Encodes a recorded sale plus the post-sale catalog.
#[must_use]
pub fn checkout_reply_message(_receipt: &CheckoutReceipt, _catalog: &Catalog) -> String {
    let _ = _receipt;
    let _ = _catalog;
    unimplemented!()
}

/// Encodes a reply for a recorded checkout.
#[must_use]
pub fn checkout_reply_message(receipt: &CheckoutReceipt, catalog: &Catalog) -> String {
    let mut out = String::from(CHECKOUT_REPLY_HEADER);
    out.push('\n');

    out.push('S');
    for field in [
        &receipt.sale_id,
        &receipt.checkout_id,
        &receipt.total_minor.to_string(),
        &receipt.currency,
    ] {
        out.push('\t');
        escape_into(&mut out, field);
    }
    out.push('\n');

    out.push_str(&encode_catalog(catalog));
    out
}

pub fn checkout_reply_message(receipt: &CheckoutReceipt, catalog: &Catalog) -> String {
    let mut out = String::from(CHECKOUT_REPLY_HEADER);
    out.push('\n');

    out.push('S');
    for field in [
        &receipt.sale_id,
        &receipt.checkout_id,
        &receipt.total_minor.to_string(),
        &receipt.currency,
    ] {
        out.push('\t');
        escape_into(&mut out, field);
    }
    out.push('\n');

    out.push_str(&encode_catalog(catalog));
    out
}



/// Encodes a rejected checkout, echoing the attempt's key.
#[must_use]
pub fn checkout_failure_message(checkout_id: &str, reason: &str) -> String {
    let mut out = String::from(CHECKOUT_REPLY_HEADER);
    out.push('\n');
    out.push('E');

    for field in [checkout_id, reason] {
        out.push('\t');
        escape_into(&mut out, field);
    }

    out.push('\n');
    out
}



/// Encodes a rejected checkout, echoing the attempt's key.
#[must_use]pub fn checkout_failure_message(checkout_id: &str, reason: &str) -> String {
    let mut out = String::from(CHECKOUT_REPLY_HEADER);
    out.push('\n');
    out.push('E');

    for field in [checkout_id, reason] {
        out.push('\t');
        escape_into(&mut out, field);
    }

    out.push('\n');
    out
}

/// Decodes a worker reply produced by [`checkout_reply_message`] or
/// [`checkout_failure_message`].
///
/// A recorded reply must carry a catalog; a failure must not. Partial messages
/// are rejected rather than guessed at.
pub fn decode_checkout_reply(text: &str) -> Result<CheckoutReply, WireError> {
    let mut lines = text.split('\n');

    match lines.next() {
        Some(header) if header == CHECKOUT_REPLY_HEADER => {}
        Some(header) => {
            return Err(WireError::new(format!(
                "unsupported checkout reply header `{header}`"
            )));
        }
        None => return Err(WireError::new("empty checkout reply")),
    }

    let record = lines
        .next()
        .ok_or_else(|| WireError::new("checkout reply has no record"))?;

    let mut fields = record.split('\t');
    let tag = fields.next().unwrap_or_default();
    let rest = fields
        .map(unescape)
        .collect::<Result<Vec<String>, WireError>>()?;

    match (tag, rest.len()) {
        ("S", 4) => {
            let mut receipt = CheckoutReceipt {
                sale_id: rest[0].clone(),
                checkout_id: rest[1].clone(),
                total_minor: parse_integer(&rest[2], "total")?,
                currency: rest[3].clone(),
            };
            receipt.sale_items = Vec::new();

            let body = lines.collect::<Vec<_>>().join("\n");
            if body.trim().is_empty() {
                return Err(WireError::new(
                    "a recorded checkout reply must include the catalog",
                ));
            }

            let catalog = decode_catalog(&body)?;
            Ok(CheckoutReply::Recorded { receipt, catalog })
        }
        ("E", 2) => Ok(CheckoutReply::Failed {
            checkout_id: rest[0].clone(),
            reason: rest[1].clone(),
        }),
        ("S", found) => Err(WireError::new(format!(
            "record `S` has {found} fields, expected 4"
        ))),
        ("E", found) => Err(WireError::new(format!(
            "record `E` has {found} fields, expected 2"
        ))),
        (unknown, _) => Err(WireError::new(format!("unknown record `{unknown}`"))),
    }
}

fn parse_integer(text: &str, label: &str) -> Result<i64, WireError> {
    text.parse::<i64>()
        .map_err(|_| WireError::new(format!("`{text}` is not a valid {label}")))
}

fn parse_quantity(text: &str) -> Result<u32, WireError> {
    text.parse::<u32>()
        .map_err(|_| WireError::new(format!("`{text}` is not a valid quantity")))
}

fn escape_into(out: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
}

fn unescape(value: &str) -> Result<String, WireError> {
    let mut out = String::with_capacity(value.len());
    let mut characters = value.chars();

    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }

        match characters.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => {
                return Err(WireError::new(format!("unknown escape `\\{other}`")));
            }
            None => return Err(WireError::new("field ends with a lone backslash")),
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        CATALOG_PREFIX, CheckoutReply, WorkerMessage, catalog_message, checkout_failure_message,
        checkout_reply_message, checkout_request_message, decode_catalog,
        decode_checkout_reply, decode_checkout_request, decode_message, decode_worker_message,
        encode_catalog, error_message,
    };
    use crate::catalog::{Catalog, Product, Variant};
    use crate::checkout::{CheckoutLine, CheckoutReceipt, CheckoutRequest};

    #[test]
    fn the_sample_catalog_round_trips_unchanged() {
        let catalog = Catalog::sample();
        let decoded = decode_catalog(&encode_catalog(&catalog)).expect("decode");
        assert_eq!(decoded, catalog);
    }

    #[test]
    fn an_empty_catalog_round_trips() {
        let catalog = Catalog::default();
        let decoded = decode_catalog(&encode_catalog(&catalog)).expect("decode");
        assert!(decoded.products().is_empty());
        assert!(decoded.variants().is_empty());
    }

    #[test]
    fn fields_with_separators_survive_the_round_trip() {
        let catalog = Catalog::new(
            vec![Product {
                id: "p\\1".to_owned(),
                name: "Tabbed\tName\nWith Line\rBreak".to_owned(),
                category: "Odd\tCategory".to_owned(),
            }],
            vec![Variant {
                id: "v\t1".to_owned(),
                product_id: "p\\1".to_owned(),
                sku: "SKU\n1".to_owned(),
                price_minor: -7,
                currency: "U\tSD".to_owned(),
                stock: 0,
            }],
        );

        let decoded = decode_catalog(&encode_catalog(&catalog)).expect("decode");
        assert_eq!(decoded, catalog);
    }

    #[test]
    fn the_encoded_body_is_one_record_per_line() {
        let body = encode_catalog(&Catalog::sample());
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines[0], "openrs-catalog 1");
        assert_eq!(lines.len(), 1 + 4 + 7);
        assert!(lines[1].starts_with("P\t"));
        assert!(lines[5].starts_with("V\t"));
    }

    #[test]
    fn a_wrong_header_is_rejected() {
        let error = decode_catalog("openrs-catalog 2\n").expect_err("must reject");
        assert!(error.to_string().contains("unsupported catalog header"));
    }

    #[test]
    fn malformed_records_are_rejected() {
        for body in [
            "openrs-catalog 1\nX\ta\n",
            "openrs-catalog 1\nP\tonly-two\n",
            "openrs-catalog 1\nV\ta\tb\tc\tnot-a-price\tUSD\t1\n",
            "openrs-catalog 1\nV\ta\tb\tc\t1\tUSD\tmany\n",
            "openrs-catalog 1\nP\ta\tbad\\qescape\tc\n",
            "openrs-catalog 1\nP\ta\tlone-backslash\\\n",
        ] {
            assert!(
                decode_catalog(body).is_err(),
                "`{body}` must not decode into a catalog"
            );
        }
    }

    #[test]
    fn a_checkout_request_round_trips() {
        let request = CheckoutRequest {
            checkout_id: "checkout\t1".to_owned(),
            lines: vec![
                CheckoutLine {
                    variant_id: "v-1".to_owned(),
                    quantity: 2,
                },
                CheckoutLine {
                    variant_id: "v\\2".to_owned(),
                    quantity: 1,
                },
            ],
        };

        let message = checkout_request_message(&request);
        assert!(message.starts_with("openrs-checkout 1\n"));
        assert_eq!(decode_checkout_request(&message).expect("decode"), request);
    }

    #[test]
    fn a_checkout_request_without_an_id_is_rejected() {
        let error = decode_checkout_request("openrs-checkout 1\nL\tv-1\t1\n")
            .expect_err("must require an id");
        assert!(error.to_string().contains("no id line"));
    }

    #[test]
    fn malformed_checkout_requests_are_rejected() {
        for body in [
            "openrs-checkout 2\nI\tc1\n",
            "openrs-checkout 1\nI\tc1\nL\tv-1\n",
            "openrs-checkout 1\nI\tc1\nL\tv-1\t-1\n",
            "openrs-checkout 1\nI\tc1\nL\tv-1\tnot-a-number\n",
            "openrs-checkout 1\nI\tc1\nX\tv-1\n",
            "openrs-checkout 1\nI\tc1\tc2\n",
        ] {
            assert!(
                decode_checkout_request(body).is_err(),
                "`{body}` must not decode into a checkout request"
            );
        }
    }

    #[test]
    fn a_recorded_reply_round_trips_with_its_catalog() {
        let receipt = CheckoutReceipt {
            sale_id: "sale-1".to_owned(),
            checkout_id: "checkout-1".to_owned(),
            total_minor: 4_497,
            currency: "USD".to_owned(),
        };
        let catalog = Catalog::sample();

        let message = checkout_reply_message(&receipt, &catalog);

        assert_eq!(
            decode_checkout_reply(&message).expect("decode"),
            CheckoutReply::Recorded { receipt, catalog }
        );
    }

    #[test]
    fn a_failed_reply_round_trips_with_its_reason() {
        let message = checkout_failure_message("checkout-1", "insufficient stock\nfor `v-1`");

        assert_eq!(
            decode_checkout_reply(&message).expect("decode"),
            CheckoutReply::Failed {
                checkout_id: "checkout-1".to_owned(),
                reason: "insufficient stock\nfor `v-1`".to_owned(),
            }
        );
    }

    #[test]
    fn malformed_checkout_replies_are_rejected() {
        for body in [
            "openrs-checkout-reply 2\nE\tc1\tboom\n",
            "openrs-checkout-reply 1\n",
            "openrs-checkout-reply 1\nS\tsale-1\tc1\t12\n",
            "openrs-checkout-reply 1\nS\tsale-1\tc1\tnot-a-total\tUSD\nopenrs-catalog 1\n",
            "openrs-checkout-reply 1\nS\tsale-1\tc1\t12\tUSD\n",
            "openrs-checkout-reply 1\nE\tc1\n",
            "openrs-checkout-reply 1\nX\tc1\tboom\n",
        ] {
            assert!(
                decode_checkout_reply(body).is_err(),
                "`{body}` must not decode into a checkout reply"
            );
        }
    }

    #[test]
    fn worker_messages_are_dispatched_by_family() {
        let catalog = Catalog::sample();

        assert_eq!(
            decode_worker_message(&catalog_message(&catalog)),
            WorkerMessage::Catalog(Ok(catalog))
        );
        assert_eq!(
            decode_worker_message(&error_message("database is locked")),
            WorkerMessage::Catalog(Err("database is locked".to_owned()))
        );

        let receipt = CheckoutReceipt {
            sale_id: "sale-1".to_owned(),
            checkout_id: "checkout-1".to_owned(),
            total_minor: 1_999,
            currency: "USD".to_owned(),
        };
        let recorded = checkout_reply_message(&receipt, &Catalog::sample());
        assert!(matches!(
            decode_worker_message(&recorded),
            WorkerMessage::Checkout(CheckoutReply::Recorded { .. })
        ));

        let failed = checkout_failure_message("checkout-1", "nope");
        assert_eq!(
            decode_worker_message(&failed),
            WorkerMessage::Checkout(CheckoutReply::Failed {
                checkout_id: "checkout-1".to_owned(),
                reason: "nope".to_owned(),
            })
        );

        assert_eq!(
            decode_worker_message("something else"),
            WorkerMessage::Catalog(Err(String::from(
                "the storage worker sent a message this build does not understand"
            )))
        );
    }

    #[test]
    fn messages_round_trip_through_the_worker_format() {
        let catalog = Catalog::sample();
        let message = catalog_message(&catalog);
        assert!(message.starts_with(CATALOG_PREFIX));
        assert_eq!(decode_message(&message).expect("decode"), catalog);

        assert_eq!(
            decode_message(&error_message("database is locked")),
            Err("database is locked".to_owned())
        );
        assert!(
            decode_message("something else").is_err(),
            "an unknown message must not be treated as a catalog"
        );
    }
}
