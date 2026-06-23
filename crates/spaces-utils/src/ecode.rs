pub struct Ecode {
    serial_number: u32,
    message: &'static str,
}

impl Ecode {
    const fn new(serial_number: u32, message: &'static str) -> Self {
        Self {
            serial_number,
            message,
        }
    }
}

pub fn anyhow(serial_number: u32, context: &str) -> anyhow::Error {
    let ecode = ECODES[serial_number as usize];
    assert!(serial_number == ecode.serial_number);
    let mut result = format!("ecode[{:04}]: {}\n", serial_number, ecode.message);
    if !context.is_empty() {
        for line in context.lines() {
            result.push_str("  ");
            result.push_str(line);
            result.push('\n');
        }
    }
    anyhow::anyhow!(result)
}

pub fn anyhow_trace(serial_number: u32) -> anyhow::Error {
    anyhow(serial_number, "")
}

// Codes
pub static ECODES: &[&Ecode] = &[
    &Ecode::new(0, "None"),
    &Ecode::new(1, "dependency graph contains a circular dependency"),
    &Ecode::new(2, "failed to evaluate module"),
    &Ecode::new(3, "failed to evaluate module during checkout"),
    &Ecode::new(4, "failed to load internal module using //"),
    &Ecode::new(5, "target file is claimed by multiple rules"),
    &Ecode::new(6, "target dir is claimed by multiple rules"),
    &Ecode::new(7, "target artifact is contained in target dir"),
    &Ecode::new(8, "git command failed"),
    &Ecode::new(9, "git command failed with retries"),
    &Ecode::new(10, "git command failed with retries (unknown cause)"),
    &Ecode::new(11, "<trace> runner sync failed"),
];
