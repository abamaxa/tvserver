use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

//const FILTER: &str = "tvserver=debug,tower_http=debug";
pub const TVSERVER_LOG: &str = "tvserver=info,tower_http=debug";
pub const DBTOOL_LOG: &str = "dbtool=info,tvserver=info";


pub fn setup_logging(filter: &str) {
    let format = fmt::format()
        .with_ansi(false)
        .without_time()
        .with_level(true)
        .with_target(false)
        .compact();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| filter.into()),
        )
        .with(tracing_subscriber::fmt::layer().event_format(format))
        .init();
}
