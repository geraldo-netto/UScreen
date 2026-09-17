//! Cheap GUI status only. Destructive actions still call full daemon discovery.
use super::{matches, processes, Process};
use std::path::Path;

trait Source {
    fn inspect(&self, pid: u32) -> Option<Process>;
    fn discover(&self) -> Vec<Process>;
}

struct NativeSource;
impl NativeSource {
    fn accepted(process: &Process) -> bool {
        process.pid != std::process::id() && matches(process, unsafe { libc::getuid() })
    }
}
impl Source for NativeSource {
    fn inspect(&self, pid: u32) -> Option<Process> {
        Process::read(pid).filter(Self::accepted)
    }
    fn discover(&self) -> Vec<Process> {
        let mut candidates = processes::same_user_processes_named("uscreen").unwrap_or_default();
        candidates.retain(Self::accepted);
        candidates.sort_by_key(|process| process.pid);
        candidates
    }
}

#[derive(Default)]
pub struct StatusProbe {
    cached: Option<Process>,
}

impl StatusProbe {
    pub fn poll(&mut self, pid_file: Option<&Path>) -> Option<u32> {
        let tracked = pid_file
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| text.trim().parse::<u32>().ok());
        self.poll_with(&NativeSource, tracked)
    }

    fn poll_with(&mut self, source: &impl Source, tracked: Option<u32>) -> Option<u32> {
        let cached_pid = self.cached.as_ref().map(|process| process.pid);
        if tracked != cached_pid {
            if let Some(process) = tracked.and_then(|pid| source.inspect(pid)) {
                self.cached = Some(process);
                return tracked;
            }
        }
        let valid = self
            .cached
            .take()
            .and_then(|old| source.inspect(old.pid).filter(|current| *current == old));
        self.cached = valid.or_else(|| source.discover().into_iter().next());
        self.cached.as_ref().map(|process| process.pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct Fake {
        processes: RefCell<Vec<Process>>,
        inspected: Cell<usize>,
        scanned: Cell<usize>,
    }
    impl Source for Fake {
        fn inspect(&self, pid: u32) -> Option<Process> {
            self.inspected.set(self.inspected.get() + 1);
            self.processes
                .borrow()
                .iter()
                .find(|p| p.pid == pid)
                .cloned()
        }
        fn discover(&self) -> Vec<Process> {
            self.scanned.set(self.scanned.get() + 1);
            self.processes.borrow().clone()
        }
    }
    fn process(pid: u32) -> Process {
        Process {
            pid,
            uid: 1000,
            start_ticks: 1,
            executable: "/bin/uscreen".into(),
            arguments: vec!["uscreen".into(), "start".into()],
            cwd: "/".into(),
        }
    }

    #[test]
    fn t409_status_revalidates_one_pid_without_repeated_inventory() {
        let source = Fake::default();
        source.processes.borrow_mut().push(process(42));
        let mut probe = StatusProbe::default();
        for _ in 0..30 {
            assert_eq!(probe.poll_with(&source, Some(42)), Some(42));
        }
        assert_eq!(source.inspected.get(), 30);
        assert_eq!(source.scanned.get(), 0);
    }

    #[test]
    fn t409_stale_pid_and_identity_changes_force_discovery() {
        let source = Fake::default();
        source.processes.borrow_mut().push(process(42));
        let mut probe = StatusProbe::default();
        assert_eq!(probe.poll_with(&source, Some(99)), Some(42));
        source.processes.borrow_mut()[0].start_ticks = 2;
        assert_eq!(probe.poll_with(&source, Some(42)), Some(42));
        assert_eq!(
            source.scanned.get(),
            2,
            "PID reuse must invalidate cached identity"
        );
        source.processes.borrow_mut()[0].executable = "/new/uscreen".into();
        assert_eq!(probe.poll_with(&source, Some(42)), Some(42));
        assert_eq!(
            source.scanned.get(),
            3,
            "exec/path changes must invalidate identity"
        );
        source.processes.borrow_mut().clear();
        assert_eq!(probe.poll_with(&source, Some(42)), None);
        assert_eq!(source.scanned.get(), 4);
    }

    #[test]
    fn t409_changed_pid_file_precedes_still_valid_cached_daemon() {
        let source = Fake::default();
        source
            .processes
            .borrow_mut()
            .extend([process(42), process(43)]);
        let mut probe = StatusProbe::default();
        assert_eq!(probe.poll_with(&source, Some(42)), Some(42));
        assert_eq!(probe.poll_with(&source, Some(43)), Some(43));
        assert_eq!(source.scanned.get(), 0);
    }
}
