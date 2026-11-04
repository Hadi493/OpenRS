//! Native startup: database location, system services, the catalog load, and
//! the checkout call.
//!
//! The desktop app opens the SQLite file directly, seeds it on first run, and
//! hands the loaded catalog to the shared UI. The connection stays open for the
//! session so `Complete sale` can record a sale without reopening the database.
//! Everything here is native-only; the web shell reaches the same storage layer
//! through its worker instead.
//!
//! # Where the database lives
//!
//! `OPENRS_DATABASE` overrides the location, which tests and developers use.
//! Otherwise the file is `openrs.sqlite` in the platform's per-user data
//! directory:
//!
//! | Platform | Directory |
//! |---|---|
//! | Linux / other Unix | `$XDG_DATA_HOME/openrs`, else `~/.local/share/openrs` |
//! | macOS | `~/Library/Application Support/OpenRS` |
//! | Windows | `%APPDATA%\OpenRS` |
//!
//! # Why one long-lived connection
//!
//! [`open_repository`] is called once at startup and the repository lives in the
//! application struct. Recording a sale is then a single call from the same
//! frame, which keeps checkout synchronous on the desktop. The web shell needs
//! the opposite shape (a worker round trip), so the shared UI hands out a
//! [`openrs_core::CheckoutRequest`] instead of calling storage itself; see
//! `openrs_ui`.
//!
//! # Failure rules
//!
//! A rejected checkout leaves the bill on screen and records nothing - storage
//! guarantees that with one transaction. A sale that *was* recorded must never
//! be hidden by a later failure, so a failed catalog reload is reported next to
//! the receipt rather than replacing the screen with the startup error.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use openrs_storage::{
    Catalog, CheckoutOutcome, CheckoutReceipt, Clock, IdSource, RepoError, Repository,
    SqliteRepository,
};
use openrs_ui::OpenRsApp;

/// Environment variable that overrides the database path.
pub const DATABASE_PATH_ENV: &str = "OPENRS_DATABASE";

/// Database file name inside the per-user data directory.
const DATABASE_FILE: &str = "openrs.sqlite";

/// Returns the database path, creating its parent directory if needed.
///
/// Fails only when no home or data directory can be determined, or when the
/// directory cannot be created.
pub fn prepare_database_path() -> Result<PathBuf, RepoError> {
    let path = match std::env::var_os(DATABASE_PATH_ENV) {
        Some(override_path) => PathBuf::from(override_path),
        None => data_directory(
            std::env::var_os("XDG_DATA_HOME"),
            std::env::var_os("APPDATA"),
            std::env::var_os("HOME"),
        )
        .ok_or_else(|| {
            RepoError::Storage(format!(
                "no data directory found; set {DATABASE_PATH_ENV} to a writable path"
            ))
        })?
        .join(DATABASE_FILE),
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            RepoError::Storage(format!("cannot create {}: {error}", parent.display()))
        })?;
    }

    Ok(path)
}

/// Resolves the per-user data directory from the environment values that the
/// platform uses.
///
/// Pure on purpose: the environment is read by the caller so this can be tested
/// without mutating process-global state.
fn data_directory(
    xdg_data_home: Option<OsString>,
    app_data: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        return app_data.map(|dir| PathBuf::from(dir).join("OpenRS"));
    }

    if cfg!(target_os = "macos") {
        return home.map(|dir| {
            PathBuf::from(dir)
                .join("Library")
                .join("Application Support")
                .join("OpenRS")
        });
    }

    // `XDG_DATA_HOME` must be an absolute path per the specification; a
    // relative value is ignored rather than silently written somewhere odd.
    if let Some(dir) = xdg_data_home.map(PathBuf::from)
        && dir.is_absolute()
    {
        return Some(dir.join("openrs"));
    }

    home.map(|dir| {
        PathBuf::from(dir)
            .join(".local")
            .join("share")
            .join("openrs")
    })
}

/// Opens the database at `path` and keeps it open for the session.
///
/// This is the only place the native shell creates a repository, so the clock
/// and id source are the same instances for every sale in the process.
pub fn open_repository(path: &Path) -> Result<SqliteRepository, RepoError> {
    SqliteRepository::open(path, Arc::new(SystemClock), Arc::new(ProcessIds::new()))
}

