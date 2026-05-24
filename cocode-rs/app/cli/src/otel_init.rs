//! OTel provider initialization from Config.

use cocode_config::Config;
use cocode_otel::otel_provider::OtelProvider;

/// Build OtelProvider from Config. Returns None if OTel is disabled or not configured.
pub fn build_provider(config: &Config) -> Option<OtelProvider> {
    let settings = config.otel.as_ref()?;
    match OtelProvider::from(settings) {
        Ok(Some(p)) => Some(p),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!("OTel init failed: {e}");
            None
        }
    }
}
