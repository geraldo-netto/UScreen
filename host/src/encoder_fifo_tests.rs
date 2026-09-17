//! T405: real private FIFOs, no encoder, display, or input device.
use super::*;
use crate::encoder_io::read_frame;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

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