/// Seeds `repository` when it is empty and returns the catalog it holds.
///
/// The catalog is the same development sample on every first run; after that the
/// database is the only source of truth, so a restart shows exactly what was
/// stored.
pub fn load_catalog(repository: &mut SqliteRepository) -> Result<Catalog, RepoError> {
    let (outcome, catalog) =
        openrs_storage::load_or_seed_catalog(repository, &Catalog::sample())?;

    log::info!(
        "catalog ready: {} products, {} variants ({outcome:?})",
        catalog.products().len(),
        catalog.variants().len()
    );

    Ok(catalog)
}

/// Records the sale the UI is waiting on, if any.
///
/// Returns whether a request was taken, which the caller uses to decide whether
/// another frame is needed. The shared UI owns the attempt and its idempotency
/// key; this function only performs the storage call and reports the outcome:
///
/// 1. no pending request - nothing happens;
/// 2. recorded - the receipt is applied and the catalog is reloaded, because
///    stock moved;
/// 3. rejected - the bill and the key are kept so the user can retry, and
///    storage guarantees nothing was written.
///
/// A reload failure after a successful sale is shown in the bill panel instead
/// of the error screen: the sale exists, and hiding that would invite a second
/// sale of the same goods.
pub fn complete_sale(repository: &mut SqliteRepository, app: &mut OpenRsApp) -> bool {
    let Some(request) = app.take_checkout_request() else {
        return false;
    };

    let sale = match repository.checkout(&request) {
        Ok(CheckoutOutcome::Completed(sale) | CheckoutOutcome::Replayed(sale)) => sale,
        Err(error) => {
            log::warn!("checkout {} rejected: {error}", request.checkout_id);
            app.checkout_failed(&request.checkout_id, error.to_string());
            return true;
        }
    };

    log::info!(
        "sale {} recorded for checkout {}: {} {}",
        sale.id,
        sale.checkout_id,
        sale.total_minor,
        sale.currency
    );

    app.checkout_recorded(CheckoutReceipt {
        sale_id: sale.id,
        checkout_id: sale.checkout_id,
        total_minor: sale.total_minor,
        currency: sale.currency,
    });

    match repository.load_catalog() {
        Ok(catalog) => app.set_catalog(catalog),
        Err(error) => {
            log::error!("catalog reload after the sale failed: {error}");
            app.show_bill_message(format!(
                "The sale was recorded, but the catalog could not be refreshed: {error}"
            ));
        }
    }

    true
}

/// Wall clock based on `SystemTime`.
///
/// Timestamps are RFC 3339 in UTC so stored sales are readable without a
/// timezone database. Sub-second precision is dropped: a retail sale needs
/// second resolution, not nanoseconds.
struct SystemClock;

impl Clock for SystemClock {
    fn now_rfc3339(&self) -> String {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
            .unwrap_or_default();
        rfc3339_from_unix_seconds(seconds)
    }
}

/// Sale id source that does not need a random generator.
///
/// Uniqueness comes from the process id, the current millisecond, and a counter
/// that is shared by every thread of this process. Two sales recorded in the same
/// millisecond still differ, and a restart cannot repeat an id recorded earlier
/// unless the clock moves backwards. ADR 0002 plans UUIDv7; this is the smallest
/// correct-enough source until then.
struct ProcessIds(AtomicU64);

impl ProcessIds {
    fn new() -> Self {
        Self(AtomicU64::new(0))
    }
}

impl IdSource for ProcessIds {
    fn next_sale_id(&self) -> String {
        let sequence = self.0.fetch_add(1, Ordering::Relaxed);
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or_default();
        format!("sale-{}-{millis}-{sequence}", std::process::id())
    }
}

