//! T405: real private FIFOs, no encoder, display, or input device.
use super::*;
use crate::encoder_io::read_frame;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

static T497_INTERRUPTS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn t497_interrupt(_: libc::c_int) {
    T497_INTERRUPTS.fetch_add(1, Ordering::SeqCst);
}

fn t497_wait_for_poll(tid: libc::pid_t) {
    let path = format!("/proc/self/task/{tid}/syscall");
    let expected = format!("{} ", libc::SYS_poll);
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !std::fs::read_to_string(&path)
        .unwrap()
        .starts_with(&expected)
    {
        assert!(
            std::time::Instant::now() < deadline,
            "T497 thread never entered poll"
        );
        std::thread::yield_now();
    }
}

fn t497_poll_interruption() {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = t497_interrupt as *const () as usize;
    assert_eq!(unsafe { libc::sigemptyset(&mut action.sa_mask) }, 0);
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGUSR1, &action, &mut previous) },
        0
    );
    let stop = StopSignal::new().unwrap();
    let request = stop.clone();
    let target = unsafe { libc::pthread_self() };
    let tid = unsafe { libc::syscall(libc::SYS_gettid) } as libc::pid_t;
    let worker = std::thread::spawn(move || {
        t497_wait_for_poll(tid);
        assert_eq!(unsafe { libc::pthread_kill(target, libc::SIGUSR1) }, 0);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while T497_INTERRUPTS.load(Ordering::SeqCst) == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "T497 signal was not delivered"
            );
            std::thread::yield_now();
        }
        request.request();
    });
    let result = poll_ready(&mut [poll_descriptor(stop.event.as_raw_fd())], 3000, &stop);
    worker.join().unwrap();
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGUSR1, &previous, std::ptr::null_mut()) },
        0
    );
    assert!(
        !result.unwrap(),
        "T497 interrupted poll ignored latched cancellation"
    );
    assert_eq!(T497_INTERRUPTS.load(Ordering::SeqCst), 1);
}

fn t497_poll_rejects_excess_descriptors() {
    let stop = StopSignal::new().unwrap();
    let mut previous: libc::rlimit = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut previous) },
        0
    );
    let bounded = libc::rlimit {
        rlim_cur: 0,
        rlim_max: previous.rlim_max,
    };
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &bounded) }, 0);
    let error = poll_ready(&mut [poll_descriptor(stop.event.as_raw_fd())], 0, &stop);
    assert_eq!(
        unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &previous) },
        0
    );
    assert_eq!(error.unwrap_err().raw_os_error(), Some(libc::EINVAL));
}

#[test]
fn t497_poll_retries_interrupted_waits_and_preserves_kernel_errors() {
    if std::env::var_os("BLENT_T497_POLL_CHILD").is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "encoder_io::fifo::tests::t497_poll_retries_interrupted_waits_and_preserves_kernel_errors", "--nocapture"])
            .env("BLENT_T497_POLL_CHILD", "1")
            .output().unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    t497_poll_interruption();
    t497_poll_rejects_excess_descriptors();
}

#[test]
fn t497_cancellation_survives_a_full_event_counter_and_missing_fifo_watch() {
    struct Missing;
    impl Read for Missing {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }
    }
    impl AsRawFd for Missing {
        fn as_raw_fd(&self) -> RawFd {
            -1
        }
    }
    let stop = StopSignal::new().unwrap();
    let full = u64::MAX - 1;
    assert_eq!(
        unsafe { libc::write(stop.event.as_raw_fd(), (&full as *const u64).cast(), 8) },
        8
    );
    stop.request(); // EAGAIN still leaves cancellation latched.
    stop.request();
    assert!(stop.requested());
    let mut source = FifoReader::new(Missing);
    assert!(source.events.is_none());
    assert!(!source.wait(Waiting::Writer, &stop).unwrap());
    assert!(!source.wait(Waiting::Data, &stop).unwrap());
}

#[test]
fn t497_retired_event_sources_are_not_treated_as_live_fifo_notifications() {
    let mut empty = tempfile::tempfile().unwrap();
    assert!(!drain_events(&mut empty).unwrap());
    let path = tempfile::NamedTempFile::new().unwrap();
    let mut write_only = std::fs::OpenOptions::new()
        .write(true)
        .open(path.path())
        .unwrap();
    assert_eq!(
        drain_events(&mut write_only).unwrap_err().raw_os_error(),
        Some(libc::EBADF)
    );
}

