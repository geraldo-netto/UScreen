//! Linux descriptor transport. Packet boundaries and fd ownership are explicit.
use anyhow::{ensure, Context, Result};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;
use uscreen_config::raw_frame::{Message, MESSAGE_BYTES};

#[derive(Clone)]
pub(crate) struct Socket(Arc<OwnedFd>);

fn owned(fd: RawFd) -> std::io::Result<OwnedFd> {
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }
}

impl Socket {
    pub fn pair() -> Result<(Self, Self)> {
        let mut pair = [-1; 2];
        let result = unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                0,
                pair.as_mut_ptr(),
            )
        };
        ensure!(
            result == 0,
            "socketpair: {}",
            std::io::Error::last_os_error()
        );
        Ok((
            Self(Arc::new(owned(pair[0])?)),
            Self(Arc::new(owned(pair[1])?)),
        ))
    }

    /// Only the child endpoint crosses exec. The parent's endpoint stays CLOEXEC.
    pub fn attach(&self, command: &mut tokio::process::Command) -> Result<()> {
        // Reserve an owned descriptor above stdio before fork. A fixed dup target
        // could overwrite the exec-error pipe or another inherited resource.
        let child = owned(unsafe { libc::fcntl(self.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) })?;
        command
            .arg("--capture-socket-fd")
            .arg(child.as_raw_fd().to_string());
        unsafe {
            command.pre_exec(move || inherit_descriptor(child.as_raw_fd(), child.as_raw_fd()));
        }
        Ok(())
    }

    pub fn send(&self, message: Message, fd: Option<RawFd>) -> Result<()> {
        self.send_bytes(&message.encode(), fd)
    }

    pub fn send_bytes(&self, bytes: &[u8], fd: Option<RawFd>) -> Result<()> {
        let mut iov = libc::iovec {
            iov_base: bytes.as_ptr().cast_mut().cast(),
            iov_len: bytes.len(),
        };
        // usize provides cmsghdr alignment; spare space bounds malformed-control tests.
        let mut control = [0usize; 8];
        let mut header: libc::msghdr = unsafe { std::mem::zeroed() };
        header.msg_iov = &mut iov;
        header.msg_iovlen = 1;
        if let Some(fd) = fd {
            header.msg_control = control.as_mut_ptr().cast();
            header.msg_controllen = unsafe { libc::CMSG_SPACE(4) } as usize;
            unsafe {
                let cmsg = libc::CMSG_FIRSTHDR(&header);
                (*cmsg).cmsg_level = libc::SOL_SOCKET;
                (*cmsg).cmsg_type = libc::SCM_RIGHTS;
                (*cmsg).cmsg_len = libc::CMSG_LEN(4) as usize;
                std::ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<RawFd>(), fd);
            }
        }
        let result = unsafe { libc::sendmsg(self.as_raw_fd(), &header, libc::MSG_NOSIGNAL) };
        ensure!(
            result == bytes.len() as isize,
            "send raw packet: {}",
            std::io::Error::last_os_error()
        );
        Ok(())
    }

    /// All received descriptors become OwnedFd before payload validation.
    pub fn receive(&self) -> Result<Option<(Message, Vec<OwnedFd>)>> {
        let mut bytes = [0u8; MESSAGE_BYTES];
        let mut iov = libc::iovec {
            iov_base: bytes.as_mut_ptr().cast(),
            iov_len: bytes.len(),
        };
        let mut control = [0usize; 8];
        let mut header: libc::msghdr = unsafe { std::mem::zeroed() };
        header.msg_iov = &mut iov;
        header.msg_iovlen = 1;
        header.msg_control = control.as_mut_ptr().cast();
        header.msg_controllen = std::mem::size_of_val(&control);
        let result =
            unsafe { libc::recvmsg(self.as_raw_fd(), &mut header, libc::MSG_CMSG_CLOEXEC) };
        if result < 0 {
            return receive_error();
        }
        let fds = received_fds(&header)?;
        ensure!(result > 0, "raw capture peer closed");
        ensure!(
            header.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) == 0,
            "truncated raw packet"
        );
        let message = Message::decode(&bytes[..result as usize])?;
        Ok(Some((message, fds)))
    }

    pub fn wait(&self, stop: &crate::encoder_io::StopSignal) -> Result<bool> {
        let mut descriptors = [
            libc::pollfd {
                fd: self.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stop.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        while !stop.requested() {
            let ready = unsafe { libc::poll(descriptors.as_mut_ptr(), 2, -1) };
            if poll_result(ready)? {
                return Ok(!stop.requested());
            }
        }
        Ok(false)
    }
}

/// The only operations between fork and exec are async-signal-safe syscalls.
fn inherit_descriptor(source: RawFd, target: RawFd) -> std::io::Result<()> {
    let result = unsafe {
        if source == target {
            libc::fcntl(source, libc::F_SETFD, 0)
        } else {
            libc::dup3(source, target, 0)
        }
    };
    if result < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn poll_result(result: i32) -> Result<bool> {
    if result >= 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::Interrupted {
        return Ok(false);
    }
    Err(error).context("poll raw capture")
}

impl AsRawFd for Socket {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

fn receive_error() -> Result<Option<(Message, Vec<OwnedFd>)>> {
    let error = std::io::Error::last_os_error();
    if matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
    ) {
        Ok(None)
    } else {
        Err(error).context("receive raw packet")
    }
}

fn received_fds(header: &libc::msghdr) -> Result<Vec<OwnedFd>> {
    let mut fds = Vec::new();
    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(header);
        while !cmsg.is_null() {
            ensure!(
                (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS,
                "unexpected raw control message"
            );
            let size = (*cmsg).cmsg_len - libc::CMSG_LEN(0) as usize;
            for offset in (0..size).step_by(4) {
                fds.push(owned(std::ptr::read_unaligned(
                    libc::CMSG_DATA(cmsg).add(offset).cast::<RawFd>(),
                ))?);
            }
            cmsg = libc::CMSG_NXTHDR(header, cmsg);
        }
    }
    Ok(fds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t418_exec_handoff_preserves_unrelated_child_descriptor() {
        let (_parent, endpoint) = Socket::pair().unwrap();
        let sentinel = tempfile::tempfile().unwrap();
        let source = sentinel.as_raw_fd();
        let mut command = tokio::process::Command::new("/bin/true");
        unsafe {
            command.pre_exec(move || inherit_descriptor(source, 198));
        }
        endpoint.attach(&mut command).unwrap();
        unsafe {
            command.pre_exec(move || {
                let mut expected: libc::stat = std::mem::zeroed();
                let mut actual: libc::stat = std::mem::zeroed();
                if libc::fstat(source, &mut expected) != 0 || libc::fstat(198, &mut actual) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::close(198);
                if (actual.st_dev, actual.st_ino) != (expected.st_dev, expected.st_ino) {
                    return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
                }
                Ok(())
            });
        }
        assert!(
            command.as_std_mut().status().unwrap().success(),
            "T418: inherited descriptor was overwritten"
        );
    }

    #[test]
    fn t418_native_error_policy_and_exec_descriptor_ownership() {
        for error in [libc::EINTR, libc::EAGAIN, libc::EIO] {
            unsafe {
                *libc::__errno_location() = error;
            }
            assert_eq!(receive_error().is_ok(), error != libc::EIO);
            unsafe {
                *libc::__errno_location() = error;
            }
            assert_eq!(poll_result(-1).is_ok(), error == libc::EINTR);
        }
        assert!(poll_result(1).unwrap());
        assert!(owned(-1).is_err());
        let (a, b) = Socket::pair().unwrap();
        let target =
            owned(unsafe { libc::fcntl(a.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 64) }).unwrap();
        inherit_descriptor(target.as_raw_fd(), target.as_raw_fd()).unwrap();
        inherit_descriptor(a.as_raw_fd(), target.as_raw_fd()).unwrap();
        assert_eq!(
            unsafe { libc::fcntl(target.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        assert!(inherit_descriptor(-1, target.as_raw_fd()).is_err());
        drop(target);
        drop(b);
        assert!(a.receive().is_err());
    }
}
