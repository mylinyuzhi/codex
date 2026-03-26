//! Domain-based network filtering for sandbox proxies.

/// Domain filter with allow/deny lists and wildcard support.
///
/// Deny list always takes precedence. An empty allow list permits all
/// domains not in the deny list. Wildcard entries like `*.example.com`
/// match any subdomain of `example.com`.
pub struct DomainFilter {
    allowed_domains: Vec<String>,
    denied_domains: Vec<String>,
    /// Unix socket paths allowed through the sandbox.
    allowed_unix_sockets: Vec<std::path::PathBuf>,
    /// Allow all Unix sockets without filtering.
    allow_all_unix_sockets: bool,
    /// Allow binding to localhost ports.
    allow_local_binding: bool,
}

impl DomainFilter {
    /// Create a new domain filter.
    ///
    /// Domains are normalized to lowercase. Wildcard prefixes (`*.`)
    /// match any subdomain of the base domain.
    pub fn new(allowed: Vec<String>, denied: Vec<String>) -> Self {
        Self {
            allowed_domains: allowed.into_iter().map(|d| d.to_lowercase()).collect(),
            denied_domains: denied.into_iter().map(|d| d.to_lowercase()).collect(),
            allowed_unix_sockets: Vec::new(),
            allow_all_unix_sockets: false,
            allow_local_binding: false,
        }
    }

    /// Create a domain filter from the full `NetworkConfig`.
    pub fn from_network_config(config: &crate::config::NetworkConfig) -> Self {
        Self {
            allowed_domains: config
                .allowed_domains
                .iter()
                .map(|d| d.to_lowercase())
                .collect(),
            denied_domains: config
                .denied_domains
                .iter()
                .map(|d| d.to_lowercase())
                .collect(),
            allowed_unix_sockets: config.allow_unix_sockets.clone(),
            allow_all_unix_sockets: config.allow_all_unix_sockets,
            allow_local_binding: config.allow_local_binding,
        }
    }

    /// Check if a Unix socket path is allowed.
    pub fn is_unix_socket_allowed(&self, path: &std::path::Path) -> bool {
        if self.allow_all_unix_sockets {
            return true;
        }
        self.allowed_unix_sockets
            .iter()
            .any(|allowed| path.starts_with(allowed) || path == allowed)
    }

    /// Whether local port binding is allowed.
    pub fn allow_local_binding(&self) -> bool {
        self.allow_local_binding
    }

    /// Check if a domain is allowed through the proxy.
    ///
    /// Evaluation order:
    /// 1. Deny list match -> deny (highest priority)
    /// 2. Allow list empty -> allow (open policy)
    /// 3. Allow list match -> allow
    /// 4. Otherwise -> deny
    pub fn is_allowed(&self, domain: &str) -> bool {
        let domain = domain.to_lowercase();

        if self.matches_list(&domain, &self.denied_domains) {
            return false;
        }

        if self.allowed_domains.is_empty() {
            return true;
        }

        self.matches_list(&domain, &self.allowed_domains)
    }

    /// Check if a domain matches any entry in the given list.
    ///
    /// Supports exact match and wildcard suffix matching (`*.example.com`).
    fn matches_list(&self, domain: &str, list: &[String]) -> bool {
        list.iter().any(|pattern| {
            if let Some(suffix) = pattern.strip_prefix("*.") {
                // Wildcard: *.example.com matches sub.example.com
                // but not example.com itself.
                domain.ends_with(&format!(".{suffix}"))
            } else {
                domain == pattern
            }
        })
    }
}

#[cfg(test)]
#[path = "filter.test.rs"]
mod tests;