struct Counted {
    file: std::fs::File,
    reads: Arc<AtomicUsize>,
    started: Option<mpsc::Sender<()>>,
}
impl Read for Counted {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let result = self.file.read(bytes);
        if let Some(started) = self.started.take() {
            started.send(()).unwrap();
        }
        result
    }
}
impl AsRawFd for Counted {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

fn create_fifo(path: &std::path::Path) -> std::fs::File {
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .unwrap()
}

fn idle_fifo(writer_attached: bool) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("capture.fifo");
    let file = create_fifo(&path);
    let _writer = writer_attached.then(|| {
        std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
            .unwrap()
    });
    let stop = StopSignal::new().unwrap();
    let stopped = stop.clone();
    let reads = Arc::new(AtomicUsize::new(0));
    let (started, ready) = mpsc::channel();
    let source = Counted {
        file,
        reads: reads.clone(),
        started: Some(started),
    };
    let mut source = FifoReader::new(source);
    let (done, finished) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        done.send(read_frame(&mut source, &mut [0; 16], &stopped))
            .unwrap();
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    std::thread::sleep(Duration::from_millis(120));
    stop.request();
    assert!(!finished
        .recv_timeout(Duration::from_secs(2))
        .expect("T405: idle reader ignored cancellation")
        .unwrap());
    worker.join().unwrap();
    assert!(
        reads.load(Ordering::SeqCst) <= 2,
        "T405: idle FIFO repeatedly read {} times",
        reads.load(Ordering::SeqCst)
    );
}

#[test]
fn t405_no_writer_fifo_waits_for_an_event_instead_of_repeated_eof_reads() {
    idle_fifo(false);
}

#[test]
fn t405_empty_fifo_waits_for_readiness_instead_of_repeated_eagain_reads() {
    idle_fifo(true);
}

#[test]
fn t405_writer_notifications_follow_opened_inode_after_path_replacement() {
    use std::io::Write;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("capture");
    let original = directory.path().join("original");
    let file = create_fifo(&path);
    std::fs::rename(&path, &original).unwrap();
    drop(create_fifo(&path));
    let (started, ready) = mpsc::channel();
    let source = Counted {
        file,
        reads: Arc::new(AtomicUsize::new(0)),
        started: Some(started),
    };
    let mut reader = FifoReader::new(source);
    let stop = StopSignal::new().unwrap();
    let stopped = stop.clone();
    let (done, finished) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut bytes = [0; 4];
        let complete = read_frame(&mut reader, &mut bytes, &stopped).unwrap();
        done.send((complete, bytes)).unwrap();
    });
    // The original descriptor must observe EOF before its writer opens.
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut writer = std::fs::OpenOptions::new()
        .write(true)
        .open(original)
        .unwrap();
    writer.write_all(&[1, 2, 3, 4]).unwrap();
    let result = finished.recv_timeout(Duration::from_secs(1));
    stop.request();
    worker.join().unwrap();
    assert_eq!(
        result.unwrap(),
        (true, [1, 2, 3, 4]),
        "T405: watched a replacement path instead of the owned FIFO"
    );
}

struct ReportWait<T>(T, mpsc::Sender<Waiting>);
impl<T: Read> Read for ReportWait<T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.read(bytes)
    }
}
impl<T: FrameSource> FrameSource for ReportWait<T> {
    fn wait(&mut self, waiting: Waiting, stop: &StopSignal) -> io::Result<bool> {
        let _ = self.1.send(waiting);
        self.0.wait(waiting, stop)
    }
}
struct ReaderTask {
    stop: Arc<StopSignal>,
    worker: Option<std::thread::JoinHandle<()>>,
    waiting: mpsc::Receiver<Waiting>,
    finished: mpsc::Receiver<io::Result<Option<[u8; 4]>>>,
}
impl ReaderTask {
    fn start(source: FifoReader<std::fs::File>) -> Self {
        let stop = StopSignal::new().unwrap();
        let stopped = stop.clone();
        let (wait, waiting) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut frame = [0; 4];
            let result = read_frame(&mut ReportWait(source, wait), &mut frame, &stopped)
                .map(|complete| complete.then_some(frame));
            let _ = done.send(result);
        });
        Self {
            stop,
            worker: Some(worker),
            waiting,
            finished,
        }
    }
    fn wait_for(&self, state: Waiting) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while self
            .waiting
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .unwrap()
            != state
        {}
    }
    fn frame(&self) -> Option<[u8; 4]> {
        self.finished
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap()
    }
}
impl Drop for ReaderTask {
    fn drop(&mut self) {
        self.stop.request();
        self.worker.take().unwrap().join().unwrap();
    }
}
fn open_writer(path: &std::path::Path) -> std::fs::File {
    std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .unwrap()
}

