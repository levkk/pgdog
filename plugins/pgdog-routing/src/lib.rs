//! Parse queries using pg_query and route all SELECT queries
//! to replicas. All other queries are routed to a primary.

use once_cell::sync::Lazy;
use pg_query::{parse, NodeEnum};
use pgdog_plugin::bindings::{Config, Input, Output};
use pgdog_plugin::Route;

use tracing::trace;
use tracing::{debug, level_filters::LevelFilter};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use std::io::IsTerminal;
use std::sync::atomic::{AtomicUsize, Ordering};

static SHARD_ROUND_ROBIN: Lazy<AtomicUsize> = Lazy::new(|| AtomicUsize::new(0));

pub mod comment;
pub mod order_by;
pub mod sharding_function;

#[no_mangle]
pub extern "C" fn pgdog_init() {
    let format = fmt::layer()
        .with_ansi(std::io::stderr().is_terminal())
        .with_file(false);

    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    tracing_subscriber::registry()
        .with(format)
        .with(filter)
        .init();

    // TODO: This is more for fun/demo, but in prod, we want
    // this logger to respect options passed to pgDog proper, e.g.
    // use JSON output.
    debug!("🐕 pgDog routing plugin v{}", env!("CARGO_PKG_VERSION"));
}

#[no_mangle]
pub extern "C" fn pgdog_route_query(input: Input) -> Output {
    if let Some(query) = input.query() {
        let route = match route_internal(query.query(), input.config) {
            Ok(route) => route,
            Err(_) => Route::unknown(),
        };
        Output::forward(route)
    } else {
        Output::skip()
    }
}

fn route_internal(query: &str, config: Config) -> Result<Route, pg_query::Error> {
    let ast = parse(query)?;
    let shards = config.shards;
    let databases = config.databases();

    // Shortcut for typical single shard replicas-only/primary-only deployments.
    if shards == 1 {
        let read_only = databases.iter().all(|d| d.replica());
        let write_only = databases.iter().all(|d| d.primary());
        if read_only {
            return Ok(Route::read(0));
        }
        if write_only {
            return Ok(Route::read(0));
        }
    }

    trace!("{:#?}", ast);
    let shard = comment::shard(query, shards as usize)?;

    // For cases like SELECT NOW(), or SELECT 1, etc.
    let tables = ast.tables();
    if tables.is_empty() && shard.is_none() {
        // Better than random for load distribution.
        let shard_counter = SHARD_ROUND_ROBIN.fetch_add(1, Ordering::Relaxed);
        return Ok(Route::read(shard_counter % shards as usize));
    }

    if let Some(query) = ast.protobuf.stmts.first() {
        if let Some(ref node) = query.stmt {
            match node.node {
                Some(NodeEnum::SelectStmt(ref stmt)) => {
                    let order_by = order_by::extract(stmt)?;
                    let mut route = if let Some(shard) = shard {
                        Route::read(shard)
                    } else {
                        Route::read_all()
                    };

                    if !order_by.is_empty() {
                        route.order_by(&order_by);
                    }

                    return Ok(route);
                }

                Some(_) => (),

                None => (),
            }
        }
    }

    Ok(if let Some(shard) = shard {
        Route::write(shard)
    } else {
        Route::write_all()
    })
}

#[no_mangle]
pub extern "C" fn pgdog_fini() {
    debug!(
        "🐕 pgDog routing plugin v{} shutting down",
        env!("CARGO_PKG_VERSION")
    );
}