/// Formats Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Uses Howard Hinnant's `civil_from_days` algorithm, which is exact for the
/// whole `i64` range of days and needs no dependency.
fn rfc3339_from_unix_seconds(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);

    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_position + 2) / 5 + 1;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    };

    if month <= 2 {
        year += 1;
    }

    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::{SystemClock, ProcessIds, civil_from_days, complete_sale, data_directory,
        rfc3339_from_unix_seconds};
    use openrs_storage::{Catalog, Product, Repository, SqliteRepository, Variant};
    use openrs_ui::OpenRsApp;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn text(value: &str) -> Option<OsString> {
        Some(OsString::from(value))
    }

    #[test]
    fn unix_epoch_formats_as_rfc3339() {
        assert_eq!(rfc3339_from_unix_seconds(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            rfc3339_from_unix_seconds(1_700_000_000),
            "2023-11-14T22:13:20Z"
        );
        assert_eq!(
            rfc3339_from_unix_seconds(951_782_400),
            "2000-02-29T00:00:00Z",
            "leap days must be right"
        );
    }

    #[test]
    fn dates_before_the_epoch_format_correctly() {
        assert_eq!(rfc3339_from_unix_seconds(-1), "1969-12-31T23:59:59Z");
        assert_eq!(
            rfc3339_from_unix_seconds(-2_208_988_800),
            "1900-01-01T00:00:00Z"
        );
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_694), (2026, 8, 29));
    }

    #[test]
    fn the_data_directory_uses_xdg_on_linux() {
        if cfg!(target_os = "windows") || cfg!(target_os = "macos") {
            return;
        }

        assert_eq!(
            data_directory(text("/tmp/xdg"), None, text("/home/user")),
            Some(PathBuf::from("/tmp/xdg/openrs"))
        );
        assert_eq!(
            data_directory(text("relative/dir"), None, text("/home/user")),
            Some(PathBuf::from("/home/user/.local/share/openrs")),
            "a relative XDG_DATA_HOME is ignored"
        );
        assert_eq!(
            data_directory(None, None, text("/home/user")),
            Some(PathBuf::from("/home/user/.local/share/openrs"))
        );
    }

    #[test]
    fn a_missing_home_directory_is_reported_not_guessed() {
        assert_eq!(data_directory(None, None, None), None);
    }

    fn in_memory_repository() -> SqliteRepository {
        SqliteRepository::open_in_memory(Arc::new(SystemClock), Arc::new(ProcessIds::new()))
            .expect("open in-memory database")
    }

    /// One product and one variant, in the shape the UI and storage share.
    fn catalog_with_stock(stock: i64) -> Catalog {
        Catalog::new(
            vec![Product {
                id: "p-1".to_owned(),
                name: "Test Shirt".to_owned(),
                category: "Apparel".to_owned(),
            }],
            vec![Variant {
                id: "v-1".to_owned(),
                product_id: "p-1".to_owned(),
                sku: "SKU-1".to_owned(),
                price_minor: 1_000,
                currency: "USD".to_owned(),
                stock,
            }],
        )
    }

    #[test]
    fn a_completed_sale_is_recorded_once_and_refreshes_the_catalog() {
        let mut repository = in_memory_repository();
        repository
            .seed_catalog_if_empty(&Catalog::sample())
            .expect("seed");

        let mut app = OpenRsApp::with_catalog(Catalog::sample());
        app.add_to_bill("v-tee-s").expect("add");
        app.add_to_bill("v-tee-s").expect("add again");
        assert!(app.request_checkout());

        assert!(complete_sale(&mut repository, &mut app));

        assert_eq!(repository.sale_count().expect("count"), 1);
        assert_eq!(
            repository.stock_of("v-tee-s").expect("stock"),
            10,
            "two units leave stock"
        );
        assert!(app.bill().is_empty(), "the bill is consumed by the sale");
        assert_eq!(
            app.receipt().expect("receipt").total_minor,
            2_998,
            "the receipt shows the recorded total"
        );
        assert!(
            !complete_sale(&mut repository, &mut app),
            "without a new request nothing may be recorded"
        );
        assert_eq!(repository.sale_count().expect("count"), 1);
    }

    #[test]
    fn a_rejected_sale_keeps_the_bill_and_a_retry_records_once() {
        let mut repository = in_memory_repository();
        repository
            .seed_catalog_if_empty(&catalog_with_stock(1))
            .expect("seed");

        // The snapshot on screen is older than the database, so the UI accepts
        // a quantity that storage refuses - the race this test exercises.
        let mut app = OpenRsApp::with_catalog(catalog_with_stock(5));
        app.add_to_bill("v-1").expect("add");
        app.set_bill_quantity("v-1", 3).expect("set quantity");

        assert!(app.request_checkout());
        assert!(complete_sale(&mut repository, &mut app));

        assert_eq!(repository.sale_count().expect("count"), 0, "nothing written");
        assert_eq!(repository.stock_of("v-1").expect("stock"), 1);
        assert_eq!(app.bill().len(), 1, "the bill survives the rejection");
        assert!(app.receipt().is_none());

        app.set_bill_quantity("v-1", 1).expect("reduce quantity");
        assert!(app.request_checkout(), "the user can retry");
        assert!(complete_sale(&mut repository, &mut app));

        assert_eq!(repository.sale_count().expect("count"), 1);
        assert_eq!(repository.stock_of("v-1").expect("stock"), 0);
        assert!(app.receipt().is_some());
        assert!(app.bill().is_empty());
    }
}