#[test]
fn t405_partial_eof_discards_old_bytes_before_writer_reconnect() {
    use std::io::Write;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("capture");
    let source = FifoReader::new(create_fifo(&path));
    let mut first = open_writer(&path);
    first.write_all(&[9, 8]).unwrap();
    let task = ReaderTask::start(source);
    task.wait_for(Waiting::Data);
    drop(first);
    task.wait_for(Waiting::Writer);
    let mut next = open_writer(&path);
    next.write_all(&[1, 2, 3, 4]).unwrap();
    assert_eq!(task.frame(), Some([1, 2, 3, 4]));
}

#[test]
fn t405_partial_frame_cancellation_needs_no_more_input() {
    use std::io::Write;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("capture");
    let source = FifoReader::new(create_fifo(&path));
    let mut writer = open_writer(&path);
    writer.write_all(&[1, 2]).unwrap();
    let task = ReaderTask::start(source);
    task.wait_for(Waiting::Data);
    task.stop.request();
    assert_eq!(task.frame(), None);
}

#[test]
fn t405_no_inotify_fallback_reconnects_and_still_cancels() {
    use std::io::Write;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("capture");
    let task = ReaderTask::start(FifoReader {
        source: create_fifo(&path),
        events: None,
    });
    task.wait_for(Waiting::Writer);
    open_writer(&path).write_all(&[4, 3, 2, 1]).unwrap();
    assert_eq!(task.frame(), Some([4, 3, 2, 1]));
    drop(task);
    let path = directory.path().join("cancel");
    let task = ReaderTask::start(FifoReader {
        source: create_fifo(&path),
        events: None,
    });
    task.wait_for(Waiting::Writer);
    task.stop.request();
    assert_eq!(task.frame(), None);
}

#[test]
fn t405_cancellation_latches_before_wait_and_descriptors_are_owned() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("capture");
    let mut source = FifoReader::new(create_fifo(&path));
    let stop = StopSignal::new().unwrap();
    let weak = Arc::downgrade(&stop);
    stop.request();
    stop.request();
    assert!(!source.wait(Waiting::Writer, &stop).unwrap());
    assert!(!source.wait(Waiting::Data, &stop).unwrap());
    assert!(!read_frame(&mut source, &mut [0; 4], &stop).unwrap());
    // Fds are RAII-owned; no reuse can occur between these drops and fcntl
    // in the project's serial test suite.
    let descriptors = [
        source.source.as_raw_fd(),
        source.events.as_ref().unwrap().as_raw_fd(),
        stop.event.as_raw_fd(),
    ];
    drop(source);
    drop(stop);
    assert!(weak.upgrade().is_none());
    for descriptor in descriptors {
        assert_eq!(unsafe { libc::fcntl(descriptor, libc::F_GETFD) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    }
}

#[test]
fn t405_inode_move_falls_back_without_losing_live_writer_data() {
    use std::io::Write;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("capture");
    let mut source = FifoReader::new(create_fifo(&path));
    let moved = directory.path().join("moved");
    std::fs::rename(&path, &moved).unwrap();
    let stop = StopSignal::new().unwrap();
    assert!(source.wait(Waiting::Writer, &stop).unwrap());
    assert!(source.events.is_none());
    open_writer(&moved).write_all(&[1, 2, 3, 4]).unwrap();
    let mut bytes = [0; 4];
    assert!(read_frame(&mut source, &mut bytes, &stop).unwrap());
    assert_eq!(bytes, [1, 2, 3, 4]);
}
