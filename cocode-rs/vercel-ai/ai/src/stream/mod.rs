//! Stream processing for vercel-ai streaming responses.
//!
//! Provides [`StreamProcessor`] for consuming and accumulating streaming
//! responses from language models. This is the mid-level API between raw
//! `LanguageModelV4StreamPart` events and the high-level `stream_text()`.
//!
//! # API Levels
//!
//! ```text
//! Level 1: Raw Stream (do_stream() → Stream<StreamPart>)   — too low for most uses
//! Level 2: StreamProcessor (accumulate → snapshot + events) — this module
//! Level 3: stream_text() (multi-step + tool exec)           — too high for agent loops
//! ```
//!
//! # Example
//!
//! ```ignore
//! use vercel_ai::stream::StreamProcessor;
//!
//! let result = model.do_stream(options).await?;
//! let mut processor = StreamProcessor::new(result);
//!
//! while let Some(Ok((part, snapshot))) = processor.next().await {
//!     println!("Text so far: {}", snapshot.text);
//!     if snapshot.is_complete {
//!         println!("Done! Usage: {:?}", snapshot.usage);
//!     }
//! }
//! ```

mod processor;
mod processor_state;
mod snapshot;

pub use processor::StreamProcessor;
pub use processor::StreamProcessorConfig;
pub use snapshot::FileSnapshot;
pub use snapshot::ReasoningSnapshot;
pub use snapshot::SourceSnapshot;
pub use snapshot::StreamSnapshot;
pub use snapshot::ToolCallSnapshot;
