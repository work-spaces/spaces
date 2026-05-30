//! `log` builtin namespace for Rules-mode starlark modules.
//!
//! These functions write to the per-evaluation `logger::Logger` stored on
//! `EvalContext`. When no logger is available (e.g. LSP mode or scripts that
//! were not constructed with a console), the calls are no-ops so that rules
//! modules can be loaded without side effects.
use crate::builtins::eval_context::get_eval_context;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::none::NoneType;

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Log a trace-level message on the active console.
    ///
    /// ```python
    /// log.trace("starting checkout")
    /// ```
    fn trace(message: &str, eval: &mut Evaluator) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
        if let Some(ref logger) = ctx.logger {
            logger.trace(message);
        }
        Ok(NoneType)
    }

    /// Log a debug-level message on the active console.
    ///
    /// ```python
    /// log.debug("resolved version 1.2.3")
    /// ```
    fn debug(message: &str, eval: &mut Evaluator) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
        if let Some(ref logger) = ctx.logger {
            logger.debug(message);
        }
        Ok(NoneType)
    }

    /// Log an informational message on the active console.
    ///
    /// ```python
    /// log.info("workspace ready")
    /// ```
    fn info(message: &str, eval: &mut Evaluator) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
        if let Some(ref logger) = ctx.logger {
            logger.info(message);
        }
        Ok(NoneType)
    }

    /// Log a high-level user-facing message on the active console.
    ///
    /// ```python
    /// log.message("--Building--")
    /// ```
    fn message(message: &str, eval: &mut Evaluator) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
        if let Some(ref logger) = ctx.logger {
            logger.message(message);
        }
        Ok(NoneType)
    }

    /// Queue a deferred warning to be displayed at the end of the run.
    ///
    /// ```python
    /// log.warn("deprecated rule used")
    /// ```
    fn warn(message: &str, eval: &mut Evaluator) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
        if let Some(ref logger) = ctx.logger {
            logger.warning(message);
        }
        Ok(NoneType)
    }

    /// Log an error-level message on the active console.
    ///
    /// ```python
    /// log.error("something went wrong")
    /// ```
    fn error(message: &str, eval: &mut Evaluator) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
        if let Some(ref logger) = ctx.logger {
            logger.error(message);
        }
        Ok(NoneType)
    }
}
