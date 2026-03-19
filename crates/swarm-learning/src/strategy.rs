//! The core trait for learning strategies.

use async_trait::async_trait;

use swarm_core::error::SwarmResult;

use crate::event::LearningEvent;
use crate::output::{LearningOutput, LearningResult, LearningRuleId};
use crate::scope::LearningScope;

/// Context provided to a learning strategy during observation and application.
#[derive(Debug, Clone)]
pub struct LearningContext {
    /// The scope in which learning is occurring.
    pub scope: LearningScope,
    /// Whether human approval is required for this scope.
    pub require_approval: bool,
    /// The tenant this context belongs to (for isolation).
    pub tenant_id: Option<String>,
}

/// A learning strategy that observes events and produces learning outputs.
///
/// Implement this trait to create custom learning algorithms. Strategies are
/// registered with the learning subsystem and receive events relevant to
/// their scope.
#[async_trait]
pub trait LearningStrategy: Send + Sync {
    /// A unique identifier for this strategy.
    fn id(&self) -> LearningRuleId;

    /// Human-readable name for logging and audit.
    fn name(&self) -> &str;

    /// Observe a learning event and optionally produce outputs.
    ///
    /// The strategy may produce zero or more [`LearningOutput`] values
    /// from a single event.
    async fn observe(
        &self,
        event: &LearningEvent,
        ctx: &LearningContext,
    ) -> SwarmResult<Vec<LearningOutput>>;

    /// Apply a previously produced learning output.
    ///
    /// This is called only after the output has been approved (if approval
    /// is required). Returns a result describing whether application succeeded.
    async fn apply(
        &self,
        output: &LearningOutput,
        ctx: &LearningContext,
    ) -> SwarmResult<LearningResult>;

    /// Whether this strategy's outputs always require human approval,
    /// regardless of the scope configuration.
    fn always_requires_approval(&self) -> bool {
        false
    }
}
