//! Linux process counters only; absence remains unknown to selection policy.
use super::{Counter, Sampler};
pub(crate) struct Process;
impl Sampler for Process {
    fn sample(&self, pid: u32) -> Option<Counter> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        parse(&text, ticks.try_into().ok()?, page.try_into().ok()?)
    }
}
fn parse(text: &str, ticks: u64, page: u64) -> Option<Counter> {
    let fields: Vec<_> = text.rsplit_once(") ")?.1.split_whitespace().collect();
    let value = |index: usize| fields.get(index)?.parse::<u64>().ok();
    let cpu = value(11)?.checked_add(value(12)?)?;
    Some(Counter {
        identity: value(19)?,
        cpu_us: cpu.checked_mul(1_000_000)?.checked_div(ticks)?,
        rss_bytes: value(21)?.checked_mul(page)?,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t612_linux_counters_keep_process_identity_and_reject_invalid_bounds() {
        assert!(Process.sample(std::process::id()).unwrap().rss_bytes > 0);
        assert!(Process.sample(0).is_none());
        let valid = "1 (name with spaces) S 0 0 0 0 0 0 0 0 0 0 2 3 0 0 0 0 0 0 7 0 10";
        let counter = parse(valid, 100, 4096).unwrap();
        assert_eq!((counter.identity, counter.cpu_us, counter.rss_bytes), (7, 50_000, 40960));
        assert!(parse(valid, 0, 4096).is_none());
        assert!(parse(valid, 100, u64::MAX).is_none());
        for end in 0..valid.len() { let _ = parse(&valid[..end], 100, 4096); }
        for byte in 0..=127 { assert!(parse(&char::from(byte).to_string().repeat(128), 100, 4096).is_none()); }
    }
}
