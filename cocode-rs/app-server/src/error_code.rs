//! Standard JSON-RPC error codes for the app-server protocol.

/// Server is overloaded; client should retry with exponential backoff.
pub const OVERLOADED_ERROR_CODE: i64 = -32001;

/// Invalid parameters in a request.
pub const INVALID_PARAMS_ERROR_CODE: i64 = -32602;

/// Server has not been initialized (no `initialize` request received).
pub const NOT_INITIALIZED_ERROR_CODE: i64 = -32002;

/// Server has already been initialized (duplicate `initialize` request).
pub const ALREADY_INITIALIZED_ERROR_CODE: i64 = -32003;
