use crate::executor;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Condvar, Mutex};
use utils::rule;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ValueEnum, strum::Display)]
pub enum Phase {
    Checkout,
    Run,
    Inspect,
    Complete,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Signal {
    pub ready: bool,
    pub name: Arc<str>,
}

#[derive(Default, Debug, Clone)]
pub struct SignalArc {
    pub signal: Arc<(Mutex<Signal>, Condvar)>,
}

impl SignalArc {
    pub fn new(name: Arc<str>) -> Self {
        SignalArc {
            signal: Arc::new((Mutex::new(Signal { ready: false, name }), Condvar::new())),
        }
    }

    pub fn wait_is_ready(&self, duration: std::time::Duration) {
        loop {
            let (lock, cvar) = &*self.signal;
            let signal_access = lock.lock().unwrap();
            if !signal_access.ready {
                let _ = cvar.wait_timeout(signal_access, duration).unwrap();
            } else {
                break;
            }
        }
    }

    pub fn set_ready_notify_all(&self) {
        let (lock, cvar) = &*self.signal;
        {
            let mut signal_access = lock.lock().unwrap();
            signal_access.ready = true;
        }
        cvar.notify_all();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// The tasks executor
    pub executor: executor::Task,
    pub phase: Phase,
    /// the rule
    pub rule: rule::Rule,
    /// digest of the task and it's dependencies digests also
    pub digest: Arc<str>,
    /// signal to notify dependents that the task is complete
    #[serde(skip)]
    pub signal: SignalArc,
}

impl Task {
    pub fn new(rule: rule::Rule, phase: Phase, executor: executor::Task) -> Self {
        Task {
            executor,
            phase,
            signal: SignalArc::new(rule.name.clone()),
            rule,
            digest: "".into(),
        }
    }

    pub fn calculate_digest(&self) -> blake3::Hash {
        let mut self_clone = self.clone();
        self_clone.digest = "".into();
        let seed = serde_json::to_string(&self_clone).unwrap();
        let mut digest = blake3::Hasher::new();
        digest.update(seed.as_bytes());
        digest.finalize()
    }

    pub fn _update_implicit_dependency(&mut self, other_task: &Task) {
        if let Some(deps) = &self.rule.deps
            && deps.contains(&other_task.rule.name)
        {
            return;
        }

        if let Some(rule::InputsOutputs::Globs(inputs)) = &self.rule.inputs {
            for input in inputs {
                if let Some(rule::InputsOutputs::Globs(other_outputs)) = &other_task.rule.outputs
                    && other_outputs.contains(input)
                {
                    if let Some(deps) = self.rule.deps.as_mut() {
                        deps.push(other_task.rule.name.clone());
                    } else {
                        self.rule.deps = Some(vec![other_task.rule.name.clone()]);
                    }
                    return;
                }
            }
        }
    }
}
